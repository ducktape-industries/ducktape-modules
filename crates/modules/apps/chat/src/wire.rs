//! the chat module's public wire surface -- types only.
//!
//! writes go via [`ChatMsg`]; reads via [`ChatQuery`] -> [`ChatReply`]; hook
//! subscribers receive [`ChatEvent`] payloads. authorship is never part of a
//! write payload — the module derives the acting [`Party`] from the dispatch
//! origin — so a write names a party only where it addresses one (a mention,
//! a membership), and replies and events carry the party the module
//! resolved. who is in a channel's CALL is not here: the `call` module owns
//! that roster and asks chat only whether a party may post.

use sdk::AccountNumber;
use serde::{Deserialize, Serialize};

// the derived-tier materialized view: the PURE decision core (fold + view
// over index_guest::StateRead). every reader of the feed — the app, the MCP
// host, a view — links it here without the module.

// the CLIENT view model: rendered row types, composer parsing, optimistic
// merges, and the op-delta fold a feed-following UI splices state with.
// pure data-in/data-out, beside the index fold (same feed, same vocabulary).

// chat's half of a `duck://` address.
pub use duck_address::chat::MessageAddress;

pub const DEFAULT_CHAT_TARGET: &str = "chat";

/// the attribution object kinds chat reports under (`ObjectRef::kind`): a
/// channel is reported by its id, a message by its client-minted message id.
pub const OBJECT_KIND_CHANNEL: &str = "channel";
pub const OBJECT_KIND_MESSAGE: &str = "message";

// ---- write-time caps (consensus constants) ---------------------------------
// enforced by the module BEFORE staging: the qmdb codec's 1 MiB cap is
// decode-only, so an oversized committed value would panic every validator on
// the next read. shared here so clients can pre-validate.

/// serialized [`MessageHead`] bound, per message record.
pub const MAX_MESSAGE_HEAD_BYTES: usize = 64 * 1024;
/// serialized [`Channel`] record bound (also applied to the membership index).
pub const MAX_CHANNEL_RECORD_BYTES: usize = 256 * 1024;
/// revisions per message; further edits are rejected.
pub const MAX_REVISIONS: u32 = 256;
/// emoji byte length bound.
pub const MAX_EMOJI_BYTES: usize = 64;
/// distinct emojis per message.
pub const MAX_REACTION_EMOJIS: usize = 64;
/// hook modules per channel.
pub const MAX_HOOKS_PER_CHANNEL: usize = 8;
/// replies per thread.
pub const MAX_THREAD_REPLIES: usize = 4096;
/// query page bound; larger limits are clamped down to this.
pub const MAX_QUERY_LIMIT: u64 = 256;
/// channels one creator (an account or key party) may have open at once.
/// there is no `DeleteChannel` op — every created channel is permanent — so
/// this is the only thing bounding one party's share of the channel set.
/// module/system origins are exempt (genesis-fixed trusted code). picked in
/// the same spirit as forge's `MAX_OPEN_ITEMS_PER_ACTOR` / tasks'
/// `MAX_OPEN_TASKS_PER_OWNER`.
pub const MAX_CHANNELS_PER_CREATOR: usize = 256;

pub use crate::message::{Block, Mark, Party, Span, resolve_assigned_mentions};

/// who may post (and react) in a channel.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PostPolicy {
    /// any authenticated party.
    Open,
    /// people (account and key parties) must be channel members; module and
    /// system parties always may.
    MembersOnly,
}

/// the per-channel record: metadata plus the head sequence counter that
/// assigns every message's position (P3 — gap-free, in-state, at execute time).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    /// the last assigned message sequence; 0 = no messages yet.
    pub head_seq: u64,
    pub post_policy: PostPolicy,
    /// module ids notified (one follow-up msg each) on every successful post.
    pub hooks: Vec<String>,
    /// pinned message sequences (no pin op yet; carried for the record shape).
    pub pinned: Vec<u64>,
    /// a voice room: opened by `CreateVoiceChannel`, entered by joining its
    /// call (the `call` module's roster) rather than by reading it.
    pub voice: bool,
    /// the party that created the channel. a person owner is the only person
    /// who may administer it (rename, archive, roster, hooks); a module or
    /// system owner admits no person at all, and module and system parties
    /// administer every channel.
    pub owner: Party,
    /// archived channels reject posts and reactions (and, through `Access`,
    /// call joins); membership, rename, and unarchive stay allowed.
    pub archived: bool,
    /// the channel's attribution revision: 1 at creation, +1 for every rename
    /// and archive toggle — the strictly increasing counter every attribution
    /// report of this channel carries. roster ops (membership, hooks) do not
    /// revise the channel.
    pub revision: u64,
}

/// the mutable head of one message. prior contents live in immutable revision
/// records; a delete tombstones the head but keeps the skeleton so thread
/// linkage and the per-channel sequence promise survive.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageHead {
    pub message_id: String,
    pub author: Party,
    /// Authenticated origin of the original post. Account resolution never
    /// erases the exact signing key needed by consumers with key-owned rights.
    pub origin: sdk::Origin,
    /// Actual authenticated writer of the current body, updated on edits.
    /// A consumer executing the content must use this proof for key-only rights.
    pub content_origin: sdk::Origin,
    pub blocks: Vec<Block>,
    pub created_at: u64,
    /// edit revision; 0 = original post. indexes the immutable content
    /// history (`rev` records hold the replaced heads).
    pub rev: u32,
    /// the message's attribution revision: 1 at post, +1 per edit, +1 at
    /// delete — the strictly increasing counter every attribution report of
    /// this message carries. distinct from `rev`, which counts edits only.
    pub revision: u64,
    pub edited_at: Option<u64>,
    /// the revision the last edit CLAIMED to be based on. a stale base is
    /// recorded (base_rev != rev - 1), never rejected — head is last-write-wins
    /// under the consensus total order.
    pub base_rev: Option<u32>,
    pub deleted: bool,
    /// `Some(root_seq)` marks this message as a thread reply.
    pub thread: Option<u64>,
    pub reply_count: u64,
    pub last_reply_seq: Option<u64>,
}

/// a query-side message view: one sequence's head, addressed. reaction
/// summaries and head-sequence watermarks are read-model decoration and
/// live on the index tier — dispatch consumers read heads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageView {
    pub channel_id: String,
    pub seq: u64,
    pub head: MessageHead,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatMsg {
    /// `channel_id`s containing `:` are a reserved module namespace: a person
    /// (account or key party) may not create one, and a module origin `m`
    /// may only create ids prefixed `"{m}:"` (forge's per-issue/PR discussion
    /// channels are `forge:<repo>:<n>`). system origin is unrestricted.
    CreateChannel {
        channel_id: String,
        name: String,
        post_policy: PostPolicy,
    },
    /// open a voice room: a channel whose point is its call, listed under
    /// its own heading and entered by joining. it posts `PostPolicy::Open`
    /// like any channel; `Channel::voice` is what sets it apart. the same
    /// id and namespace rules as `CreateChannel`.
    CreateVoiceChannel { channel_id: String, name: String },
    /// open the two-party room with `counterpart`. the module derives the id
    /// itself — `client::dm_channel_id(creator account, counterpart)`, the
    /// creator being the ACCOUNT the origin resolved to — so the id can never
    /// be spoofed to any other pair's DM, and a key holding no account cannot
    /// open one. always seats `PostPolicy::MembersOnly`, whatever
    /// `CreateChannel` might otherwise allow. `dm-`-shaped ids are reserved:
    /// plain `CreateChannel` refuses one from a person (see
    /// `Chat::stage_channel`).
    CreateDmChannel { counterpart: u64, name: String },
    /// rename a channel, reusing `CreateChannel`'s name validation (non-empty +
    /// the reserved `:` namespace gate + the record byte cap). any
    /// authenticated party renames any channel: `owner` is attribution, not
    /// a gate; the `:` namespace gate is what keeps a person off a
    /// module-namespaced channel.
    RenameChannel { channel_id: String, name: String },
    /// archive or unarchive a channel. an archived channel rejects posts and
    /// reactions; membership, rename, and unarchive stay allowed. archiving
    /// also ends the channel's call: chat emits [`ChatEvent::ChannelArchived`]
    /// to the `call` module when one is registered. authorization mirrors
    /// `RenameChannel`.
    SetChannelArchived { channel_id: String, archived: bool },
    /// post a message; `thread` = `Some(root_seq)` posts a thread reply, which
    /// is a normal message record consuming its own channel sequence. the
    /// author is the origin's party; every `Mark::Mention` must name an
    /// account (see [`Mark::Mention`]).
    PostMessage {
        channel_id: String,
        message_id: String,
        blocks: Vec<Block>,
        thread: Option<u64>,
    },
    /// replace the head blocks; the prior head is appended to the immutable
    /// revision history. any authenticated party may edit any message; the
    /// mentions of the new blocks are validated like a post's.
    EditMessage {
        channel_id: String,
        seq: u64,
        blocks: Vec<Block>,
        base_rev: Option<u32>,
    },
    /// tombstone: content and reactions cleared, skeleton kept. any
    /// authenticated party may delete any message.
    DeleteMessage { channel_id: String, seq: u64 },
    /// idempotent per (emoji, party).
    AddReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    /// exact remove of this party's reaction; absent = deterministic no-op.
    RemoveReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    /// subscribe a module to this channel's post notifications: a hook sees
    /// everything posted to the channel. any authenticated party attaches
    /// one, as with `RenameChannel`; the module must be registered.
    RegisterHook {
        channel_id: String,
        module_id: String,
    },
    /// detach a hook module, which disables that automation on the channel.
    /// any authenticated party detaches one; an absent hook is a no-op.
    UnregisterHook {
        channel_id: String,
        module_id: String,
    },
    /// add/remove a person from the channel member set. this roster is
    /// `PostPolicy::MembersOnly`'s admission list, and any authenticated
    /// party writes it, themself included: the roster records who posts, it
    /// is not a gate a channel's owner holds.
    /// `party` names a person in the resolved vocabulary: an account that
    /// exists, or a key that holds no account (a key that does hold one is
    /// refused — name the account). modules and the system are never members;
    /// they always may post.
    SetMembership {
        channel_id: String,
        party: Party,
        member: bool,
    },
}

/// the DISPATCH read surface — exactly the point/computed reads other
/// modules' `execute()` paths consume through `Ctx::query` (runs' context
/// pinning and existence probes, automations' event handling). every
/// UI-shaped read (channel lists, latest pages, threads, revisions,
/// reactions, members, search) is served by chat's index guest on the
/// derived tier instead — consensus never reads the unverifiable index,
/// and canonical state never grows scan machinery for a human surface.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatQuery {
    /// one channel record by id — the existence/policy probe.
    Channel { channel_id: String },
    /// `limit` messages starting at `from_seq`, ascending — the agent
    /// context window. computed-key point reads driven by the gap-free
    /// sequence space (P3), deterministic on every validator.
    MessagesRange {
        channel_id: String,
        from_seq: u64,
        limit: u64,
    },
    /// global message-id lookup — the id-collision probe.
    Message { message_id: String },
    /// what ONE party may do in ONE channel — the standing a module acting on
    /// that party's behalf must gate on. chat owns the answer so a caller
    /// never carries a second copy of the admission rule.
    Access { channel_id: String, party: Party },
}

/// chat's answer to [`ChatQuery::Access`]: one party's standing in one channel.
/// an unknown channel answers `false` to both — a caller fails closed on a
/// channel that does not exist.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChannelAccess {
    /// the party may see the channel's messages: a member, or any
    /// authenticated party when the channel is [`PostPolicy::Open`]. archival
    /// does not close reading.
    pub may_read: bool,
    /// the party's own `PostMessage` would be admitted — chat's post gate
    /// verbatim, so an archived or members-only channel answers `false`.
    pub may_post: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatReply {
    Channel(Option<Channel>),
    Messages(Vec<MessageView>),
    Message(Option<MessageView>),
    Access(ChannelAccess),
}

/// chat's follow-up payloads: one [`sdk::Msg`]-shaped dispatch per
/// registered hook module for a post, emitted in the same block (P2), and
/// the archive notice the `call` module clears a roster on.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatEvent {
    MessagePosted {
        channel_id: String,
        seq: u64,
        thread_root: Option<u64>,
        author: Party,
        /// the accounts the post mentions, resolved and deduplicated, in
        /// first-occurrence order.
        mentions: Vec<AccountNumber>,
    },
    /// the channel closed: addressed to the `call` module (never to hooks),
    /// which ends the channel's call in the same unit.
    ChannelArchived { channel_id: String },
}

/// the assigned stamp chat declares per applied op ([`sdk::Ctx::set_assigned`]):
/// the values the module assigned in-state that the op payload cannot carry.
/// rides the dispatch trace onto the derived-tier op-feed row, so feed
/// followers (the index fold, clients) consume exact assignments instead of
/// re-deriving them by counting.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatAssigned {
    /// `PostMessage`: assigned sequence and resolution of the payload's keys.
    Posted {
        seq: u64,
        actor: Party,
        /// Accounts for distinct raw-key mentions, in first-occurrence order.
        /// The payload retains the body and already-canonical account mentions.
        key_mentions: Vec<AccountNumber>,
    },
    /// `EditMessage`: assigned revision and resolution of the payload's keys.
    Edited {
        rev: u32,
        actor: Party,
        key_mentions: Vec<AccountNumber>,
    },
    /// `CreateDmChannel`: the derived id the module minted from (creator,
    /// counterpart) — the payload never carries it.
    DmChannel { channel_id: String, actor: Party },
    /// Canonical actor for an operation without an additional assignment.
    Actor { actor: Party },
    /// Exact existing/new party whose reaction was affected.
    Participant { actor: Party, participant: Party },
}

impl ChatAssigned {
    pub fn participant(&self) -> Result<&Party, String> {
        let Self::Participant { participant, .. } = self else {
            return Err("participant operation carried a non-Participant stamp".into());
        };
        Ok(participant)
    }

    pub fn actor(&self) -> &Party {
        match self {
            Self::Posted { actor, .. }
            | Self::Edited { actor, .. }
            | Self::DmChannel { actor, .. }
            | Self::Actor { actor }
            | Self::Participant { actor, .. } => actor,
        }
    }
}

pub fn encode_msg(m: &ChatMsg) -> Vec<u8> {
    sdk::wire::encode(m)
}

pub fn decode_msg(b: &[u8]) -> Result<ChatMsg, String> {
    sdk::wire::decode(b)
}

pub fn encode_query(q: &ChatQuery) -> Vec<u8> {
    sdk::wire::encode(q)
}

pub fn decode_query(b: &[u8]) -> Result<ChatQuery, String> {
    sdk::wire::decode(b)
}

pub fn encode_reply(r: &ChatReply) -> Vec<u8> {
    sdk::wire::encode(r)
}

pub fn decode_reply(b: &[u8]) -> Result<ChatReply, String> {
    sdk::wire::decode(b)
}

pub fn encode_event(e: &ChatEvent) -> Vec<u8> {
    sdk::wire::encode(e)
}

pub fn decode_event(b: &[u8]) -> Result<ChatEvent, String> {
    sdk::wire::decode(b)
}

pub fn encode_assigned(a: &ChatAssigned) -> Vec<u8> {
    sdk::wire::encode(a)
}

pub fn decode_assigned(b: &[u8]) -> Result<ChatAssigned, String> {
    sdk::wire::decode(b)
}

#[cfg(test)]
mod interface_tests {
    use super::*;

    #[test]
    fn a_party_round_trips_both_codecs() {
        for party in [
            Party::Account(7),
            Party::Key(vec![0xab; 32]),
            Party::Module("forge".into()),
            Party::System,
        ] {
            let wire: Party = sdk::wire::decode(&sdk::wire::encode(&party)).unwrap();
            assert_eq!(wire, party);
            let bytes = borsh::to_vec(&party).unwrap();
            assert_eq!(borsh::from_slice::<Party>(&bytes).unwrap(), party);
        }
        assert_eq!(Party::Account(7).account(), Some(7));
        assert_eq!(Party::Key(vec![1]).account(), None);
        assert!(Party::Key(vec![1]).is_person());
        assert!(!Party::Module("m".into()).is_person());
    }

    #[test]
    fn a_post_carries_no_author_field() {
        // the exact wire a member's post is: no author, no agent refinement.
        let wire = br#"{"post_message":{"channel_id":"g","message_id":"m1","blocks":[{"paragraph":[{"text":"hi","marks":[]}]}],"thread":null}}"#;
        let ChatMsg::PostMessage { channel_id, .. } = decode_msg(wire).unwrap() else {
            panic!("expected PostMessage")
        };
        assert_eq!(channel_id, "g");
        assert!(decode_msg(br#"{"post_message":{"channel_id":"g","message_id":"m1","blocks":[],"thread":null,"as_agent":"bot"}}"#).is_err());
    }
}
