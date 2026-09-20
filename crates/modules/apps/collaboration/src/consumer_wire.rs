//! The small, owned interfaces collaboration consumes from sibling modules.
//!
//! These types intentionally do not link a sibling module or its wire crate.
//! Their field order, variant names, and envelope shape mirror the locked
//! owner codecs; reply views keep only fields collaboration reads.

use sdk::AccountNumber;

use crate::Party;

pub mod chat {
    use super::Party;
    use serde::de::IgnoredAny;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatMsg {
        SetMembership {
            channel_id: String,
            party: Party,
            member: bool,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
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
        Access {
            channel_id: String,
            party: Party,
        },
    }

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChannelAccess {
        pub may_read: bool,
        pub may_post: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MessageHead {
        pub origin: sdk::Origin,
        pub deleted: bool,
        pub thread: Option<u64>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MessageView {
        pub channel_id: String,
        pub seq: u64,
        pub head: MessageHead,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ChatReply {
        Access(ChannelAccess),
        Message(Option<MessageView>),
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WireMessageHead {
        #[serde(rename = "message_id")]
        _message_id: IgnoredAny,
        #[serde(rename = "author")]
        _author: IgnoredAny,
        origin: sdk::Origin,
        #[serde(rename = "content_origin")]
        _content_origin: IgnoredAny,
        #[serde(rename = "blocks")]
        _blocks: IgnoredAny,
        #[serde(rename = "created_at")]
        _created_at: IgnoredAny,
        #[serde(rename = "rev")]
        _rev: IgnoredAny,
        #[serde(rename = "revision")]
        _revision: IgnoredAny,
        #[serde(rename = "edited_at")]
        _edited_at: IgnoredAny,
        #[serde(rename = "base_rev")]
        _base_rev: IgnoredAny,
        deleted: bool,
        thread: Option<u64>,
        #[serde(rename = "reply_count")]
        _reply_count: IgnoredAny,
        #[serde(rename = "last_reply_seq")]
        _last_reply_seq: IgnoredAny,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WireMessageView {
        channel_id: String,
        seq: u64,
        head: WireMessageHead,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WireChatReply {
        Channel(IgnoredAny),
        Messages(IgnoredAny),
        Access(ChannelAccess),
        Message(Option<WireMessageView>),
    }

    fn message(value: WireMessageView) -> MessageView {
        MessageView {
            channel_id: value.channel_id,
            seq: value.seq,
            head: MessageHead {
                origin: value.head.origin,
                deleted: value.head.deleted,
                thread: value.head.thread,
            },
        }
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
        match sdk::wire::decode(bytes)? {
            WireChatReply::Access(value) => Ok(ChatReply::Access(value)),
            WireChatReply::Message(value) => Ok(ChatReply::Message(value.map(message))),
            WireChatReply::Channel(_) | WireChatReply::Messages(_) => {
                Err("expected a chat access or message reply".into())
            }
        }
    }
}

pub mod identity {
    use super::AccountNumber;
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    /// Only the control variants and program standing used by collaboration
    /// are decoded. The owner’s extra program fields are ignored.
    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum Control {
        Keys,
        Program { standing: ProgramStanding },
        Revoked { controller: AccountNumber },
    }

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct AccountView {
        pub number: AccountNumber,
        pub control: Control,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        Get { number: AccountNumber },
        OfKey { key: Vec<u8> },
    }

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
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

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod tasks {
    use serde::de::IgnoredAny;
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct Job {
        pub attempt: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsQuery {
        Get { job_id: String },
    }

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsReply {
        Job(Option<Job>),
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WireJobsReply {
        Job(Option<Job>),
        Worker(IgnoredAny),
        Controls(IgnoredAny),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WorkQuery {
        Job(JobsQuery),
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WorkReply {
        Job(WireJobsReply),
    }

    pub fn encode_job_query(value: &JobsQuery) -> Vec<u8> {
        sdk::wire::encode(&WorkQuery::Job(value.clone()))
    }

    pub fn decode_job_reply(bytes: &[u8]) -> Result<JobsReply, String> {
        match sdk::wire::decode(bytes)? {
            WorkReply::Job(WireJobsReply::Job(value)) => Ok(JobsReply::Job(value)),
            WorkReply::Job(WireJobsReply::Worker(_))
            | WorkReply::Job(WireJobsReply::Controls(_)) => {
                Err("expected a tasks job reply".into())
            }
        }
    }
}
