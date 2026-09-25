//! Messages: posting, threads, editing, deleting, and the reads over them.
use super::channels::edit;
use super::*;
use crate::{MAX_MESSAGE_BYTES, roots_below};

#[test]
fn posts_land_as_roots_newest_first_and_page_older() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    for n in 1..=5 {
        chat.post(&BO, &format!("m{n}"), &format!("note {n}"), None);
    }
    let roots = |after| {
        let Reply::Roots(page) = chat.ask(Query::Roots {
            channel_id: "general".into(),
            viewer: vec![],
            page: Page {
                after,
                limit: Some(2),
            },
        }) else {
            panic!("roots answer roots");
        };
        page
    };
    let newest = roots(None);
    assert_eq!(seqs(&newest.items), [5, 4]);
    assert_eq!(seqs(&roots(newest.next).items), [3, 2]);
    assert_eq!(seqs(&roots(Some(roots_below("general", 2))).items), [1]);
    assert_eq!(chat.message(5).author, BO);
}

#[test]
fn a_message_id_is_unique_and_a_colon_id_is_its_programs_alone() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hi", None);
    let again = chat.refused(&CY, post("general", "m1", "hi", None));
    assert_eq!(again, reason::ALREADY_EXISTS);
    let squat = chat.refused(&BO, post("general", "forge:0001", "hi", None));
    assert_eq!(squat, reason::UNAUTHORIZED);
    chat.ok(
        &Principal::Module("forge".into()),
        post("general", "forge:0001", "opened", None),
    );
    let Reply::Message(Some(row)) = chat.ask(Query::MessageById {
        message_id: "forge:0001".into(),
    }) else {
        panic!("the id finds its message");
    };
    assert_eq!(row.seq, 2);
    let nowhere = chat.refused(&BO, post("nowhere", "m9", "hi", None));
    assert_eq!(nowhere, reason::NOT_FOUND);
}

#[test]
fn a_message_is_at_most_its_byte_cap() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let huge = "word ".repeat(MAX_MESSAGE_BYTES / 5 + 1);
    assert_eq!(
        chat.refused(&BO, post("general", "big", &huge, None)),
        reason::CAPACITY
    );
}

#[test]
fn a_reply_joins_its_root_and_the_authors_attention() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "m1", "question", None);
    chat.post(&BO, "m2", "answer", Some(1));
    chat.post(&CY, "m3", "another", Some(1));
    let root = chat.message(1);
    assert_eq!((root.reply_count, root.last_reply_seq), (2, Some(3)));
    let Reply::Thread { root, replies } = chat.ask(Query::Thread {
        channel_id: "general".into(),
        root_seq: 1,
        viewer: vec![],
        page: Page::default(),
    }) else {
        panic!("a thread answers a thread");
    };
    assert_eq!((root.unwrap().seq, seqs(&replies.items)), (1, vec![2, 3]));
    assert_eq!(attention(&chat, ADA).map(|row| row.seq), Some(1));
    assert_eq!(attention(&chat, BO), None);
    let nested = chat.refused(&CY, post("general", "m4", "deeper", Some(2)));
    assert_eq!(nested, reason::INVALID_INPUT);
    let orphan = chat.refused(&CY, post("general", "m4", "nowhere", Some(99)));
    assert_eq!(orphan, reason::NOT_FOUND);
}

#[test]
fn only_the_author_edits_and_search_follows_the_edit() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello #launch", None);
    chat.ok(&BO, edit(1, "goodbye"));
    let row = chat.message(1);
    assert_eq!(
        (row.rev, row.edited, row.text.as_str()),
        (1, true, "goodbye")
    );
    assert_eq!(
        (chat.search("hello"), chat.search("goodbye")),
        (vec![], vec![1])
    );
    assert!(tagged(&chat, "launch").is_empty());
    assert_eq!(chat.refused(&CY, edit(1, "mine")), reason::UNAUTHORIZED);
    assert_eq!(chat.refused(&ADA, edit(1, "owner")), reason::UNAUTHORIZED);
    chat.ok(&BO, delete(1));
    assert_eq!(chat.refused(&BO, edit(1, "undead")), reason::WRONG_STATE);
}

#[test]
fn the_author_or_the_owner_deletes_and_a_tombstone_stays() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello #launch", None);
    chat.post(&BO, "m2", "hello again", Some(1));
    assert_eq!(chat.refused(&CY, delete(1)), reason::UNAUTHORIZED);
    chat.ok(&ADA, delete(1));
    let tombstone = chat.message(1);
    assert!(tombstone.deleted && tombstone.blocks.is_empty() && tombstone.text.is_empty());
    assert_eq!(tombstone.reply_count, 1, "the thread under it stands");
    assert_eq!(chat.search("hello"), [2]);
    assert!(tagged(&chat, "launch").is_empty());
    assert_eq!(attention(&chat, BO), None);
    let once = chat.store.borrow().state.clone();
    chat.ok(&BO, delete(1));
    assert_eq!(
        chat.store.borrow().state,
        once,
        "deleting twice changes nothing"
    );
    chat.ok(&BO, delete(2));
}

#[test]
fn search_needs_every_word_and_tags_page_newest_first() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "Hello #World", None);
    chat.post(&BO, "m2", "hello there #world", None);
    chat.post(&BO, "m3", "there", None);
    assert_eq!(chat.search("HELLO"), [2, 1]);
    assert_eq!(chat.search("hello there"), [2]);
    let empty = crate::Chat::query(
        &chat.reads(),
        Query::Search {
            text: "!".into(),
            viewer: vec![],
            channel_id: None,
            page: Page::default(),
        },
    );
    assert_eq!(empty.unwrap_err().reason, reason::INVALID_INPUT);
    assert_eq!(tagged(&chat, "#WORLD"), [2, 1]);
}

fn seqs(rows: &[crate::MsgRow]) -> Vec<u64> {
    rows.iter().map(|row| row.seq).collect()
}

fn attention(chat: &Chat, author: Principal) -> Option<crate::MsgRow> {
    let Reply::Attention(row) = chat.ask(Query::ThreadAttention {
        channel_id: "general".into(),
        author,
    }) else {
        panic!("attention answers attention");
    };
    row
}

/// The seqs tagged `tag`, in the channel and across channels (the same).
fn tagged(chat: &Chat, tag: &str) -> Vec<u64> {
    let ask = |channel_id| {
        let Reply::TagHits(page) = chat.ask(Query::TagSearch {
            tag: tag.into(),
            viewer: vec![],
            channel_id,
            page: Page::default(),
        }) else {
            panic!("a tag search answers tag hits");
        };
        seqs(&page.items)
    };
    let everywhere = ask(None);
    assert_eq!(ask(Some("general".into())), everywhere);
    everywhere
}

#[test]
fn a_read_names_a_bounded_number_of_viewers() {
    let chat = Chat::with_channel(PostPolicy::Open);
    let roots = |viewer: Vec<Principal>| Query::Roots {
        channel_id: "general".into(),
        viewer,
        page: Page::first(8),
    };
    let most = (0..crate::MAX_VIEWERS as u64)
        .map(Principal::Account)
        .collect();
    crate::Chat::query(&chat.reads(), roots(most)).unwrap();
    let over = (0..=crate::MAX_VIEWERS as u64)
        .map(Principal::Account)
        .collect();
    let refusal = crate::Chat::query(&chat.reads(), roots(over)).unwrap_err();
    assert_eq!(refusal.reason, reason::CAPACITY);
}
