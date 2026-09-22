use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct Memory(BTreeMap<Vec<u8>, Vec<u8>>);

impl Read for Memory {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
    fn scan(&self, scan: Scan) -> Vec<Entry> {
        let mut hits: Vec<Entry> = self
            .0
            .iter()
            .filter(|(key, _)| scan.admits(key))
            .map(|(key, value)| Entry {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        if scan.reverse {
            hits.reverse();
        }
        if let Some(limit) = scan.limit {
            hits.truncate(limit as usize);
        }
        hits
    }
}

impl Write for Memory {
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.0.insert(key, value);
    }
    fn delete(&mut self, key: &[u8]) {
        self.0.remove(key);
    }
}

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

    let ChatViewReply::Roots {
        roots, has_more, ..
    } = query(
        &store,
        ChatViewQuery::Roots {
            channel_id: "general".into(),
            viewer_handles: vec!["acct:1".into()],
            before_seq: None,
            limit: Some(10),
        },
    )
    .unwrap()
    else {
        panic!()
    };
    assert!(!has_more);
    assert_eq!(roots.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![1, 3]);
    assert_eq!(roots[0].reply_count, 1);
    assert_eq!(roots[0].tags, vec!["world"]);
    assert!(roots[0].reactions[0].reacted_by_me);

    let ChatViewReply::Thread { root, replies, .. } = query(
        &store,
        ChatViewQuery::Thread {
            channel_id: "general".into(),
            root_seq: 1,
            viewer_handles: vec![],
            after_reply_seq: None,
            limit: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!((root.unwrap().seq, replies[0].seq), (1, 2));

    let ChatViewReply::Hits(hits) = query(
        &store,
        ChatViewQuery::Search {
            text: "hello".into(),
            viewer_handles: vec![],
            channel_id: None,
            limit: None,
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
        ChatViewQuery::TagSearch {
            tag: "#World".into(),
            viewer_handles: vec![],
            channel_id: Some("general".into()),
            after: None,
            limit: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(tags.hits[0].seq, 1);

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
        ChatViewQuery::Search {
            text: "hello".into(),
            viewer_handles: vec![],
            channel_id: Some("general".into()),
            limit: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(hits.hits.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![2]);
}
