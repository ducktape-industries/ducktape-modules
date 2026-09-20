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
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChannelAccess {
        pub may_read: bool,
        pub may_post: bool,
    }

    /// Content is owned by chat and never inspected here. It stays opaque so
    /// replies preserve the owner’s full message-head shape without importing
    /// chat’s body model.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Opaque;

    impl Serialize for Opaque {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_unit()
        }
    }

    impl<'de> Deserialize<'de> for Opaque {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            IgnoredAny::deserialize(deserializer).map(|_| Self)
        }
    }

    /// The owner’s full message head is retained at the boundary because the
    /// existing guest artifact and owner codec carry all of these fields.
    /// Collaboration reads only `origin`, `deleted`, and `thread`.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct MessageHead {
        pub message_id: String,
        pub author: Party,
        pub origin: sdk::Origin,
        pub content_origin: sdk::Origin,
        pub blocks: Vec<Opaque>,
        pub created_at: u64,
        pub rev: u32,
        pub revision: u64,
        pub edited_at: Option<u64>,
        pub base_rev: Option<u32>,
        pub deleted: bool,
        pub thread: Option<u64>,
        pub reply_count: u64,
        pub last_reply_seq: Option<u64>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct MessageView {
        pub channel_id: String,
        pub seq: u64,
        pub head: MessageHead,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    #[expect(
        clippy::large_enum_variant,
        reason = "The local reply keeps the owner wire shape without boxing"
    )]
    pub enum ChatReply {
        Access(ChannelAccess),
        Message(Option<MessageView>),
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    #[expect(
        clippy::large_enum_variant,
        reason = "The decode boundary keeps the owner wire shape without boxing"
    )]
    enum WireChatReply {
        Channel(IgnoredAny),
        Messages(IgnoredAny),
        Access(ChannelAccess),
        Message(Option<MessageView>),
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

    pub fn encode_reply(value: &ChatReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<ChatReply, String> {
        match sdk::wire::decode(bytes)? {
            WireChatReply::Access(value) => Ok(ChatReply::Access(value)),
            WireChatReply::Message(value) => Ok(ChatReply::Message(value)),
            WireChatReply::Channel(_) | WireChatReply::Messages(_) => {
                Err("expected a chat access or message reply".into())
            }
        }
    }
}

pub mod identity {
    use super::AccountNumber;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    /// Only the control variants and program standing used by collaboration
    /// are decoded. The owner’s extra program fields are ignored.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum Control {
        Keys,
        Program { standing: ProgramStanding },
        Revoked { controller: AccountNumber },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_reply(value: &IdentityReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }
}

pub mod tasks {
    use serde::de::IgnoredAny;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct Job {
        pub attempt: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsQuery {
        Get { job_id: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

    pub fn encode_job_reply(value: &JobsReply) -> Vec<u8> {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum EncodedWorkReply<'a> {
            Job(&'a JobsReply),
        }
        sdk::wire::encode(&EncodedWorkReply::Job(value))
    }
}
