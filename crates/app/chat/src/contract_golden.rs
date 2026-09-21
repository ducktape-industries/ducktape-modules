//! Immutable owner vectors for the consensus chat edges.
//!
//! Consumers copy only the fields they use and pin those local codecs to these
//! bytes. These values are the b66f47f owner encodings, not consumer-generated
//! round trips; changing one is a wire-contract change.

pub const CREATE_CHANNEL: &[u8] =
    br#"{"create_channel":{"channel_id":"general","name":"General","post_policy":"open"}}"#;
pub const REGISTER_HOOK: &[u8] =
    br#"{"register_hook":{"channel_id":"general","module_id":"automations"}}"#;
pub const POST_MESSAGE: &[u8] = br#"{"post_message":{"channel_id":"general","message_id":"m1","blocks":[{"paragraph":[{"text":"hello","marks":[]}]}],"thread":null}}"#;

pub const CHANNEL_QUERY: &[u8] = br#"{"channel":{"channel_id":"general"}}"#;
pub const MESSAGES_RANGE_QUERY: &[u8] =
    br#"{"messages_range":{"channel_id":"general","from_seq":1,"limit":64}}"#;
pub const MESSAGE_QUERY: &[u8] = br#"{"message":{"message_id":"m1"}}"#;

pub const CHANNEL_NONE_REPLY: &[u8] = br#"{"channel":null}"#;
pub const MESSAGES_EMPTY_REPLY: &[u8] = br#"{"messages":[]}"#;
pub const MESSAGE_NONE_REPLY: &[u8] = br#"{"message":null}"#;
pub const CHANNEL_REPLY: &[u8] = br#"{"channel":{"id":"general","name":"General","created_at":1,"head_seq":0,"post_policy":"open","hooks":[],"pinned":[],"huddle":[],"voice":false,"owner":{"account":7},"archived":false,"revision":1}}"#;
pub const MESSAGES_REPLY: &[u8] = br#"{"messages":[{"channel_id":"general","seq":1,"head":{"message_id":"m1","author":{"account":7},"origin":{"Program":3},"content_origin":{"Program":3},"blocks":[{"paragraph":[{"text":"hello","marks":[]}]}],"created_at":1,"rev":0,"revision":1,"edited_at":null,"base_rev":null,"deleted":false,"thread":null,"reply_count":0,"last_reply_seq":null}}]}"#;
pub const MESSAGE_POSTED_EVENT: &[u8] = br#"{"message_posted":{"channel_id":"general","seq":1,"thread_root":null,"author":{"account":7},"mentions":[3]}}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Block, Channel, ChatEvent, ChatMsg, ChatQuery, ChatReply, MessageHead, MessageView, Party,
        PostPolicy, Span,
    };

    #[test]
    fn owner_encodings_pin_consumer_edges() {
        assert_eq!(
            crate::encode_msg(&ChatMsg::CreateChannel {
                channel_id: "general".into(),
                name: "General".into(),
                post_policy: PostPolicy::Open,
            }),
            CREATE_CHANNEL
        );
        assert_eq!(
            crate::decode_msg(CREATE_CHANNEL).unwrap(),
            ChatMsg::CreateChannel {
                channel_id: "general".into(),
                name: "General".into(),
                post_policy: PostPolicy::Open,
            }
        );

        assert_eq!(
            crate::encode_msg(&ChatMsg::RegisterHook {
                channel_id: "general".into(),
                module_id: "automations".into(),
            }),
            REGISTER_HOOK
        );
        assert_eq!(
            crate::decode_msg(REGISTER_HOOK).unwrap(),
            ChatMsg::RegisterHook {
                channel_id: "general".into(),
                module_id: "automations".into(),
            }
        );

        let post = ChatMsg::PostMessage {
            channel_id: "general".into(),
            message_id: "m1".into(),
            blocks: vec![Block::Paragraph(vec![Span {
                text: "hello".into(),
                marks: Vec::new(),
            }])],
            thread: None,
        };
        assert_eq!(crate::encode_msg(&post), POST_MESSAGE);
        assert_eq!(crate::decode_msg(POST_MESSAGE).unwrap(), post);

        assert_eq!(
            crate::encode_query(&ChatQuery::Channel {
                channel_id: "general".into(),
            }),
            CHANNEL_QUERY
        );
        assert_eq!(
            crate::decode_query(CHANNEL_QUERY).unwrap(),
            ChatQuery::Channel {
                channel_id: "general".into(),
            }
        );
        assert_eq!(
            crate::encode_query(&ChatQuery::MessagesRange {
                channel_id: "general".into(),
                from_seq: 1,
                limit: 64,
            }),
            MESSAGES_RANGE_QUERY
        );
        assert_eq!(
            crate::decode_query(MESSAGES_RANGE_QUERY).unwrap(),
            ChatQuery::MessagesRange {
                channel_id: "general".into(),
                from_seq: 1,
                limit: 64,
            }
        );
        assert_eq!(
            crate::encode_query(&ChatQuery::Message {
                message_id: "m1".into(),
            }),
            MESSAGE_QUERY
        );
        assert_eq!(
            crate::decode_query(MESSAGE_QUERY).unwrap(),
            ChatQuery::Message {
                message_id: "m1".into(),
            }
        );

        assert_eq!(
            crate::encode_reply(&ChatReply::Channel(None)),
            CHANNEL_NONE_REPLY
        );
        assert_eq!(
            crate::decode_reply(CHANNEL_NONE_REPLY).unwrap(),
            ChatReply::Channel(None)
        );
        assert_eq!(
            crate::encode_reply(&ChatReply::Messages(Vec::new())),
            MESSAGES_EMPTY_REPLY
        );
        assert_eq!(
            crate::decode_reply(MESSAGES_EMPTY_REPLY).unwrap(),
            ChatReply::Messages(Vec::new())
        );
        assert_eq!(
            crate::encode_reply(&ChatReply::Message(None)),
            MESSAGE_NONE_REPLY
        );
        assert_eq!(
            crate::decode_reply(MESSAGE_NONE_REPLY).unwrap(),
            ChatReply::Message(None)
        );
        let channel = Channel {
            id: "general".into(),
            name: "General".into(),
            created_at: 1,
            head_seq: 0,
            post_policy: PostPolicy::Open,
            hooks: Vec::new(),
            pinned: Vec::new(),
            huddle: Vec::new(),
            voice: false,
            owner: Party::Account(7),
            archived: false,
            revision: 1,
        };
        assert_eq!(
            crate::encode_reply(&ChatReply::Channel(Some(channel.clone()))),
            CHANNEL_REPLY
        );
        assert_eq!(
            crate::decode_reply(CHANNEL_REPLY).unwrap(),
            ChatReply::Channel(Some(channel))
        );
        let messages = vec![MessageView {
            channel_id: "general".into(),
            seq: 1,
            head: MessageHead {
                message_id: "m1".into(),
                author: Party::Account(7),
                origin: sdk::Origin::Program(3),
                content_origin: sdk::Origin::Program(3),
                blocks: vec![Block::Paragraph(vec![Span {
                    text: "hello".into(),
                    marks: Vec::new(),
                }])],
                created_at: 1,
                rev: 0,
                revision: 1,
                edited_at: None,
                base_rev: None,
                deleted: false,
                thread: None,
                reply_count: 0,
                last_reply_seq: None,
            },
        }];
        assert_eq!(
            crate::encode_reply(&ChatReply::Messages(messages.clone())),
            MESSAGES_REPLY
        );
        assert_eq!(
            crate::decode_reply(MESSAGES_REPLY).unwrap(),
            ChatReply::Messages(messages)
        );
        assert_eq!(
            crate::encode_event(&ChatEvent::MessagePosted {
                channel_id: "general".into(),
                seq: 1,
                thread_root: None,
                author: Party::Account(7),
                mentions: vec![3],
            }),
            MESSAGE_POSTED_EVENT
        );
        assert_eq!(
            crate::decode_event(MESSAGE_POSTED_EVENT).unwrap(),
            ChatEvent::MessagePosted {
                channel_id: "general".into(),
                seq: 1,
                thread_root: None,
                author: Party::Account(7),
                mentions: vec![3],
            }
        );
    }
}
