//! the small, owned interfaces call consumes from sibling modules.
//!
//! these types intentionally link neither a sibling module nor a wire crate:
//! field order, variant names and envelope shape mirror the owner codecs,
//! pinned by the owner-encoding fixtures in `tests/golden.rs`. reply views
//! keep only the fields call reads.

use serde::{Deserialize, Serialize};

pub mod chat {
    use super::*;
    use crate::Party;
    use serde::de::IgnoredAny;

    /// the one chat read a join makes: `Access` — chat's post gate verbatim
    /// (archival and policy included), answered for the party call resolved.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatQuery {
        Access { channel_id: String, party: Party },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChannelAccess {
        pub may_read: bool,
        pub may_post: bool,
    }

    /// the reply arms call decodes; every other arm of chat's reply is
    /// present so the envelope stays `deny_unknown_fields`-exact, but its
    /// body is ignored.
    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatReply {
        Channel(IgnoredAny),
        Messages(IgnoredAny),
        Message(IgnoredAny),
        Access(ChannelAccess),
    }

    /// chat's follow-up events. call is addressed only by `ChannelArchived`
    /// (chat emits it to call when a channel closes); `MessagePosted` is the
    /// hook notification other subscribers receive, mirrored so the enum
    /// decodes every event chat encodes.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatEvent {
        MessagePosted {
            channel_id: String,
            seq: u64,
            thread_root: Option<u64>,
            author: Party,
            mentions: Vec<u64>,
        },
        ChannelArchived {
            channel_id: String,
        },
    }

    pub fn encode_query(query: &ChatQuery) -> Vec<u8> {
        sdk::wire::encode(query)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<ChatReply, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_event(event: &ChatEvent) -> Vec<u8> {
        sdk::wire::encode(event)
    }

    pub fn decode_event(bytes: &[u8]) -> Result<ChatEvent, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod identity {
    use super::*;
    use serde::de::IgnoredAny;

    /// the one identity read call makes: the account holding a key.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        OfKey { key: Vec<u8> },
    }

    /// an account view reduced to its number; every other owner field is
    /// named so the shape stays exact, and ignored.
    #[derive(Deserialize, Debug)]
    #[serde(deny_unknown_fields)]
    pub struct AccountView {
        pub number: u64,
        #[serde(rename = "name")]
        _name: IgnoredAny,
        #[serde(rename = "control")]
        _control: IgnoredAny,
        #[serde(rename = "keys")]
        _keys: IgnoredAny,
        #[serde(rename = "avatar")]
        _avatar: IgnoredAny,
        #[serde(rename = "bio")]
        _bio: IgnoredAny,
        #[serde(rename = "updated_at")]
        _updated_at: IgnoredAny,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityReply {
        Accounts(IgnoredAny),
        Account(Option<AccountView>),
        Resolved(IgnoredAny),
        Gen(IgnoredAny),
    }

    pub fn encode_query(query: &IdentityQuery) -> Vec<u8> {
        sdk::wire::encode(query)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }
}
