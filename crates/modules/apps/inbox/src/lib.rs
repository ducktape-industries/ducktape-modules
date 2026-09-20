//! qmdb-backed inbox module: per-ACCOUNT notification queues held as
//! consensus state, fed by the attribution plane.
//!
//! an inbox belongs to an identity account, the stable number every key the
//! human holds resolves to — several keys, one inbox. its items are receipts
//! of the attribution plane's canonical changes: the attribution module
//! queues every change it records for its subscribers, and the host delivers
//! each one here as that module's own follow-up, so a notification commits in
//! the delivery's unit and never without its canonical record. there is no
//! external push service — the queue IS the delivery, which is also the
//! air-gap-native notification story.
//!
//! ## who may do what — told apart by the authenticated origin
//!
//! - a DELIVERY is accepted from `Origin::Module(attribution)` only: the
//!   payload is [`attribution::AttributionEvent::Changed`], and no other
//!   origin's bytes decode as one. the recipient account decides its fate:
//!   a key-held account gets the notification; a program or revoked account
//!   holds no human inbox and is IGNORED (stamped, nothing staged; a
//!   program's controller is never notified on its behalf); an account that
//!   does not exist FAILS the delivery, which the attribution plane keeps as
//!   that delivery's receipt while its queue moves on.
//! - an ADMIN op (`MarkRead`, `Clear`) is accepted from `Origin::External(key)`
//!   only when identity resolves `key` to the account the op names
//!   ([`resolve_admin_account`]). an unbound key, a key of another account, a
//!   program origin, a module and the system are refused before any lookup:
//!   a stranger learns nothing about whether an inbox exists.
//!
//! ## delivery semantics
//!
//! - deliveries from the attribution source arrive in CHANGE ORDER: the
//!   source numbers its items in change order and retires them strictly in
//!   item order. the inbox keeps the last change it queued per account
//!   ([`AccountMeta::last_change`]); a delivery of that same change again is
//!   a DUPLICATE (stamped, nothing staged), and one of an older change is an
//!   ordering violation — an error, never a silent skip.
//! - per account, at most [`MAX_ITEMS_PER_ACCOUNT`] items: when a delivery
//!   would overflow, the OLDEST item (lowest seq) is DROPPED deterministically
//!   and counted ([`AccountMeta::evicted`]) so the loss stays visible. this is
//!   a notification queue, NOT a ledger — the ledger is the attribution plane.
//! - every record is encoded and checked against the store's value bound
//!   BEFORE anything is staged: a delivery whose reference the store cannot
//!   hold fails whole.
//!
//! NO-OP TOLERANCE: `MarkRead`/`Clear` against the key's OWN empty inbox or
//! an unknown seq are deterministic no-ops, never errors — acking an inbox
//! that holds nothing yet is a race a client cannot avoid. that tolerance is
//! scoped to the seq LOOKUP and stops at the account gate.
//!
//! READ TRACKING is a per-account WATERMARK ([`AccountMeta::read_watermark`]),
//! never a per-item flag: `MarkRead` costs one meta read and at most one meta
//! write regardless of queue length, and every read path derives `read` as
//! `seq <= read_watermark`. the watermark is CLAMPED to the last seq ever
//! assigned (`next_seq - 1`), so `MarkRead { up_to_seq: u64::MAX }` never
//! marks a FUTURE delivery pre-read on arrival.
//!
//! ## state model
//!
//! pure logic over a host-injected [`sdk::MerkleStore`]: one META record per
//! account (`meta1\0{account}` → [`AccountMeta`], borsh) and one record per
//! live notification (`item\0{account}{seq}` → [`Notification`]). the meta
//! record lives as long as the account: `next_seq` and `last_change` never
//! rewind, so a cleared inbox continues its numbering and never re-queues a
//! change it already held. NOTHING enumerates accounts (the whole read
//! surface lives on the index tier). writes are staged during a block and
//! flushed in one batch at `commit_block`; the module root IS the store's
//! merkle root, and sync belongs to the store (`QmdbStore::sync_from`).
//!
//! the meta record is VERSIONED in its key. the live network keeps module
//! state across guest swaps, so accounts whose record predates the live-window
//! rewrite still hold the old layout ([`LegacyAccountMeta`], under `meta\0`),
//! and a decode failure on a stranger's inbox is unfixable from outside. so
//! the new layout took a NEW key and the old one is read ONCE, on a miss:
//! [`Inbox::meta`] converts it for a read, [`Inbox::meta_for_write`] also
//! stages the converted record under `meta1\0` and retires the old key. that
//! is state CARRY-OVER, not wire compatibility — bounded to one extra read per
//! account until its first write lands. the fallback is removed in a later
//! round, once no old record can remain.

// this module owns its protocol shapes and codecs; flatten them for existing
// module and test callers without creating a shared API crate.
mod producer;
pub use producer::*;

// the derived-tier read model: the PURE decision core (fold + view over
// index_guest::StateRead), compiled everywhere and unit-tested natively.
// the engine shell that runs it inside the module's index database is
// `index_guest` below.
pub mod index;

// NO CLIENT VIEW MODEL LIVES HERE. What a notification says, which ones are
// noise and what the unread count is are the `inbox` VIEW's own fold
// (`crates/views/inbox`), read off this module's index tier as the JSON the
// wire already is. A rendered-row type here would be a second copy of those
// rules that no view swap could reach.

// the wasm index-mapper shell: wires the pure core into the fluent31 engine.
// compiled only by `guest-builder --index`'s synthesized wasm32 workspace
// (feature `index-guest`), never by the native build.
#[cfg(feature = "index-guest")]
mod index_guest;

use sdk::refusal;
use std::cmp::Ordering;

use attribution::{AttributionEvent, Change, decode_event};
use borsh::{BorshDeserialize, BorshSerialize};
use identity::{
    Control, IdentityQuery, IdentityReply, decode_reply as identity_decode_reply,
    encode_query as identity_encode_query,
};
use sdk::{
    Ctx, Error, MAX_STORE_VALUE_BYTES, MerkleStore, Module, ModuleId, Msg, Origin,
    ResolverSyncTarget, StagedStore, StateRoot, StateSyncHandle,
};

fn module_error(reason: &'static str, text: impl Into<String>) -> Error {
    Error::Module {
        reason: reason.into(),
        sentence: text.into(),
    }
}

/// per-account META record key: prefix + 0 + the account number. every key
/// literal here is fixed and none is another followed by a 0 byte (`meta1`
/// is `meta` followed by `1`, not by 0, so the two key spaces stay disjoint).
fn meta_key(account: AccountNumber) -> Vec<u8> {
    let mut key = Vec::with_capacity(5 + 1 + 8);
    key.extend_from_slice(b"meta1");
    key.push(0);
    key.extend_from_slice(&account.to_le_bytes());
    key
}

/// the PRE-VERSIONED meta key ([`LegacyAccountMeta`]). read only on a miss of
/// [`meta_key`], and never written — see [`Inbox::meta_for_write`].
fn legacy_meta_key(account: AccountNumber) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 1 + 8);
    key.extend_from_slice(b"meta");
    key.push(0);
    key.extend_from_slice(&account.to_le_bytes());
    key
}

/// per-notification record key: prefix + 0 + the account number + big-endian
/// seq.
fn item_key(account: AccountNumber, seq: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 1 + 8 + 8);
    key.extend_from_slice(b"item");
    key.push(0);
    key.extend_from_slice(&account.to_le_bytes());
    key.extend_from_slice(&seq.to_be_bytes());
    key
}

/// one account's queue metadata — five counters, a fixed-width record. seqs
/// are dense and monotonic and BOTH removals (the overflow drop and `Clear`)
/// only ever take the LOW end, so the live set is exactly the contiguous
/// window `first_live..next_seq` and needs no stored list.
///
/// `next_seq` is the NEXT seq to assign; it starts at 1 and never rewinds.
/// `first_live` is the LOWEST live seq (equal to `next_seq` when the queue is
/// empty); it never rewinds either. `evicted` counts every item this account
/// has ever lost to the overflow drop. `read_watermark` is the seq up to which
/// every item is read: `MarkRead` only ever raises it. `last_change` is the
/// canonical seq of the last change queued here — the duplicate gate.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct AccountMeta {
    next_seq: u64,
    first_live: u64,
    evicted: u64,
    read_watermark: u64,
    last_change: u64,
}

impl Default for AccountMeta {
    fn default() -> Self {
        Self {
            next_seq: 1,
            first_live: 1,
            evicted: 0,
            read_watermark: 0,
            last_change: 0,
        }
    }
}

/// the meta record as it was stored BEFORE the live-window rewrite: the same
/// four counters plus the explicit live-seq list `first_live` replaced. kept
/// to DECODE accounts whose record predates the change and nothing else — it
/// is never written, and it goes away with the fallback in a later round.
#[derive(BorshDeserialize)]
struct LegacyAccountMeta {
    next_seq: u64,
    seqs: Vec<u64>,
    evicted: u64,
    read_watermark: u64,
    last_change: u64,
}

impl From<LegacyAccountMeta> for AccountMeta {
    /// `seqs` was always CONTIGUOUS — pushes are sequential and both removals
    /// (the overflow drop and `Clear`) drain the low end — so the list is
    /// exactly the window its first entry opens. an empty list is an empty
    /// queue, whose window is `next_seq..next_seq`. every counter carries
    /// over verbatim.
    fn from(old: LegacyAccountMeta) -> Self {
        Self {
            next_seq: old.next_seq,
            first_live: old.seqs.first().copied().unwrap_or(old.next_seq),
            evicted: old.evicted,
            read_watermark: old.read_watermark,
            last_change: old.last_change,
        }
    }
}

/// one dispatch as this module understands it, classified by the
/// authenticated origin before any handler runs.
enum Input {
    /// the attribution source's delivery of one canonical change (boxed:
    /// the change dwarfs the admin arms).
    Changed(Box<Change>),
    MarkRead {
        account: AccountNumber,
        up_to_seq: u64,
    },
    Clear {
        account: AccountNumber,
        up_to_seq: u64,
    },
}

/// what one delivery decided: the writes to stage and the stamp it earns.
/// pure — decided against the loaded meta, staged by the one writer.
enum Ingest {
    Queued {
        meta: AccountMeta,
        seq: u64,
        record: Vec<u8>,
        /// the one item this delivery pushed out of the window, if any.
        evicted: Option<u64>,
    },
    Duplicate,
}

/// the pure delivery decision over one account's meta: the duplicate gate,
/// the seq allocation (checked), the overflow drop, and the record encoded
/// and checked against the store's bound. writes nothing.
fn decide_delivery(
    meta: &AccountMeta,
    account: AccountNumber,
    change: &Change,
    created_at: u64,
) -> Result<Ingest, Error> {
    match change.seq.cmp(&meta.last_change) {
        Ordering::Equal => return Ok(Ingest::Duplicate),
        Ordering::Less => {
            return Err(module_error(
                refusal::STALE,
                format!(
                    "change {} reached account {account}'s inbox after change {}: deliveries arrive in change order",
                    change.seq, meta.last_change
                ),
            ));
        }
        Ordering::Greater => {}
    }
    let mut meta = meta.clone();
    // seq-space exhaustion is a deterministic rejection, checked BEFORE any
    // mutation — never a panic or a wrapping re-assignment of an old seq.
    let seq = meta.next_seq;
    meta.next_seq = seq.checked_add(1).ok_or_else(|| {
        module_error(
            refusal::EXHAUSTED,
            format!("inbox seq space exhausted for account {account}"),
        )
    })?;
    meta.last_change = change.seq;
    // overflow: drop the OLDEST (lowest seq) item. one insert per delivery
    // means at most one drop, counted so the loss stays visible. the live set
    // is the window `first_live..next_seq`, so the drop is a counter bump —
    // never a scan of the queue.
    let live = meta.next_seq.saturating_sub(meta.first_live);
    let mut evicted = None;
    if live > MAX_ITEMS_PER_ACCOUNT as u64 {
        // `first_live < next_seq` here (the window is over-full), so the bump
        // cannot overflow.
        evicted = Some(meta.first_live);
        meta.first_live += 1;
        meta.evicted = meta.evicted.checked_add(1).ok_or_else(|| {
            module_error(
                refusal::EXHAUSTED,
                format!("inbox eviction count exhausted for account {account}"),
            )
        })?;
    }
    let record = borsh::to_vec(&Notification {
        seq,
        account,
        change: change.reference(),
        created_at,
    })
    .expect("inbox record is serializable");
    let fits_the_store = record.len() <= MAX_STORE_VALUE_BYTES;
    if !fits_the_store {
        return Err(module_error(
            refusal::CAPACITY,
            format!(
                "a notification of {} bytes exceeds the store's value bound of {MAX_STORE_VALUE_BYTES}",
                record.len()
            ),
        ));
    }
    Ok(Ingest::Queued {
        meta,
        seq,
        record,
        evicted,
    })
}

pub struct Inbox {
    id: ModuleId,
    /// the attribution module — the ONE origin whose payloads are deliveries.
    attribution: ModuleId,
    /// the identity module — the resolver of recipients and of admin keys.
    identity: ModuleId,
    /// the host-injected authenticated store plus this block's staging overlay
    /// (read-your-writes, folded into `root()` at `commit_block`). store key
    /// is `sha256(logical_key)`, owned by [`StagedStore`].
    staged: StagedStore,
}

impl Inbox {
    /// wrap the host-constructed store under module identity `id`, wired to
    /// its two collaborators by their genesis-constant ids.
    pub fn new(
        id: impl Into<ModuleId>,
        store: Box<dyn MerkleStore>,
        attribution: impl Into<ModuleId>,
        identity: impl Into<ModuleId>,
    ) -> Self {
        Self {
            id: id.into(),
            attribution: attribution.into(),
            identity: identity.into(),
            staged: StagedStore::new(store),
        }
    }

    // ---- staged-over-committed reads ----------------------------------------

    async fn load<T>(&self, key: &[u8]) -> Result<Option<T>, Error>
    where
        T: BorshDeserialize,
    {
        match self.staged.get(key).await? {
            Some(bytes) => {
                Ok(Some(borsh::from_slice(&bytes).map_err(|e| {
                    module_error(refusal::CORRUPT, e.to_string())
                })?))
            }
            None => Ok(None),
        }
    }

    /// stage a meta record — five u64 counters, a fixed-width record, so no
    /// byte gate is needed here.
    fn store_meta(&mut self, account: AccountNumber, meta: &AccountMeta) {
        self.staged.stage(
            meta_key(account),
            borsh::to_vec(meta).expect("inbox meta is serializable"),
        );
    }

    /// one account's meta, READ-ONLY: the current record, or — for an account
    /// whose record predates the live-window rewrite — the pre-versioned one
    /// converted on the spot. converting here stages NOTHING, so a caller
    /// that cannot write never silently upgrades a record behind a read.
    async fn meta(&self, account: AccountNumber) -> Result<Option<AccountMeta>, Error> {
        if let Some(meta) = self.load(&meta_key(account)).await? {
            return Ok(Some(meta));
        }
        Ok(self
            .load::<LegacyAccountMeta>(&legacy_meta_key(account))
            .await?
            .map(AccountMeta::from))
    }

    /// the meta a WRITE path works from: as [`Inbox::meta`], plus the one-time
    /// CARRY-OVER — a record found under the pre-versioned key is converted,
    /// staged under the current key and the old key retired, all in the very
    /// operation that observed the miss. bounded: one extra read per account,
    /// once, and only until that account's first write lands.
    async fn meta_for_write(
        &mut self,
        account: AccountNumber,
    ) -> Result<Option<AccountMeta>, Error> {
        if let Some(meta) = self.load(&meta_key(account)).await? {
            return Ok(Some(meta));
        }
        let Some(legacy) = self
            .load::<LegacyAccountMeta>(&legacy_meta_key(account))
            .await?
        else {
            return Ok(None);
        };
        let meta = AccountMeta::from(legacy);
        self.store_meta(account, &meta);
        self.staged.delete(legacy_meta_key(account));
        Ok(Some(meta))
    }

    /// a live item of the meta's `first_live..next_seq` window. a seq inside
    /// the window without its record is a store bug — loud, never skipped.
    #[cfg(feature = "testkit")]
    async fn item(&self, account: AccountNumber, seq: u64) -> Result<Notification, Error> {
        self.load(&item_key(account, seq)).await?.ok_or_else(|| {
            module_error(
                refusal::CORRUPT,
                format!("the inbox of account {account} lists item {seq} with no record"),
            )
        })
    }

    // ---- the identity seam ----------------------------------------------------

    async fn identity_account(
        &self,
        ctx: &dyn Ctx,
        query: &IdentityQuery,
    ) -> Result<Option<identity::AccountView>, Error> {
        let reply = ctx
            .query(&self.identity, &identity_encode_query(query))
            .await?;
        match identity_decode_reply(&reply).map_err(|sentence| Error::Module {
            reason: refusal::UNEXPECTED_REPLY.into(),
            sentence,
        })? {
            IdentityReply::Account(account) => Ok(account),
            IdentityReply::Accounts(_) | IdentityReply::Resolved(_) | IdentityReply::Gen(_) => {
                Err(module_error(
                    refusal::UNEXPECTED_REPLY,
                    "identity answered an account lookup with something other than an account",
                ))
            }
        }
    }

    /// how the recipient of a change is controlled, or `None` for an account
    /// that does not exist.
    async fn recipient_control(
        &self,
        ctx: &dyn Ctx,
        recipient: AccountNumber,
    ) -> Result<Option<Control>, Error> {
        let account = self
            .identity_account(ctx, &IdentityQuery::Get { number: recipient })
            .await?;
        Ok(account.map(|view| view.control))
    }

    /// the ONE admin-authority decision: the submitting key must be one of
    /// the named account's keys, resolved through identity's `OfKey`. every
    /// other origin is refused before any lookup, so no stranger learns
    /// whether an inbox exists from which answer comes back.
    async fn resolve_admin_account(
        &self,
        ctx: &dyn Ctx,
        account: AccountNumber,
    ) -> Result<(), Error> {
        let key = match &ctx.env().origin {
            Origin::External(key) if !key.is_empty() => key.clone(),
            Origin::External(_) => {
                return Err(module_error(
                    refusal::INVALID_INPUT,
                    "external origin must carry a non-empty submitter key",
                ));
            }
            Origin::Program(program) => {
                return Err(module_error(
                    refusal::UNAUTHORIZED,
                    format!("a program account holds no human inbox: {program}"),
                ));
            }
            Origin::Module(id) => {
                return Err(module_error(
                    refusal::UNAUTHORIZED,
                    format!("a module holds no inbox: {id}"),
                ));
            }
            Origin::System => {
                return Err(module_error(
                    refusal::UNAUTHORIZED,
                    "the system holds no inbox",
                ));
            }
        };
        let holder = self
            .identity_account(ctx, &IdentityQuery::OfKey { key })
            .await?;
        let Some(holder) = holder else {
            return Err(module_error(
                refusal::NOT_FOUND,
                "this key belongs to no identity account",
            ));
        };
        let holds_the_account = holder.number == account;
        if !holds_the_account {
            return Err(module_error(
                refusal::UNAUTHORIZED,
                format!(
                    "only the account's own keys may ack its inbox: this key holds account {}, not {account}",
                    holder.number
                ),
            ));
        }
        let is_key_held = matches!(holder.control, Control::Keys);
        if !is_key_held {
            return Err(module_error(
                refusal::INVALID_INPUT,
                format!("account {account} is not key-held and holds no human inbox"),
            ));
        }
        Ok(())
    }

    // ---- classification ----------------------------------------------------------

    /// the ONE place the origin decides what the bytes are: the attribution
    /// source's bytes are a delivery, every other origin's are an admin op.
    fn classify(&self, origin: &Origin, payload: &[u8]) -> Result<Input, Error> {
        let from_attribution = *origin == Origin::Module(self.attribution.clone());
        if from_attribution {
            let AttributionEvent::Changed(change) =
                decode_event(payload).map_err(|sentence| Error::Module {
                    reason: refusal::UNEXPECTED_REPLY.into(),
                    sentence,
                })?;
            return Ok(Input::Changed(Box::new(change)));
        }
        Ok(
            match decode_msg(payload).map_err(|sentence| Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence,
            })? {
                InboxMsg::MarkRead { account, up_to_seq } => Input::MarkRead { account, up_to_seq },
                InboxMsg::Clear { account, up_to_seq } => Input::Clear { account, up_to_seq },
            },
        )
    }

    // ---- the handlers ----------------------------------------------------------

    /// the attribution source's delivery of one change: the recipient's
    /// control decides, the delivery decision is pure, the writer stages.
    async fn on_changed(&mut self, ctx: &mut dyn Ctx, change: Change) -> Result<(), Error> {
        let recipient = change.recipient;
        let Some(control) = self.recipient_control(ctx, recipient).await? else {
            return Err(module_error(
                refusal::NOT_FOUND,
                format!("recipient account {recipient} does not exist"),
            ));
        };
        let holds_a_human_inbox = matches!(control, Control::Keys);
        if !holds_a_human_inbox {
            ctx.set_assigned(encode_assigned(&InboxAssigned::Ignored));
            return Ok(());
        }
        let meta = self.meta_for_write(recipient).await?.unwrap_or_default();
        let created_at = ctx.env().consensus_time;
        let stamp = match decide_delivery(&meta, recipient, &change, created_at)? {
            Ingest::Duplicate => InboxAssigned::Duplicate,
            Ingest::Queued {
                meta,
                seq,
                record,
                evicted,
            } => {
                if let Some(oldest) = evicted {
                    self.staged.delete(item_key(recipient, oldest));
                }
                self.staged.stage(item_key(recipient, seq), record);
                self.store_meta(recipient, &meta);
                InboxAssigned::Delivered { seq }
            }
        };
        ctx.set_assigned(encode_assigned(&stamp));
        Ok(())
    }

    /// O(1): one meta read, at most one meta write — never a per-item read or
    /// write. `read_watermark` only ever rises, so an `up_to_seq` at or below
    /// it is a byte-identical no-op (idempotent re-acks never move the root).
    async fn on_mark_read(
        &mut self,
        ctx: &mut dyn Ctx,
        account: AccountNumber,
        up_to_seq: u64,
    ) -> Result<(), Error> {
        self.resolve_admin_account(ctx, account).await?;
        let Some(mut meta) = self.meta_for_write(account).await? else {
            return Ok(());
        };
        // clamp to the last seq ever ASSIGNED, never the raw `up_to_seq`: an
        // unclamped watermark would mark every FUTURE delivery pre-read.
        let last_seq = meta.next_seq.saturating_sub(1);
        let watermark = up_to_seq.min(last_seq);
        let already_read = watermark <= meta.read_watermark;
        if already_read {
            return Ok(());
        }
        meta.read_watermark = watermark;
        self.store_meta(account, &meta);
        Ok(())
    }

    async fn on_clear(
        &mut self,
        ctx: &mut dyn Ctx,
        account: AccountNumber,
        up_to_seq: u64,
    ) -> Result<(), Error> {
        self.resolve_admin_account(ctx, account).await?;
        let Some(mut meta) = self.meta_for_write(account).await? else {
            return Ok(());
        };
        // the cleared prefix is `first_live..new_first`, clamped to the live
        // window so an `up_to_seq` past the end clears exactly the queue and
        // an `up_to_seq` below `first_live` is a byte-identical no-op. the
        // deletes are the work here; the bookkeeping is one counter.
        let new_first = up_to_seq.saturating_add(1).min(meta.next_seq);
        let nothing_to_clear = new_first <= meta.first_live;
        if nothing_to_clear {
            return Ok(());
        }
        for seq in meta.first_live..new_first {
            self.staged.delete(item_key(account, seq));
        }
        meta.first_live = new_first;
        // next_seq and last_change are left untouched: neither ever rewinds,
        // so a cleared inbox continues its numbering and never re-queues a
        // change it already held.
        self.store_meta(account, &meta);
        Ok(())
    }

    /// the one dispatch: one arm per [`Input`] variant, each arm one call to
    /// the handler named for it.
    async fn dispatch(&mut self, ctx: &mut dyn Ctx, input: Input) -> Result<(), Error> {
        match input {
            Input::Changed(change) => self.on_changed(ctx, *change).await,
            Input::MarkRead { account, up_to_seq } => {
                self.on_mark_read(ctx, account, up_to_seq).await
            }
            Input::Clear { account, up_to_seq } => self.on_clear(ctx, account, up_to_seq).await,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Module for Inbox {
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

    /// the network state-sync serve lane: answers the shared qmdb wire requests
    /// (historical proof-carrying op ranges) from committed state. read-only;
    /// the joiner's sync engine merkle-verifies every batch.
    async fn serve_sync(&self, req: &[u8]) -> Result<Vec<u8>, Error> {
        self.staged.serve_sync(req).await
    }

    async fn resolver_sync_target(&self) -> Result<ResolverSyncTarget, Error> {
        self.staged.sync_target().await
    }

    /// the origin is bound ONCE, before the payload is decoded: it decides
    /// what the bytes are (a delivery or an admin op) and every arm gates on
    /// it — an arm that took no origin would be exactly the class of bug the
    /// two gates exist to close.
    async fn execute(&mut self, ctx: &mut dyn Ctx, msg: &Msg) -> Result<(), Error> {
        let input = self.classify(&ctx.env().origin, &msg.payload)?;
        self.dispatch(ctx, input).await
    }

    // NO `query`: nothing in any execute() path reads an inbox, so the whole
    // read surface (paged lists, unread counts) is the index guest's job
    // (`index.rs`) on the derived tier. the default `Error::QueryUnsupported`
    // is the honest answer here.

    /// publish the block's staged writes in ONE store batch. no-op (and no
    /// root movement) if nothing was staged.
    async fn commit_block(&mut self) -> Result<(), Error> {
        self.staged.commit().await
    }

    async fn abort_block(&mut self) -> Result<(), Error> {
        self.staged.abort();
        Ok(())
    }
}

// test-only inspection reads. dev-only: inbox deliberately has NO wire query
// surface (the index tier owns every read), so the state-side tests probe the
// records through this feature-gated seam instead of golden byte images.
#[cfg(feature = "testkit")]
impl Inbox {
    /// one account's staged-over-committed queue: `(next_seq, live items in
    /// seq order)`; `None` for an account never delivered to.
    pub async fn queue_view(
        &self,
        account: AccountNumber,
    ) -> Result<Option<(u64, Vec<Notification>)>, Error> {
        let Some(meta) = self.meta(account).await? else {
            return Ok(None);
        };
        let mut items = Vec::new();
        for seq in meta.first_live..meta.next_seq {
            items.push(self.item(account, seq).await?);
        }
        Ok(Some((meta.next_seq, items)))
    }

    /// the number of items this account has ever lost to the overflow drop —
    /// `0` for an account never delivered to.
    pub async fn evicted_count(&self, account: AccountNumber) -> Result<u64, Error> {
        Ok(self.meta(account).await?.map(|m| m.evicted).unwrap_or(0))
    }

    /// an account's read watermark — everything at or below it reads as
    /// read. `0` (never marked) for an account never delivered to.
    pub async fn read_watermark_view(&self, account: AccountNumber) -> Result<u64, Error> {
        Ok(self
            .meta(account)
            .await?
            .map(|m| m.read_watermark)
            .unwrap_or(0))
    }

    /// whether `seq` in `account`'s inbox currently reads as read — derived
    /// from the watermark, exactly like every real read path.
    pub async fn is_read(&self, account: AccountNumber, seq: u64) -> Result<bool, Error> {
        Ok(seq <= self.read_watermark_view(account).await?)
    }

    /// the canonical seq of the last change queued for `account` — the
    /// duplicate gate. `0` for an account never delivered to.
    pub async fn last_change_view(&self, account: AccountNumber) -> Result<u64, Error> {
        Ok(self
            .meta(account)
            .await?
            .map(|m| m.last_change)
            .unwrap_or(0))
    }

    /// where `account`'s meta record physically sits: `(under the current
    /// key, under the pre-versioned one)`. the carry-over's only visible
    /// effect — every view reads the same meta either way, so nothing else
    /// can tell a converted record from one still read through the old key.
    pub async fn meta_records_present(
        &self,
        account: AccountNumber,
    ) -> Result<(bool, bool), Error> {
        Ok((
            self.staged.get(&meta_key(account)).await?.is_some(),
            self.staged.get(&legacy_meta_key(account)).await?.is_some(),
        ))
    }

    /// stage an account whose seq space is one delivery from exhaustion — the
    /// boundary state is execute-reachable only after 2^64 - 2 deliveries, so
    /// the exhaustion test injects it instead.
    pub async fn testkit_saturate_seq(&mut self, account: AccountNumber) -> Result<(), Error> {
        let mut meta = self.meta(account).await?.unwrap_or_default();
        meta.next_seq = u64::MAX;
        // an empty queue at the end of the seq space: the window stays
        // coherent (`first_live == next_seq`) rather than claiming 2^64 items.
        meta.first_live = meta.next_seq;
        self.store_meta(account, &meta);
        Ok(())
    }
}

// the wasm-guest port: the dispatch shell that adapts this module to the
// ducktape:module world. compiled only by the guest-builder's synthesized
// wasm32 cdylib workspace (feature `guest`), never by the native build.
#[cfg(feature = "guest")]
mod guest;
