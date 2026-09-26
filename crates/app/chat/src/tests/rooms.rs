//! Huddles, and how an op reads in the explorer (`describe`).
use super::channels::open_dm;
use super::*;
use crate::{HUDDLE_NODE_KEY_BYTES, MAX_HUDDLE_MEMBERS, describe, dm_channel_id};

fn join(node: u8) -> Op {
    Op::JoinHuddle {
        channel_id: "general".into(),
        node: vec![node; HUDDLE_NODE_KEY_BYTES],
        node_proof: vec![],
    }
}

fn leave() -> Op {
    Op::LeaveHuddle {
        channel_id: "general".into(),
    }
}

#[test]
fn a_person_takes_one_huddle_seat_and_moves_it_to_a_new_node() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&BO, join(1));
    chat.ok(&BO, join(2));
    let huddle = chat.channel().huddle;
    assert_eq!(huddle.len(), 1);
    assert_eq!(
        (huddle[0].principal.clone(), huddle[0].node.clone()),
        (BO, "02".repeat(32))
    );
    let module = FORGE;
    assert_eq!(chat.refused(&module, join(1)), code::UNAUTHORIZED);
    let short = Op::JoinHuddle {
        channel_id: "general".into(),
        node: vec![1],
        node_proof: vec![],
    };
    assert_eq!(chat.refused(&CY, short), code::INVALID_INPUT);
}

#[test]
fn a_huddle_is_bounded_and_seats_only_who_may_write() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    assert_eq!(chat.refused(&BO, join(1)), code::UNAUTHORIZED);
    let mut chat = Chat::with_channel(PostPolicy::Open);
    for n in 0..MAX_HUDDLE_MEMBERS as u64 {
        chat.ok(&Principal::Account(100 + n), join(1));
    }
    assert_eq!(chat.refused(&BO, join(1)), code::CAPACITY);
}

#[test]
fn leaving_a_huddle_frees_the_seat() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&BO, join(1));
    chat.ok(&BO, leave());
    assert!(chat.channel().huddle.is_empty());
    let unseated = chat.store.borrow().state.clone();
    chat.ok(&CY, leave());
    assert_eq!(
        chat.store.borrow().state,
        unseated,
        "leaving unseated changes nothing"
    );
    let nowhere = Op::LeaveHuddle {
        channel_id: "nowhere".into(),
    };
    assert_eq!(chat.refused(&CY, nowhere), code::NOT_FOUND);
}

#[test]
fn a_dm_title_names_no_account_numbers_and_its_accounts_are_fields() {
    use describe::Value;
    let dm = dm_channel_id(1, 3);
    let react = describe(&Op::AddReaction {
        channel_id: dm.clone(),
        seq: 4,
        emoji: "👍".into(),
    });
    assert_eq!(react.title, "React · DM");
    let between = react.fields.iter().find(|f| f.label == "between").unwrap();
    assert_eq!(
        between.value,
        Value::List(vec![Value::Account(1), Value::Account(3)])
    );
    let edit = describe(&Op::EditMessage {
        channel_id: dm,
        seq: 4,
        blocks: Vec::new(),
        base_rev: None,
    });
    assert_eq!(edit.title, "Edit message · DM");
    // a channel keeps the name a person picked, and no `between`
    let channel = describe(&Op::DeleteMessage {
        channel_id: "old-launch".into(),
        seq: 1,
    });
    assert_eq!(channel.title, "Delete message · #old-launch");
    assert!(channel.fields.iter().all(|f| f.label != "between"));
    assert_eq!(describe(&open_dm(3)).title, "Open a DM");
    // a created channel is titled by the name a person picked, not its id
    let created = describe(&Op::CreateChannel {
        channel_id: "channel-18d8db5b".into(),
        name: "[qa] probe".into(),
        post_policy: PostPolicy::Open,
    });
    assert_eq!(created.title, "Create channel · #[qa] probe");
    assert_eq!(
        describe(&post("design", "m", "hi", None)).title,
        "Post in #design"
    );
}
