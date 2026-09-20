//! The call module: who is in a channel's call (consensus), and the media
//! planes the call rides (off consensus) — one crate, two halves.
//!
//! ## the consensus half
//!
//! pure logic over a host-injected [`sdk::MerkleStore`]: one roster record
//! per channel (`Vec<Member>` in join order; an empty roster is a deleted
//! record, so absent == no call). every read the dispatch path needs is a
//! point lookup; the human-facing listing (who is in which call) is served
//! by the index guest (`index.rs`) on the derived tier.
//!
//! authority over a channel is CHAT's. a join makes exactly ONE bounded chat
//! read — `ChatQuery::Access` for the joining party — and admits on chat's
//! `may_post` answer, so an archived or members-only channel gates a join
//! exactly like a post and call never carries a second copy of the rule.
//! leaving and sweeping read chat not at all: an absent roster is a no-op.
//! when chat archives a channel it emits `ChatEvent::ChannelArchived` to
//! this module, which clears the roster in the same unit.
//!
//! every write acts as ONE [`Party`], derived from `ctx.env().origin` and
//! never from a payload: an external key resolves through the identity
//! sibling to the account holding it, and stays a bare [`Party::Key`] when
//! identity knows none; a program origin is its account. a seat a key took
//! before it joined an account stays that key's: the authenticated key keeps
//! its historic entry instead of gaining a second one.
//!
//! ## the media half
//!
//! [`voice`], [`video`] and [`call_wire`] are the real-time media planes,
//! ported from ducktape `crates/services/media` (dfa41ce1) as pure state
//! machines: the same wire layouts, the same codec contract, the same
//! jitter/mix/reassembly decisions. What is NOT here is everything a wasm
//! guest cannot own — the data-plane flow, the pump task, the tokio clock,
//! the random media epoch and the tracing sink. Each became an explicit
//! input: the host hands the engine datagrams ([`voice::VoiceEngine::receive`])
//! and takes back frames ([`voice::VoiceEngine::encode_frame`]), ticks
//! playout at its own 20 ms cadence, and chooses the epoch at construction.

pub mod call_wire;
pub mod consumer_wire;
pub mod index;
#[cfg(any(test, feature = "selftest"))]
pub mod selftest;
pub mod video;
pub mod voice;
mod wire;

pub use wire::*;

// the wasm consensus port (feature `guest`) and index-mapper shell (feature
// `index-guest`): compiled only by guest-builder's synthesized wasm32
// workspaces, never by the native build.
#[cfg(feature = "guest")]
mod guest;
#[cfg(feature = "index-guest")]
mod index_guest;

use consumer_wire::chat::{
    ChatEvent, ChatQuery, ChatReply, decode_event as chat_decode_event,
    decode_reply as chat_decode_reply, encode_query as chat_encode_query,
};
use consumer_wire::identity::{
    IdentityQuery, IdentityReply, decode_reply as identity_decode_reply,
    encode_query as identity_encode_query,
};
use sdk::refusal;
use sdk::{
    Ctx, Error, MerkleStore, Module, ModuleId, Msg, Origin, ResolverSyncTarget, StagedStore,
    StateRoot, StateSyncHandle, require_non_empty,
};

/// Largest data-plane datagram payload: the plane's 1372-byte frame
/// (overlay MTU 1420 − IPv6 40 − UDP 8) minus its 9-byte service+flow
/// header. Copied from `data_plane::MAX_DATAGRAM_PAYLOAD` so the media
/// frames sized against it fit one datagram without linking the plane.
pub const MAX_DATAGRAM_PAYLOAD: usize = 1372 - 9;

/// A transport-authenticated peer: the raw 32-byte node key the data plane
/// binds every datagram to. The same bytes as `data_plane::PeerId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub [u8; 32]);

/// single-component key: prefix + 0 + id. safe because the prefix is a fixed
/// literal.
fn roster_key(channel_id: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(7 + channel_id.len());
    key.extend_from_slice(b"roster");
    key.push(0);
    key.extend_from_slice(channel_id.as_bytes());
    key
}

/// account attribution and proof of key ownership are separate facts: joining
/// an account never transfers an older key-owned seat to its other keys.
struct Authority {
    party: Party,
    origin: Origin,
}

impl Authority {
    /// prefer a current account seat; otherwise retain an existing seat
    /// owned by this exact signing key. other account keys cannot claim it.
    fn participant(&self, roster: &[Member]) -> Party {
        if roster.iter().any(|m| m.party == self.party) {
            return self.party.clone();
        }
        let Origin::External(key) = &self.origin else {
            return self.party.clone();
        };
        let historical = Party::Key(key.clone());
        if roster.iter().any(|m| m.party == historical) {
            return historical;
        }
        self.party.clone()
    }
}

/// storage-backed call module.
pub struct Call {
    id: ModuleId,
    /// the host-injected authenticated store plus this block's staging
    /// overlay (logical-key -> staged write; `None` = delete).
    staged: StagedStore,
    /// the chat sibling that owns every channel: the one `Access` read a
    /// join makes, and the origin whose `ChannelArchived` event clears a
    /// roster.
    chat: ModuleId,
    /// the identity sibling every external origin resolves through
    /// (`OfKey`). `None` = no sibling on this host (tests, minimal
    /// registries): every external key stays a key.
    identity: Option<ModuleId>,
}

impl Call {
    /// wrap the host-constructed store under module identity `id`, with
    /// `chat` as the channel authority.
    pub fn new(id: impl Into<ModuleId>, chat: impl Into<ModuleId>, store: Box<dyn MerkleStore>) -> Self {
        Self {
            id: id.into(),
            staged: StagedStore::new(store),
            chat: chat.into(),
            identity: None,
        }
    }

    /// resolve external keys through `identity`.
    pub fn with_identity(mut self, identity: impl Into<ModuleId>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    async fn roster(&self, channel_id: &str) -> Result<Vec<Member>, Error> {
        match self.staged.get(&roster_key(channel_id)).await? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence: e.to_string(),
            }),
            None => Ok(Vec::new()),
        }
    }

    /// stage the roster; an empty one is a deleted record.
    fn store_roster(&mut self, channel_id: &str, roster: &[Member]) -> Result<(), Error> {
        if roster.is_empty() {
            self.staged.delete(roster_key(channel_id));
            return Ok(());
        }
        let bytes = serde_json::to_vec(roster).expect("a roster is serializable");
        if bytes.len() > MAX_ROSTER_RECORD_BYTES {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!(
                    "roster record too large: {} > {MAX_ROSTER_RECORD_BYTES} bytes",
                    bytes.len()
                ),
            });
        }
        self.staged.stage(roster_key(channel_id), bytes);
        Ok(())
    }

    /// the account holding `key`, through identity's `OfKey`. `None` when
    /// identity knows no such key — or when this host wires no identity
    /// sibling, which knows no key at all.
    async fn account_of_key(&self, ctx: &dyn Ctx, key: &[u8]) -> Result<Option<u64>, Error> {
        let Some(identity) = &self.identity else {
            return Ok(None);
        };
        let reply = ctx
            .query(
                identity,
                &identity_encode_query(&IdentityQuery::OfKey { key: key.to_vec() }),
            )
            .await?;
        match identity_decode_reply(&reply).map_err(|sentence| Error::Module {
            reason: refusal::UNEXPECTED_REPLY.into(),
            sentence,
        })? {
            IdentityReply::Account(account) => Ok(account.map(|view| view.number)),
            IdentityReply::Accounts(_) | IdentityReply::Resolved(_) | IdentityReply::Gen(_) => {
                Err(Error::Module {
                    reason: refusal::UNEXPECTED_REPLY.into(),
                    sentence: "identity answered a key lookup with something other than an account"
                        .into(),
                })
            }
        }
    }

    /// the party the dispatch origin acts as — the only authorship path.
    async fn party_of_origin(&self, ctx: &dyn Ctx, origin: &Origin) -> Result<Party, Error> {
        match origin {
            Origin::External(key) if key.is_empty() => Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: "external origin must carry a non-empty submitter id".into(),
            }),
            Origin::External(key) => Ok(match self.account_of_key(ctx, key).await? {
                Some(account) => Party::Account(account),
                None => Party::Key(key.clone()),
            }),
            Origin::Module(id) => Ok(Party::Module(id.clone())),
            Origin::Program(account) => Ok(Party::Account(*account)),
            Origin::System => Ok(Party::System),
        }
    }

    /// the ONE chat read a join makes: may `party` post in `channel_id`?
    /// chat's post gate verbatim — an unknown channel answers `false`, so
    /// the join fails closed.
    async fn may_post(&self, ctx: &dyn Ctx, channel_id: &str, party: &Party) -> Result<bool, Error> {
        let reply = ctx
            .query(
                &self.chat,
                &chat_encode_query(&ChatQuery::Access {
                    channel_id: channel_id.into(),
                    party: party.clone(),
                }),
            )
            .await?;
        match chat_decode_reply(&reply).map_err(|sentence| Error::Module {
            reason: refusal::UNEXPECTED_REPLY.into(),
            sentence,
        })? {
            ChatReply::Access(access) => Ok(access.may_post),
            ChatReply::Channel(_) | ChatReply::Messages(_) | ChatReply::Message(_) => {
                Err(Error::Module {
                    reason: refusal::UNEXPECTED_REPLY.into(),
                    sentence: "chat answered an access probe with something other than access"
                        .into(),
                })
            }
        }
    }

    fn require_person(party: &Party, verb: &str) -> Result<(), Error> {
        if party.is_person() {
            return Ok(());
        }
        Err(Error::Module {
            reason: refusal::UNAUTHORIZED.into(),
            sentence: format!("only people may {verb} a call"),
        })
    }

    /// join (or start) the channel's call. only people may; `node_proof`
    /// must verify as `node`'s own signature over this join (proof of
    /// possession — see [`join_preimage`]); chat's `Access` gate admits.
    /// re-joining with the same node key stages nothing (idempotent,
    /// byte-identical op log).
    async fn stage_join(
        &mut self,
        ctx: &dyn Ctx,
        authority: &Authority,
        channel_id: &str,
        node: Vec<u8>,
        node_proof: Vec<u8>,
        now: u64,
    ) -> Result<Party, Error> {
        require_non_empty("channel_id", channel_id)?;
        Self::require_person(&authority.party, "join")?;
        if node.len() != NODE_KEY_BYTES {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!(
                    "node key must be {NODE_KEY_BYTES} bytes, got {}",
                    node.len()
                ),
            });
        }
        let (namespace, preimage) = match &authority.origin {
            Origin::External(key) => (JOIN_NS, join_preimage(channel_id, key)),
            Origin::Program(account) => {
                (PROGRAM_JOIN_NS, program_join_preimage(channel_id, *account))
            }
            Origin::Module(_) | Origin::System => {
                return Err(Error::Module {
                    reason: refusal::UNAUTHORIZED.into(),
                    sentence: "only people may join a call".into(),
                });
            }
        };
        if !keyscheme::KeyScheme::Ed25519.verify(&node, namespace, &preimage, &node_proof) {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!("the node key's proof for joining the call in {channel_id} does not verify"),
            });
        }
        let mut roster = self.roster(channel_id).await?;
        let party = authority.participant(&roster);
        // the seat's party is what chat judges: a historic key seat is the
        // key chat admitted before it held an account.
        if !self.may_post(ctx, channel_id, &party).await? {
            return Err(Error::Module {
                reason: refusal::UNAUTHORIZED.into(),
                sentence: format!("chat does not admit this party to {channel_id}"),
            });
        }
        if let Some(existing) = roster.iter_mut().find(|m| m.party == party) {
            if existing.node == node {
                return Ok(party);
            }
            existing.node = node;
        } else {
            if roster.len() >= MAX_CALL_MEMBERS {
                return Err(Error::Module {
                    reason: refusal::CAPACITY.into(),
                    sentence: format!("the call in {channel_id} is full"),
                });
            }
            roster.push(Member {
                party: party.clone(),
                node,
                joined_at: now,
            });
        }
        self.store_roster(channel_id, &roster)?;
        Ok(party)
    }

    /// leave the channel's call. absent participation is a deterministic
    /// no-op; the last leaver ends the call.
    async fn stage_leave(&mut self, authority: &Authority, channel_id: &str) -> Result<Party, Error> {
        require_non_empty("channel_id", channel_id)?;
        Self::require_person(&authority.party, "leave")?;
        let mut roster = self.roster(channel_id).await?;
        let party = authority.participant(&roster);
        let before = roster.len();
        roster.retain(|m| m.party != party);
        if roster.len() != before {
            self.store_roster(channel_id, &roster)?;
        }
        Ok(party)
    }

    /// evict `target` from the channel's call (see [`CallMsg::Sweep`]). a
    /// person naming themself is a leave in disguise; naming anyone else
    /// evicts them, by any person: a [`Member`] carries only `joined_at`,
    /// set once at join and never refreshed on liveness, so the module holds
    /// no presence signal a staleness rule could read, and the room's people
    /// are its only cleanup. absent target = no-op either way.
    async fn stage_sweep(
        &mut self,
        authority: &Authority,
        channel_id: &str,
        target: &Party,
    ) -> Result<Party, Error> {
        require_non_empty("channel_id", channel_id)?;
        Self::require_person(&authority.party, "sweep")?;
        if *target == authority.party {
            return self.stage_leave(authority, channel_id).await;
        }
        let mut roster = self.roster(channel_id).await?;
        let before = roster.len();
        roster.retain(|m| m.party != *target);
        if roster.len() != before {
            self.store_roster(channel_id, &roster)?;
        }
        Ok(target.clone())
    }

    /// chat's follow-up: a channel closed, so its call ends. any other chat
    /// event is not addressed to call and is a deterministic no-op.
    async fn apply_chat_event(&mut self, payload: &[u8]) -> Result<(), Error> {
        let event = chat_decode_event(payload).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        if let ChatEvent::ChannelArchived { channel_id } = event
            && !self.roster(&channel_id).await?.is_empty()
        {
            self.staged.delete(roster_key(&channel_id));
        }
        Ok(())
    }

    async fn execute_op(&mut self, ctx: &mut dyn Ctx, msg: &Msg) -> Result<(), Error> {
        let env = ctx.env().clone();
        if env.origin == Origin::Module(self.chat.clone()) {
            return self.apply_chat_event(&msg.payload).await;
        }
        let call = decode_msg(&msg.payload).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        let party = self.party_of_origin(&*ctx, &env.origin).await?;
        let authority = Authority {
            party: party.clone(),
            origin: env.origin.clone(),
        };
        let participant = match call {
            CallMsg::Join {
                channel_id,
                node,
                node_proof,
            } => {
                self.stage_join(&*ctx, &authority, &channel_id, node, node_proof, env.consensus_time)
                    .await?
            }
            CallMsg::Leave { channel_id } => self.stage_leave(&authority, &channel_id).await?,
            CallMsg::Sweep {
                channel_id,
                party: target,
            } => self.stage_sweep(&authority, &channel_id, &target).await?,
        };
        ctx.set_assigned(encode_assigned(&CallAssigned::Participant {
            actor: party,
            participant,
        }));
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl Module for Call {
    fn id(&self) -> ModuleId {
        self.id.clone()
    }

    /// the store's merkle root over all committed records, verbatim — the
    /// staged overlay is invisible here until `commit_block`.
    fn root(&self) -> StateRoot {
        self.staged.root()
    }

    fn state_sync_handle(&self) -> Result<StateSyncHandle, Error> {
        self.staged.state_sync_handle()
    }

    async fn serve_sync(&self, req: &[u8]) -> Result<Vec<u8>, Error> {
        self.staged.serve_sync(req).await
    }

    async fn resolver_sync_target(&self) -> Result<ResolverSyncTarget, Error> {
        self.staged.sync_target().await
    }

    async fn execute(&mut self, ctx: &mut dyn Ctx, msg: &Msg) -> Result<(), Error> {
        let checkpoint = self.staged.checkpoint();
        match self.execute_op(ctx, msg).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.staged.restore(checkpoint);
                Err(error)
            }
        }
    }

    async fn query(&self, req: &[u8]) -> Result<Vec<u8>, Error> {
        match decode_query(req).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })? {
            CallQuery::Roster { channel_id } => Ok(encode_reply(&CallReply::Roster(
                self.roster(&channel_id).await?,
            ))),
        }
    }

    /// publish the block's staged writes in ONE store batch.
    async fn commit_block(&mut self) -> Result<(), Error> {
        self.staged.commit().await
    }

    async fn abort_block(&mut self) -> Result<(), Error> {
        self.staged.abort();
        Ok(())
    }
}
