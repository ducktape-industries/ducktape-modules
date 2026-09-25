//! Channels: creating, dm rooms, renaming, archiving, membership.
use super::*;
use crate::{ChannelInfo, MAX_ID_BYTES, dm_channel_id};

#[test]
fn a_channel_is_created_once_owned_by_its_creator() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let Reply::Channel(Some(ChannelInfo { channel, head_seq })) = chat.ask(Query::Channel {
        channel_id: "general".into(),
    }) else {
        panic!("the channel reads back");
    };
    assert_eq!((channel.owner, channel.voice, head_seq), (ADA, false, 0));
    let again = chat.refused(&BO, create("general", PostPolicy::Open));
    assert_eq!(again, reason::ALREADY_EXISTS);
}

#[test]
fn a_channel_id_is_bounded_and_a_colon_id_is_its_programs_alone() {
    let mut chat = Chat::default();
    let long = "x".repeat(MAX_ID_BYTES + 1);
    for bad in ["", "a/b", long.as_str()] {
        assert_eq!(
            chat.refused(&ADA, create(bad, PostPolicy::Open)),
            reason::INVALID_INPUT
        );
    }
    for who in [ADA, Party::Key(vec![1]), Party::Module("for".into())] {
        let squat = chat.refused(&who, create("forge:repo:1", PostPolicy::Open));
        assert_eq!(squat, reason::UNAUTHORIZED);
    }
    chat.ok(
        &Party::Module("forge".into()),
        create("forge:repo:1", PostPolicy::Open),
    );
    chat.ok(&Party::System, create("system:room", PostPolicy::Open));
    let blank = Op::CreateChannel {
        channel_id: "blank".into(),
        name: "  ".into(),
        post_policy: PostPolicy::Open,
    };
    assert_eq!(chat.refused(&ADA, blank), reason::INVALID_INPUT);
}

#[test]
fn a_voice_channel_is_open_and_marked_voice() {
    let mut chat = Chat::default();
    let voice = |id: &str| Op::CreateVoiceChannel {
        channel_id: id.into(),
        name: "standup".into(),
    };
    chat.ok(&ADA, voice("standup"));
    let channel = crate::state::channel(&chat.store, "standup").unwrap();
    assert!(channel.voice && channel.post_policy == PostPolicy::Open);
    assert_eq!(chat.refused(&ADA, voice("forge:x")), reason::UNAUTHORIZED);
}

#[test]
fn nobody_squats_a_dm_id_with_a_plain_create() {
    let dm = dm_channel_id(3, 5);
    // a third account, and each peer, try the plain creates on the dm id
    for who in [7, 3, 5] {
        let mut chat = Chat::default();
        let who = Party::Account(who);
        for op in [
            create(&dm, PostPolicy::Open),
            Op::CreateVoiceChannel {
                channel_id: dm.clone(),
                name: "mine".into(),
            },
        ] {
            assert_eq!(chat.refused(&who, op), reason::UNAUTHORIZED, "{who:?}");
        }
        assert!(chat.store.state.is_empty());
        // the real dm still opens with both peers seated
        chat.ok(&Party::Account(3), open_dm(5));
        for peer in [3, 5] {
            assert!(is_member(&chat, &dm, Party::Account(peer)));
        }
    }
}

#[test]
fn a_dm_seats_both_accounts_opens_once_and_keeps_others_out() {
    let mut chat = Chat::default();
    chat.ok(&ADA, open_dm(2));
    let dm = dm_channel_id(1, 2);
    let first = chat.store.state.clone();
    chat.ok(&BO, open_dm(1));
    assert_eq!(chat.store.state, first, "opening it again changes nothing");
    chat.ok(&BO, post(&dm, "m1", "hi", None));
    assert_eq!(
        chat.refused(&CY, post(&dm, "m2", "me too", None)),
        reason::UNAUTHORIZED
    );
    assert_eq!(chat.refused(&ADA, open_dm(1)), reason::INVALID_INPUT);
    let key = Party::Key(vec![9]);
    assert_eq!(chat.refused(&key, open_dm(1)), reason::UNAUTHORIZED);
}

/// The peer who opened a dm owns it, but owning it gives nothing: neither
/// peer adds a third account, removes the other, renames or archives it.
#[test]
fn neither_dm_peer_reshapes_the_room() {
    let mut chat = Chat::default();
    chat.ok(&ADA, open_dm(2));
    let dm = dm_channel_id(1, 2);
    let seat = |party: Party, member: bool| Op::SetMembership {
        channel_id: dm.clone(),
        party,
        member,
    };
    for (who, party, member) in [
        (&ADA, CY, true),
        (&ADA, BO, false),
        (&ADA, ADA, false),
        (&BO, CY, true),
        (&BO, ADA, false),
        (&CY, CY, true),
    ] {
        assert_eq!(chat.refused(who, seat(party, member)), reason::UNAUTHORIZED);
    }
    let rename = |name: &str| Op::RenameChannel {
        channel_id: dm.clone(),
        name: name.into(),
    };
    let archive = |archived| Op::SetChannelArchived {
        channel_id: dm.clone(),
        archived,
    };
    for who in [&ADA, &BO, &CY] {
        assert_eq!(chat.refused(who, rename("ours")), reason::UNAUTHORIZED);
        assert_eq!(chat.refused(who, archive(true)), reason::UNAUTHORIZED);
        assert_eq!(chat.refused(who, archive(false)), reason::UNAUTHORIZED);
    }
    chat.ok(&BO, post(&dm, "m1", "still here", None));
    assert_eq!(
        chat.refused(&CY, post(&dm, "m2", "let me in", None)),
        reason::UNAUTHORIZED
    );
}

#[test]
fn only_the_owner_renames() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, rename("General chat"));
    assert_eq!(chat.channel().name, "General chat");
    assert_eq!(chat.refused(&BO, rename("mine")), reason::UNAUTHORIZED);
    assert_eq!(chat.refused(&ADA, rename("")), reason::INVALID_INPUT);
    let nowhere = Op::RenameChannel {
        channel_id: "nowhere".into(),
        name: "x".into(),
    };
    assert_eq!(chat.refused(&ADA, nowhere), reason::NOT_FOUND);
}

#[test]
fn an_archived_channel_takes_no_writes_until_unarchived() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello", None);
    assert_eq!(chat.refused(&BO, archive(true)), reason::UNAUTHORIZED);
    chat.ok(&ADA, archive(true));
    for op in [
        post("general", "m2", "late", None),
        react(1, "👍", true),
        edit(1, "changed"),
    ] {
        assert_eq!(chat.refused(&BO, op), reason::WRONG_STATE);
    }
    chat.ok(&ADA, archive(false));
    chat.post(&BO, "m2", "back", None);
}

#[test]
fn the_owner_seats_and_unseats_members() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    assert_eq!(
        chat.refused(&BO, post("general", "m1", "hi", None)),
        reason::UNAUTHORIZED
    );
    chat.ok(&ADA, membership(BO, true));
    chat.post(&BO, "m1", "hi", None);
    let Reply::Members(members) = chat.ask(Query::Members {
        channel_id: "general".into(),
        page: Page::default(),
    }) else {
        panic!("members answer members");
    };
    assert_eq!(members.items[0].party, BO);
    assert_eq!(
        chat.refused(&BO, membership(CY, true)),
        reason::UNAUTHORIZED
    );
    chat.ok(&ADA, membership(BO, false));
    assert!(!is_member(&chat, "general", BO));
    assert_eq!(
        chat.refused(&BO, post("general", "m2", "hi", None)),
        reason::UNAUTHORIZED
    );
}

pub(super) fn open_dm(counterpart: u64) -> Op {
    Op::CreateDmChannel {
        counterpart,
        name: "dm".into(),
    }
}

fn rename(name: &str) -> Op {
    Op::RenameChannel {
        channel_id: "general".into(),
        name: name.into(),
    }
}

pub(super) fn archive(archived: bool) -> Op {
    Op::SetChannelArchived {
        channel_id: "general".into(),
        archived,
    }
}

fn membership(party: Party, member: bool) -> Op {
    Op::SetMembership {
        channel_id: "general".into(),
        party,
        member,
    }
}

pub(super) fn edit(seq: u64, text: &str) -> Op {
    Op::EditMessage {
        channel_id: "general".into(),
        seq,
        blocks: parse_message(text),
        base_rev: None,
    }
}

fn is_member(chat: &Chat, channel: &str, party: Party) -> bool {
    crate::state::MEMBERS.has(&chat.store, &(channel.to_owned(), party))
}
