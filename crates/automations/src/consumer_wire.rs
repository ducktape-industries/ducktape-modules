//! The small, owned interfaces Automations consumes from sibling modules.
//!
//! These types intentionally do not link a sibling module or its wire crate.
//! Their field order, variant names, and envelope shape mirror the BASE wire
//! codecs; the fixtures below pin the bytes produced by the owning codec.

use sdk::AccountNumber;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Party {
    Account(AccountNumber),
    Key(Vec<u8>),
    Module(String),
    System,
}

pub mod chat {
    use super::Party;
    use sdk::AccountNumber;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub enum Mark {
        Bold,
        Italic,
        Link(String),
        Mention(Party),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Span {
        pub text: String,
        pub marks: Vec<Mark>,
    }

    impl Span {
        pub fn plain(text: impl Into<String>) -> Self {
            Self {
                text: text.into(),
                marks: Vec::new(),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Block {
        Paragraph(Vec<Span>),
        Code { lang: Option<String>, text: String },
        Quote(Vec<Span>),
        Divider,
    }

    impl Block {
        pub fn paragraph(text: impl Into<String>) -> Self {
            Self::Paragraph(vec![Span {
                text: text.into(),
                marks: Vec::new(),
            }])
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct MessageHead {
        pub message_id: String,
        pub author: Party,
        pub blocks: Vec<Block>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct MessageView {
        pub seq: u64,
        pub head: MessageHead,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ChatMsg {
        PostMessage {
            channel_id: String,
            message_id: String,
            blocks: Vec<Block>,
            thread: Option<u64>,
        },
        CreateChannel {
            channel_id: String,
            name: String,
            post_policy: PostPolicy,
        },
        RegisterHook {
            channel_id: String,
            module_id: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum PostPolicy {
        Open,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ChatQuery {
        Channel {
            channel_id: String,
        },
        MessagesRange {
            channel_id: String,
            from_seq: u64,
            limit: u64,
        },
        Message {
            message_id: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
    pub struct Channel {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ChatReply {
        Channel(Option<Channel>),
        Messages(Vec<MessageView>),
        Message(Option<MessageView>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ChatEvent {
        MessagePosted {
            channel_id: String,
            seq: u64,
            thread_root: Option<u64>,
            author: Party,
            mentions: Vec<AccountNumber>,
        },
    }

    pub fn encode_msg(value: &ChatMsg) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_msg(bytes: &[u8]) -> Result<ChatMsg, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_query(value: &ChatQuery) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_query(bytes: &[u8]) -> Result<ChatQuery, String> {
        sdk::wire::decode(bytes)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<ChatReply, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_reply(value: &ChatReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn encode_event(value: &ChatEvent) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_event(bytes: &[u8]) -> Result<ChatEvent, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod tasks {
    use super::Party;
    use serde::{Deserialize, Serialize};

    pub const MAX_LIST_LIMIT: u64 = 256;
    pub const MAX_OPEN_TASKS_PER_OWNER: usize = 128;
    pub const MAX_TASK_ID: usize = 256;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct Task {
        pub id: String,
        pub title: String,
        pub owner: Party,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum TaskMsg {
        CreateTask {
            task_id: String,
            title: String,
            owner: Option<u64>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum TaskQuery {
        Get { task_id: String },
        List { limit: u64, after: Option<String> },
        OwnerOpenCount { owner: Party },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum TaskReply {
        Task(Option<Task>),
        Tasks(Vec<Task>),
        OwnerOpenCount(u64),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    enum WorkMsg {
        Task(TaskMsg),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    enum WorkQuery {
        Task(TaskQuery),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    enum WorkReply {
        Task(TaskReply),
    }

    pub fn encode_task_msg(value: &TaskMsg) -> Vec<u8> {
        sdk::wire::encode(&WorkMsg::Task(value.clone()))
    }

    pub fn decode_task_msg(bytes: &[u8]) -> Result<TaskMsg, String> {
        match sdk::wire::decode(bytes)? {
            WorkMsg::Task(value) => Ok(value),
        }
    }

    pub fn encode_task_query(value: &TaskQuery) -> Vec<u8> {
        sdk::wire::encode(&WorkQuery::Task(value.clone()))
    }

    pub fn decode_task_query(bytes: &[u8]) -> Result<TaskQuery, String> {
        match sdk::wire::decode(bytes)? {
            WorkQuery::Task(value) => Ok(value),
        }
    }

    pub fn encode_task_reply(value: &TaskReply) -> Vec<u8> {
        sdk::wire::encode(&WorkReply::Task(value.clone()))
    }

    pub fn decode_task_reply(bytes: &[u8]) -> Result<TaskReply, String> {
        match sdk::wire::decode(bytes)? {
            WorkReply::Task(value) => Ok(value),
        }
    }
}

pub mod attribution {
    use sdk::AccountNumber;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ObjectRef {
        pub kind: String,
        pub object: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Actor {
        Account(AccountNumber),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Reason {
        Mention,
        Authorship,
        Ownership,
        Assignment,
        Credit,
        Result,
        Report,
        Defined(String),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Relation {
        pub recipient: AccountNumber,
        pub reason: Reason,
        pub detail: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Transfer {
        pub reason: Reason,
        pub from: AccountNumber,
        pub to: AccountNumber,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AttributionUpdate {
        pub object: ObjectRef,
        pub revision: u64,
        pub actor: Actor,
        pub relations: Vec<Relation>,
        pub transfers: Vec<Transfer>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionMsg {
        Attribute {
            object: ObjectRef,
            revision: u64,
            actor: Actor,
            relations: Vec<Relation>,
            transfers: Vec<Transfer>,
        },
        AttributeBatch {
            updates: Vec<AttributionUpdate>,
        },
        Subscribe {},
    }

    pub fn encode_msg(value: &AttributionMsg) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_msg(bytes: &[u8]) -> Result<AttributionMsg, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod identity {
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum KeyScheme {
        Ed25519,
        Secp256k1,
        Secp256r1,
    }

    #[cfg(test)]
    #[test]
    fn key_scheme_matches_producer_tags() {
        for (scheme, tag) in [
            (KeyScheme::Ed25519, "ed25519"),
            (KeyScheme::Secp256k1, "secp256k1"),
            (KeyScheme::Secp256r1, "secp256r1"),
        ] {
            let bytes = format!("\"{tag}\"").into_bytes();
            assert_eq!(sdk::wire::encode(&scheme), bytes);
            assert_eq!(sdk::wire::decode::<KeyScheme>(&bytes).unwrap(), scheme);
        }
    }
    use sdk::{AccountNumber, ModuleId};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Control {
        Keys,
        Program {
            controller: AccountNumber,
            executor: ModuleId,
            generation: u64,
            standing: ProgramStanding,
        },
        Revoked {
            controller: AccountNumber,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct KeyView {
        pub scheme: KeyScheme,
        pub pubkey: Vec<u8>,
        pub label: Option<String>,
        pub added_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AccountView {
        pub number: AccountNumber,
        pub name: String,
        pub control: Control,
        pub keys: Vec<KeyView>,
        pub avatar: Option<String>,
        pub bio: Option<String>,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        All {
            from: u64,
            limit: u64,
        },
        Get {
            number: AccountNumber,
        },
        OfKey {
            key: Vec<u8>,
        },
        Resolve {
            references: Vec<AccountRef>,
        },
        KeyGen {
            key: Vec<u8>,
        },
        Controlled {
            by: AccountNumber,
            from: u64,
            limit: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AccountRef {
        Account(AccountNumber),
        Key(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityReply {
        Accounts(Vec<AccountView>),
        Account(Option<AccountView>),
        Resolved(Vec<Option<AccountNumber>>),
        Gen(u64),
    }

    pub fn encode_query(value: &IdentityQuery) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_query(bytes: &[u8]) -> Result<IdentityQuery, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_reply(value: &IdentityReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }
}

#[cfg(test)]
mod fixtures {
    use super::{Party, chat, tasks};

    // BASE owner codec provenance: ducktape-sdk 8c764093, chat-wire/tasks-wire
    // wrappers over sdk::wire::encode/decode. These are immutable owner bytes,
    // not values produced by the consumer decoder.
    const CHAT_EVENT: &[u8] = br#"{"message_posted":{"channel_id":"general","seq":5,"thread_root":null,"author":{"account":7},"mentions":[3]}}"#;
    const CHAT_POST: &[u8] = br#"{"post_message":{"channel_id":"general","message_id":"auto-r1-general-5","blocks":[{"paragraph":[{"text":"hello","marks":[]}]}],"thread":null}}"#;
    const CHAT_CHANNEL_QUERY: &[u8] = br#"{"channel":{"channel_id":"general"}}"#;
    const CHAT_MESSAGE_QUERY: &[u8] = br#"{"message":{"message_id":"m1"}}"#;
    const CHAT_RANGE_QUERY: &[u8] =
        br#"{"messages_range":{"channel_id":"general","from_seq":5,"limit":1}}"#;
    const CHAT_REPLY: &[u8] = br#"{"messages":[{"channel_id":"general","seq":5,"head":{"message_id":"m1","author":{"account":7},"origin":{"External":[1,2]},"content_origin":{"External":[1,2]},"blocks":[{"paragraph":[{"text":"hello","marks":[]}]}],"created_at":100,"rev":0,"revision":1,"edited_at":null,"base_rev":null,"deleted":false,"thread":null,"reply_count":0,"last_reply_seq":null}}]}"#;
    const TASK_MSG: &[u8] =
        br#"{"task":{"create_task":{"task_id":"t1","title":"hello","owner":7}}}"#;
    const TASK_GET: &[u8] = br#"{"task":{"get":{"task_id":"t1"}}}"#;
    const TASK_OWNER_COUNT: &[u8] = br#"{"task":{"owner_open_count":{"owner":{"account":7}}}}"#;
    const TASK_REPLY: &[u8] = br#"{"task":{"task":{"id":"t1","title":"hello","status":"open","owner":{"account":7},"created_at":100,"updated_at":100}}}"#;
    const TASK_COUNT_REPLY: &[u8] = br#"{"task":{"owner_open_count":3}}"#;

    #[test]
    fn chat_event_decodes_owner_bytes() {
        assert_eq!(
            chat::decode_event(CHAT_EVENT).unwrap(),
            chat::ChatEvent::MessagePosted {
                channel_id: "general".into(),
                seq: 5,
                thread_root: None,
                author: Party::Account(7),
                mentions: vec![3],
            }
        );
    }

    #[test]
    fn chat_edges_encode_owner_bytes() {
        assert_eq!(
            chat::encode_msg(&chat::ChatMsg::PostMessage {
                channel_id: "general".into(),
                message_id: "auto-r1-general-5".into(),
                blocks: vec![chat::Block::paragraph("hello")],
                thread: None,
            }),
            CHAT_POST
        );
        assert_eq!(
            chat::encode_query(&chat::ChatQuery::Channel {
                channel_id: "general".into()
            }),
            CHAT_CHANNEL_QUERY
        );
        assert_eq!(
            chat::encode_query(&chat::ChatQuery::Message {
                message_id: "m1".into()
            }),
            CHAT_MESSAGE_QUERY
        );
        assert_eq!(
            chat::encode_query(&chat::ChatQuery::MessagesRange {
                channel_id: "general".into(),
                from_seq: 5,
                limit: 1,
            }),
            CHAT_RANGE_QUERY
        );
    }

    #[test]
    fn chat_reply_decodes_owner_bytes() {
        let chat::ChatReply::Messages(views) = chat::decode_reply(CHAT_REPLY).unwrap() else {
            panic!("owner messages reply changed shape")
        };
        assert_eq!(views[0].seq, 5);
        assert_eq!(views[0].head.message_id, "m1");
        assert_eq!(views[0].head.blocks, vec![chat::Block::paragraph("hello")]);
    }

    #[test]
    fn task_edges_match_owner_envelopes() {
        assert_eq!(
            tasks::encode_task_msg(&tasks::TaskMsg::CreateTask {
                task_id: "t1".into(),
                title: "hello".into(),
                owner: Some(7),
            }),
            TASK_MSG
        );
        assert_eq!(
            tasks::encode_task_query(&tasks::TaskQuery::Get {
                task_id: "t1".into()
            }),
            TASK_GET
        );
        assert_eq!(
            tasks::encode_task_query(&tasks::TaskQuery::OwnerOpenCount {
                owner: Party::Account(7)
            }),
            TASK_OWNER_COUNT
        );
        let tasks::TaskReply::Task(Some(task)) = tasks::decode_task_reply(TASK_REPLY).unwrap()
        else {
            panic!("owner task reply changed shape")
        };
        assert_eq!(task.id, "t1");
        assert_eq!(task.title, "hello");
        assert_eq!(
            tasks::decode_task_reply(TASK_COUNT_REPLY).unwrap(),
            tasks::TaskReply::OwnerOpenCount(3)
        );
    }
}
