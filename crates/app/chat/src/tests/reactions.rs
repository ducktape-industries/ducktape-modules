//! Reactions: one per party and emoji, counted on the message.
use super::*;
use crate::MAX_EMOJI_BYTES;

#[test]
fn a_reaction_counts_once_per_party_and_knows_its_reader() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "ship it", None);
    chat.ok(&ADA, react(1, "👍", true));
    chat.ok(&BO, react(1, "👍", true));
    let twice = chat.store.state.clone();
    chat.ok(&BO, react(1, "👍", true));
    assert_eq!(chat.store.state, twice, "choosing it again changes nothing");
    let seen_by = |viewer: Vec<Party>| {
        let Reply::Roots(page) = chat.ask(Query::Roots {
            channel_id: "general".into(),
            viewer,
            page: Page::default(),
        }) else {
            panic!("roots answer roots");
        };
        let reaction = page.items[0].reactions[0].clone();
        (reaction.count, reaction.reacted_by_me)
    };
    assert_eq!(seen_by(vec![ADA]), (2, true));
    assert_eq!(seen_by(vec![CY]), (2, false));
}

#[test]
fn removing_a_reaction_uncounts_it_and_the_last_one_leaves() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "ship it", None);
    chat.ok(&ADA, react(1, "👍", true));
    chat.ok(&BO, react(1, "👍", true));
    chat.ok(&ADA, react(1, "👍", false));
    assert_eq!(chat.message(1).reactions[0].count, 1);
    let unchosen = chat.store.state.clone();
    chat.ok(&CY, react(1, "👍", false));
    assert_eq!(chat.store.state, unchosen, "dropping what was never chosen");
    chat.ok(&BO, react(1, "👍", false));
    assert!(chat.message(1).reactions.is_empty());
}

#[test]
fn a_reaction_needs_an_emoji_a_standing_message_and_a_seat() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    chat.post(&ADA, "m1", "members only", None);
    let long = "x".repeat(MAX_EMOJI_BYTES + 1);
    for bad in ["", "a/b", long.as_str()] {
        assert_eq!(
            chat.refused(&ADA, react(1, bad, true)),
            reason::INVALID_INPUT
        );
    }
    assert_eq!(
        chat.refused(&BO, react(1, "👍", true)),
        reason::UNAUTHORIZED
    );
    assert_eq!(chat.refused(&ADA, react(9, "👍", true)), reason::NOT_FOUND);
    chat.ok(&ADA, delete(1));
    assert_eq!(
        chat.refused(&ADA, react(1, "👍", true)),
        reason::WRONG_STATE
    );
}

/// A deleted message leaves the store as if no one had reacted to it: the
/// reaction markers go with it (they once stayed forever).
#[test]
fn deleting_a_message_drops_its_reactions() {
    let reacted_then_deleted = |react_first: bool| {
        let mut chat = Chat::with_channel(PostPolicy::Open);
        chat.post(&ADA, "m1", "hi", None);
        chat.post(&ADA, "m2", "stays", None);
        chat.ok(&BO, react(2, "👍", true));
        if react_first {
            for (who, emoji) in [(&ADA, "👍"), (&BO, "🎉"), (&CY, "👍")] {
                chat.ok(who, react(1, emoji, true));
            }
        }
        chat.ok(&ADA, delete(1));
        chat.store.state
    };
    let reacted = reacted_then_deleted(true);
    assert_eq!(reacted, reacted_then_deleted(false));
    assert_eq!(
        markers(&reacted, 2),
        1,
        "the other message keeps its reaction"
    );
}

/// How many reaction markers message `seq` of `#general` holds.
fn markers(state: &std::collections::BTreeMap<Vec<u8>, Vec<u8>>, seq: u64) -> usize {
    let prefix = crate::state::REACTIONS.key(&("general".to_string(), seq));
    state.keys().filter(|key| key.starts_with(&prefix)).count()
}
