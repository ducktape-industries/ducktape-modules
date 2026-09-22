use abi::{Entry, Scan, reason};
use chat::{ChatMsg, Frame, Party, PostPolicy, Read, Write};
use std::collections::BTreeMap;
#[derive(Default)]
struct Memory(BTreeMap<Vec<u8>, Vec<u8>>);
impl Read for Memory {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.0
            .iter()
            .filter(|(k, _)| scan.admits(k))
            .map(|(k, v)| Entry {
                key: k.clone(),
                value: v.clone(),
            })
            .collect()
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
#[test]
fn channel_and_system_message_ids_belong_to_the_exact_program_prefix() {
    let mut store = Memory::default();
    for voice in [false, true] {
        for party in [
            Party::Key(vec![1]),
            Party::Account(1),
            Party::Module("other".into()),
            Party::Module("for".into()),
        ] {
            let op = if voice {
                ChatMsg::CreateVoiceChannel {
                    channel_id: "forge:repo:1".into(),
                    name: "Change".into(),
                }
            } else {
                ChatMsg::CreateChannel {
                    channel_id: "forge:repo:1".into(),
                    name: "Change".into(),
                    post_policy: PostPolicy::Open,
                }
            };
            let error = chat::execute(
                &mut store,
                &Frame {
                    party,
                    height: 1,
                    time: 1,
                },
                op,
            )
            .unwrap_err();
            assert_eq!(error.reason, reason::UNAUTHORIZED);
            assert!(store.0.is_empty());
        }
    }
    let forge = Frame {
        party: Party::Module("forge".into()),
        height: 1,
        time: 1,
    };
    chat::execute(
        &mut store,
        &forge,
        ChatMsg::CreateChannel {
            channel_id: "forge:repo:1".into(),
            name: "Change".into(),
            post_policy: PostPolicy::Open,
        },
    )
    .unwrap();
    let message = ChatMsg::PostMessage {
        channel_id: "forge:repo:1".into(),
        message_id: "forge:0001".into(),
        blocks: vec![chat::Block::paragraph("opened")],
        thread: None,
    };
    let before = store.0.clone();
    assert_eq!(
        chat::execute(
            &mut store,
            &Frame {
                party: Party::Key(vec![1]),
                height: 2,
                time: 2
            },
            message.clone()
        )
        .unwrap_err()
        .reason,
        reason::UNAUTHORIZED
    );
    assert_eq!(store.0, before);
    chat::execute(&mut store, &forge, message).unwrap();
    let chat::ChatViewReply::Message(Some(row)) = chat::query(
        &store,
        chat::ChatViewQuery::MessageById {
            message_id: "forge:0001".into(),
        },
    )
    .unwrap() else {
        panic!("message lookup");
    };
    assert_eq!((row.seq, row.author.as_str()), (1, "module:forge"));
    chat::execute(
        &mut store,
        &Frame {
            party: Party::Key(vec![1]),
            height: 2,
            time: 2,
        },
        ChatMsg::PostMessage {
            channel_id: "forge:repo:1".into(),
            message_id: "reply-1".into(),
            blocks: vec![chat::Block::paragraph("hello")],
            thread: Some(1),
        },
    )
    .unwrap();
    for party in [Party::Key(vec![2]), Party::System] {
        let id = if party == Party::System {
            "system:room"
        } else {
            "ordinary"
        };
        chat::execute(
            &mut store,
            &Frame {
                party,
                height: 3,
                time: 3,
            },
            ChatMsg::CreateChannel {
                channel_id: id.into(),
                name: id.into(),
                post_policy: PostPolicy::Open,
            },
        )
        .unwrap();
    }
}
