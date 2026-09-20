//! the call module's public wire surface — types only.
//!
//! writes go via [`CallMsg`]; reads via [`CallQuery`] -> [`CallReply`]; the
//! per-op assigned stamp is [`CallAssigned`]. authorship is never part of a
//! write payload — the module derives the acting [`Party`] from the dispatch
//! origin — so a write names a party only where it addresses one (a sweep),
//! and the stamp carries the party the module resolved.

use serde::{Deserialize, Serialize};

pub const DEFAULT_CALL_TARGET: &str = "call";

// ---- write-time caps (consensus constants) ---------------------------------

/// participants per channel call; further joins are rejected.
pub const MAX_CALL_MEMBERS: usize = 32;
/// a member's node key: raw ed25519 public key bytes.
pub const NODE_KEY_BYTES: usize = 32;
/// serialized roster record bound, per channel.
pub const MAX_ROSTER_RECORD_BYTES: usize = 64 * 1024;
/// the domain separator [`join_preimage`]'s signature is minted under — the
/// joiner proves it holds the join's `node` key by signing over exactly this
/// namespace plus the channel/user pair, so a join can never be replayed as
/// a different scheme's proof.
pub const JOIN_NS: &[u8] = b"ducktape/call-join/v1";
/// program-origin joins bind the proof to the account in a separate domain.
pub const PROGRAM_JOIN_NS: &[u8] = b"ducktape/call-join/program/v1";

/// who acts on call state — the same party vocabulary chat resolves rosters
/// and memberships in (the module derives it from `Env.origin` at write
/// time, never from a payload: a member key resolves through identity to the
/// account holding it, a program origin IS its account, a key identity does
/// not know stays a key). spelled identically to chat's so a party crosses
/// the `Access` read unchanged.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Party {
    Account(u64),
    Key(Vec<u8>),
    Module(String),
    System,
}

impl Party {
    /// a person's party — an account or a key — as opposed to trusted code.
    /// only people are ever in a call.
    pub fn is_person(&self) -> bool {
        matches!(self, Party::Account(_) | Party::Key(_))
    }
}

/// one participant of a channel's call. `node` is the raw ed25519 key of
/// the member's node — where peers route this participant's media (the media
/// plane authenticates by transport identity; this is routing, not
/// authorship). `party` derives from `Env.origin` like every actor.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub party: Party,
    pub node: Vec<u8>,
    pub joined_at: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CallMsg {
    /// join (or start) the channel's call. people only; chat's post gate
    /// (`ChatQuery::Access`) decides admission, so an archived or
    /// members-only channel gates exactly like posting. idempotent:
    /// re-joining updates `node` (the joiner's node key, [`NODE_KEY_BYTES`]
    /// raw ed25519 bytes) and stages nothing when unchanged. `node_proof` is
    /// `node`'s ed25519 signature over [`join_preimage`]`(channel_id, user)`
    /// under [`JOIN_NS`] — proof that the joining client holds `node`'s
    /// private key. a program origin signs [`program_join_preimage`] under
    /// [`PROGRAM_JOIN_NS`].
    Join {
        channel_id: String,
        node: Vec<u8>,
        node_proof: Vec<u8>,
    },
    /// leave the channel's call. leaving a call one is not in is a
    /// deterministic no-op; the last leaver ends the call.
    Leave { channel_id: String },
    /// evict a member — call liveness is not consensus-observable (a crashed
    /// client cannot leave), so cleanup has two paths: a person naming
    /// themself is a leave in disguise; a person naming anyone else evicts
    /// them, since the room's people are its only cleanup. sweeping an
    /// absent party is a deterministic no-op.
    Sweep { channel_id: String, party: Party },
}

/// the DISPATCH read surface: the roster of one channel, the point read a
/// host (the media executor's admission) or a sibling consumes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CallQuery {
    Roster { channel_id: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CallReply {
    /// join order; empty = no call.
    Roster(Vec<Member>),
}

/// the assigned stamp call declares per applied op ([`sdk::Ctx::set_assigned`]):
/// the parties the module resolved that the op payload cannot carry. rides
/// the dispatch trace onto the derived-tier op-feed row, so feed followers
/// (the index fold, clients) consume exact resolutions.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CallAssigned {
    /// the acting party and the exact existing/new party whose seat the op
    /// affected — a historic key seat is named as the key even after that
    /// key joined an account.
    Participant { actor: Party, participant: Party },
}

impl CallAssigned {
    pub fn participant(&self) -> &Party {
        let Self::Participant { participant, .. } = self;
        participant
    }

    pub fn actor(&self) -> &Party {
        let Self::Participant { actor, .. } = self;
        actor
    }
}

pub fn encode_msg(m: &CallMsg) -> Vec<u8> {
    sdk::wire::encode(m)
}

pub fn decode_msg(b: &[u8]) -> Result<CallMsg, String> {
    sdk::wire::decode(b)
}

pub fn encode_query(q: &CallQuery) -> Vec<u8> {
    sdk::wire::encode(q)
}

pub fn decode_query(b: &[u8]) -> Result<CallQuery, String> {
    sdk::wire::decode(b)
}

pub fn encode_reply(r: &CallReply) -> Vec<u8> {
    sdk::wire::encode(r)
}

pub fn decode_reply(b: &[u8]) -> Result<CallReply, String> {
    sdk::wire::decode(b)
}

pub fn encode_assigned(a: &CallAssigned) -> Vec<u8> {
    sdk::wire::encode(a)
}

pub fn decode_assigned(b: &[u8]) -> Result<CallAssigned, String> {
    sdk::wire::decode(b)
}

/// the bytes a join's `node_proof` signs: `channel_id ‖ user`, each
/// length-prefixed so no delimiter collision lets one field's tail bleed into
/// the next's head. signed and verified under [`JOIN_NS`].
pub fn join_preimage(channel_id: &str, user: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    sdk::codec::push_str(&mut out, channel_id);
    sdk::codec::push_bytes(&mut out, user);
    out
}

/// a node's possession proof for an authenticated program account's join.
pub fn program_join_preimage(channel_id: &str, account: sdk::AccountNumber) -> Vec<u8> {
    join_preimage(channel_id, &account.to_be_bytes())
}
