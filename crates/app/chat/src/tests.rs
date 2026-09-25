use super::*;

use store::Memory;

fn frame(party: Party) -> Frame {
    Frame {
        party,
        height: 1,
        time: 1000,
    }
}

fn post(
    store: &mut Memory,
    who: u64,
    ch: &str,
    id: &str,
    text: &str,
    thread: Option<u64>,
) -> Result<(), Refusal> {
    execute(
        store,
        &frame(Party::Account(who)),
        ChatMsg::PostMessage {
            channel_id: ch.into(),
            message_id: id.into(),
            blocks: parse_message(text),
            thread,
        },
    )
}

#[test]
fn a_channel_takes_posts_threads_reactions_and_answers_the_view() {
    let mut store = Memory::default();
    let ada = frame(Party::Account(1));
    execute(
        &mut store,
        &ada,
        ChatMsg::CreateChannel {
            channel_id: "general".into(),
            name: "General".into(),
            post_policy: PostPolicy::MembersOnly,
        },
    )
    .unwrap();
    assert_eq!(
        post(&mut store, 2, "general", "m1", "hi", None)
            .unwrap_err()
            .reason,
        reason::UNAUTHORIZED
    );
    execute(
        &mut store,
        &ada,
        ChatMsg::SetMembership {
            channel_id: "general".into(),
            party: Party::Account(2),
            member: true,
        },
    )
    .unwrap();
    post(&mut store, 2, "general", "m1", "hello #World", None).unwrap();
    post(&mut store, 1, "general", "m2", "hello back", Some(1)).unwrap();
    post(&mut store, 1, "general", "m3", "another root", None).unwrap();
    assert_eq!(
        post(&mut store, 1, "general", "m3", "dup", None)
            .unwrap_err()
            .reason,
        reason::ALREADY_EXISTS
    );
    execute(
        &mut store,
        &ada,
        ChatMsg::AddReaction {
            channel_id: "general".into(),
            seq: 1,
            emoji: "👍".into(),
        },
    )
    .unwrap();

    let ChatViewReply::Roots(page) = query(
        &store,
        9,
        ChatViewQuery::Roots {
            channel_id: "general".into(),
            viewer_handles: vec!["acct:1".into()],
            page: Page::first(10),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((page.height, page.next), (9, None));
    let roots = page.items;
    assert_eq!(
        roots.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![3, 1],
        "newest first"
    );
    let roots: Vec<_> = roots.into_iter().rev().collect();
    assert_eq!(roots[0].reply_count, 1);
    assert_eq!(roots[0].tags, vec!["world"]);
    assert!(roots[0].reactions[0].reacted_by_me);

    let ChatViewReply::Thread { root, replies } = query(
        &store,
        9,
        ChatViewQuery::Thread {
            channel_id: "general".into(),
            root_seq: 1,
            viewer_handles: vec![],
            page: Page::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((root.unwrap().seq, replies.items[0].seq), (1, 2));

    let ChatViewReply::Hits(hits) = query(
        &store,
        9,
        ChatViewQuery::Search {
            text: "hello".into(),
            viewer_handles: vec![],
            channel_id: None,
            page: Page::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        hits.hits.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![2, 1]
    );

    let ChatViewReply::TagHits(tags) = query(
        &store,
        9,
        ChatViewQuery::TagSearch {
            tag: "#World".into(),
            viewer_handles: vec![],
            channel_id: Some("general".into()),
            page: Page::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(tags.items[0].seq, 1);

    execute(
        &mut store,
        &frame(Party::Account(2)),
        ChatMsg::DeleteMessage {
            channel_id: "general".into(),
            seq: 1,
        },
    )
    .unwrap();
    let ChatViewReply::Hits(hits) = query(
        &store,
        9,
        ChatViewQuery::Search {
            text: "hello".into(),
            viewer_handles: vec![],
            channel_id: Some("general".into()),
            page: Page::default(),
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(hits.hits.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn a_handle_reads_back_as_its_party() {
    for party in [
        Party::Account(7),
        Party::Key(vec![0xab, 0x01]),
        Party::Module("forge".into()),
        Party::System,
    ] {
        assert_eq!(party_of_handle(&party_handle(&party)), Some(party));
    }
    for nothing in ["acct:x", "user:", "user:abc", "user:zz", "someone"] {
        assert_eq!(party_of_handle(nothing), None, "{nothing}");
    }
}

#[test]
fn a_dm_title_names_no_account_numbers_and_its_accounts_are_fields() {
    use describe::Value;
    let dm = dm_channel_id(1, 3);
    let react = describe(&ChatMsg::AddReaction {
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
    let edit = describe(&ChatMsg::EditMessage {
        channel_id: dm,
        seq: 4,
        blocks: Vec::new(),
        base_rev: None,
    });
    assert_eq!(edit.title, "Edit message · DM");
    // a channel keeps the name a person picked, and no `between`
    let channel = describe(&ChatMsg::DeleteMessage {
        channel_id: "old-launch".into(),
        seq: 1,
    });
    assert_eq!(channel.title, "Delete message · #old-launch");
    assert!(channel.fields.iter().all(|f| f.label != "between"));
}
