//! Callable model work for programmable users. Model configuration and context
//! are consensus state; workers return data or propose session actions. A
//! user's program chooses each source write and receives its actual dispatch
//! outcome, under each target module's own rules.
// the wire surface: this module's shared types, flattened at the crate root so
// every `runs::`/`crate::` path reads exactly as it did when they lived here.
// they live in `runs-wire` now — the messages, the records, the action catalog,
// the programs and the id derivations — so a view or the daemon can link the
// format without linking this module.
pub use runs_wire::catalog;
pub use runs_wire::*;

mod model_config;

mod conversations;
// the derived-tier run journal: the PURE decision core (fold + view over
// index_guest::StateRead), compiled everywhere and unit-tested natively.
// the engine shell that runs it inside the module's index database is
// `index_guest` below.
pub mod index;
// the wasm index-mapper shell: wires the pure core into the fluent31 engine.
// compiled only by `guest-builder --index`'s synthesized wasm32 workspace
// (feature `index-guest`), never by the native build.
#[cfg(feature = "index-guest")]
mod index_guest;

// dispatch payload composition: the structured run envelope.
mod envelope;

use sdk::refusal;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use attribution::{Actor, AttributionMsg, ObjectRef, Reason, Relation};
use chat::{
    Block, ChatMsg, ChatQuery, ChatReply, MAX_THREAD_REPLIES, MessageView,
    decode_reply as chat_decode_reply, encode_msg as chat_encode_msg,
    encode_query as chat_encode_query,
};
use dispatch::{
    DispatchMsg, DispatchQuery, DispatchReply, MAX_PAYLOAD_BYTES, OutputContract, ResultEvent,
    Routing, decode_reply as dispatch_decode_reply, encode_msg as dispatch_encode_msg,
    encode_query as dispatch_encode_query,
};
use files::{
    Change as FilesChange, Content as FilesContent, EntryInfo, FilesMsg, FilesQuery, FilesReply,
    decode_reply as files_decode_reply, encode_msg as files_encode_msg,
    encode_query as files_encode_query,
};
use sdk::{Ctx, Error, Event, Module, ModuleId, Msg, Origin, StateRoot, StateSyncHandle};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tasks::{
    JobStatus, JobsEvent, JobsMsg, JobsQuery, JobsReply, decode_job_event as jobs_decode_event,
    decode_job_reply as jobs_decode_reply, encode_job_msg as jobs_encode_msg,
    encode_job_query as jobs_encode_query,
};
use tasks::{
    TaskMsg, TaskQuery, TaskReply, TaskStatus, decode_task_reply as tasks_decode_reply,
    encode_task_msg as tasks_encode_msg, encode_task_query as tasks_encode_query,
};

/// how many transcript messages (newest-first, ending at the anchor) one run
/// embeds into its composed payload — the bounded prompt window (P4).
pub(crate) const CONTEXT_WINDOW: u64 = 64;

/// whole-dispatch deadline granted to a run's LLM work, in views past the
/// dispatching block. leases renew independently; this hard ceiling only
/// bounds a malicious/chatty holder and leaves multi-hour work ample room.
pub const RUN_DEADLINE_VIEWS: u64 = 6 * 60 * 60;

pub use runs_wire::RUN_LEASE_VIEWS;

/// oracle attempts per run: one retry after an explicit provider failure.
pub const RUN_MAX_ATTEMPTS: u32 = 2;

/// Maximum number of live dispatch correlation records.
pub const MAX_PENDING_RUNS: u64 = 4096;

/// Maximum number of distinct delegation edges a root may create over its
/// lifetime. This is separate from the concurrent-pending call limit carried
/// by the runs wire contract.
pub const MAX_DELEGATION_EDGES_PER_RUN: usize = 128;

/// every peer-call callee requests this fixed sandbox profile. One root call
/// tree runs at most `MAX_DELEGATIONS_PER_RUN` callees concurrently, so the
/// same bound holds live delegated compute at `2 * MAX_DELEGATIONS_PER_RUN`
/// cores and `4 * MAX_DELEGATIONS_PER_RUN` GiB. completed calls release
/// their slot.
pub const DELEGATED_CHILD_CORES: u64 = 2;
pub const DELEGATED_CHILD_MEM_GB: u64 = 4;

/// jobs-board claims created by the runs worker use a view-denominated lease.
pub(crate) const JOB_RUN_LEASE_VIEWS: u64 = 1000;

/// jobs finalization payloads must fit the jobs module's 64 KiB cap.
const JOB_FINALIZE_PAYLOAD_BYTES: usize = 64 * 1024;

/// the delivered-runs ring keeps this many terminal runs (newest evicts
/// oldest). derived observability state — never part of `root()`/snapshot.
const RUN_HISTORY_CAP: usize = 100;

/// The wasm host's per-dispatch bound on distinct sibling reads. Runs mirrors
/// it at the module boundary so reference injection degrades before the host
/// rejects the whole dispatch.
pub(crate) const MAX_SIBLING_QUERY_READS: usize = 64;

#[derive(Ord, PartialOrd, Eq, PartialEq)]
enum SiblingRead {
    Root(ModuleId),
    Query(ModuleId, Vec<u8>),
}

#[derive(Default)]
struct SiblingReadBudget {
    reads: RefCell<BTreeSet<SiblingRead>>,
}

impl SiblingReadBudget {
    fn reserve(&self, read: SiblingRead) -> bool {
        let mut reads = self.reads.borrow_mut();
        if reads.contains(&read) {
            return true;
        }
        if reads.len() >= MAX_SIBLING_QUERY_READS {
            return false;
        }
        reads.insert(read);
        true
    }

    fn reserve_root(&self, target: &str) -> bool {
        self.reserve(SiblingRead::Root(target.into()))
    }

    fn reserve_query(&self, target: &str, req: &[u8]) -> bool {
        self.reserve(SiblingRead::Query(target.into(), req.to_vec()))
    }
}

/// Internal pending-state coordinates for Pages sources. The `runs:`
/// chat namespace is reserved to this module, and Runs never mints chat
/// channels below this sub-prefix, so the existing snapshot shape can carry
/// the source discriminator without colliding with a real chat run.
const PAGE_CHANNEL_PREFIX: &str = "runs:pages:";
const PAGE_BLOCK_CHANNEL_PREFIX: &str = "runs:page-block:";

pub(crate) enum PageSource<'a> {
    CommentThread(&'a str),
    Block(&'a str),
}

pub(crate) fn page_channel_id(thread_id: &str) -> String {
    format!("{PAGE_CHANNEL_PREFIX}{thread_id}")
}

pub(crate) fn page_block_channel_id(block_id: &str) -> String {
    format!("{PAGE_BLOCK_CHANNEL_PREFIX}{block_id}")
}

pub(crate) fn page_source(channel_id: &str) -> Option<PageSource<'_>> {
    match channel_id.strip_prefix(PAGE_CHANNEL_PREFIX) {
        Some(thread) => Some(PageSource::CommentThread(thread)),
        None => channel_id
            .strip_prefix(PAGE_BLOCK_CHANNEL_PREFIX)
            .map(PageSource::Block),
    }
}

/// which lane an agent action is being applied from — and therefore how its
/// minted ids are NUMBERED. every id an action mints (a chat message id, a
/// pages thread/comment id) is derived from `(run_id, slot)` and nothing else:
/// no host randomness, no wall clock, so every replaying validator derives
/// byte-identical ids (X2).
///
/// the two lanes must never SHARE a slot: the settle path numbers actions by
/// their index in the delivered response, and the session lane by its committed
/// action counter — both count from 0, so an `s` prefix keeps the session's id
/// space disjoint. without it a mid-run write would squat exactly the id the
/// final response's nth action mints, and that action would silently degrade
/// (pages) on the id it was owed.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Lane {
    /// the settle path: the nth action of the run's delivered response.
    Settle,
    /// A callee's terminal response is returned to its live caller rather than
    /// posted as another answer in the user's chat thread.
    DelegatedSettle,
    /// the session lane: the nth action of the run's open agent session.
    Session(u32),
}

impl Lane {
    /// the id salt of the action at `index` in this lane's action list.
    fn slot(self, index: usize) -> String {
        match self {
            Lane::Settle | Lane::DelegatedSettle => index.to_string(),
            Lane::Session(actions) => format!("s{actions}"),
        }
    }

    /// the catalog lane this path admits operations for: the settle paths
    /// carry the run's final response, the session lane its live actions.
    fn kind(self) -> LaneKind {
        match self {
            Lane::Settle | Lane::DelegatedSettle => LaneKind::Final,
            Lane::Session(_) => LaneKind::Live,
        }
    }

    /// the lane's catalog name, as the strict-lane diagnostics print it.
    fn kind_name(self) -> &'static str {
        match self.kind() {
            LaneKind::Final => "final",
            LaneKind::Live => "live",
        }
    }
}

/// the dispatch-plane recipe an agent's runs execute under — registered
/// owned by runs and registered atomically with its model configuration.
pub(crate) fn recipe_id_for(agent_id: &str) -> String {
    format!("agent/{agent_id}")
}

/// THE char-boundary truncator (one home for what was four hand-rolled
/// loops): `s` within `budget` bytes is returned untouched; otherwise it is
/// cut at the largest char boundary that leaves room for `suffix`, and the
/// suffix is appended — the result never exceeds `budget` bytes.
pub(crate) fn truncate_on_boundary(s: &str, budget: usize, suffix: &str) -> String {
    if s.len() <= budget {
        return s.to_string();
    }
    let mut keep = budget.saturating_sub(suffix.len());
    while keep > 0 && !s.is_char_boundary(keep) {
        keep -= 1;
    }
    format!("{}{suffix}", &s[..keep])
}

mod admin;
mod model_intake;
use model_intake::ModelChange;
mod action_requests;
mod action_storage;
mod deployment;
mod dispatch_flow;
mod engagement;
mod facets;
mod module_updates;
mod receipts;
use facets::WireSink;
// the forge compose lane (M1): forge:<repo>:<n> channel detection, committed
// tracker/refs mirrors, and the item-session workspace/sink composition.
mod forge_source;
// deterministic forge item-context injection (M1): the byte-capped
// instructions section a forge run's envelope carries.
mod inject;
mod jobs_intake;
mod module_impl;
// the pages effects lane (M2): pages.comment / pages.set_checked applied at
// the run boundary — probe-guarded, per-action degrade.
mod pages_effects;
mod response;
// the agent session lane: the mid-run write path — an ephemeral key bound to a
// live run, and the actions it signs, decoded against the SAME catalog the
// settle path decodes.
mod sessions;
// the delivery sink (O1/O2): the forge PR sink applied at the result intake —
// gates, duplicate-PR guard, and message-facet title/body derivation.
mod sink;
mod state;

use response::canonical_origin;
use state::{
    StateVersion, committed_root, contains_run_separator, decode_committed, encode_committed,
    legacy_root, post_a_root, reject_run_separator,
};

/// one in-flight dispatch's correlation entry. the dispatch id is the map
/// key; the run id is derivable from the fields. NOT a lifecycle record: it
/// exists exactly while the dispatch is outstanding and is pruned when the
/// result delivers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingState {
    account: u64,
    generation: u64,
    cause: sdk::Cause,
    /// Explicit because delegated calls have their own idempotency-keyed run
    /// ids rather than pretending to be another chat turn.
    run_id: String,
    agent_id: String,
    /// The root workspace inherited by a generic-chat call tree. Forge runs
    /// already share their item branch, but keeping this explicit makes both
    /// paths agree under nested calls.
    workspace_agent_id: String,
    /// The run-scoped call edge that created this entry.
    delegation_id: Option<String>,
    /// empty for job-backed runs.
    channel_id: String,
    /// 0 for job-backed runs.
    anchor_seq: u64,
    /// the anchor's thread root, if the anchor was a thread reply.
    thread_root: Option<u64>,
    /// the jobs-board item this run owns, when created from a JobsEvent.
    job_id: Option<String>,
    /// the claim height this job-backed run is bound to; chat runs use 0.
    job_claim_height: u64,
    /// the ACCOUNT the run speaks for: the explicit requester, or the author
    /// whose post engaged the agent — never the plane that carried the event.
    /// it is both a cancel capability alongside the owner and the chat standing
    /// an agent's own posts are held to (`requester_may_post`).
    requester: RunOrigin,
    /// the sink COMMITTED at dispatch — the binding delivery enforces (#1835).
    /// an executing node's echoed result sink is compared against this, never
    /// trusted on its own: a mismatch degrades delivery to `Chain`.
    sink: WireSink,
    created_at: u64,
}

impl PendingState {
    fn reply_destination(&self) -> Result<ReplyDestination, String> {
        if let Some(job_id) = &self.job_id {
            return Ok(ReplyDestination::Job {
                job_id: job_id.clone(),
            });
        }
        match page_source(&self.channel_id) {
            Some(PageSource::Block(target)) => Ok(ReplyDestination::Page {
                target: target.into(),
            }),
            Some(PageSource::CommentThread(thread_id)) => Ok(ReplyDestination::PageThread {
                thread_id: thread_id.into(),
            }),
            None => {
                let has_source = !self.channel_id.is_empty();
                if !has_source {
                    return Err("this run has no reply destination".into());
                }
                Ok(ReplyDestination::Chat {
                    channel_id: self.channel_id.clone(),
                    thread: self.reply_thread(),
                })
            }
        }
    }

    fn reply_thread(&self) -> Option<u64> {
        let has_anchor = self.anchor_seq != 0;
        self.thread_root.or(has_anchor.then_some(self.anchor_seq))
    }

    fn run_id(&self) -> String {
        self.run_id.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DelegationState {
    view: DelegationView,
    request: DelegationRequest,
}

/// The point-addressed part of a delegation. The request body is intentionally
/// absent: its digest preserves exact idempotent retry behavior after dispatch
/// consumes the request record, while tree walks need only this header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegationHeader {
    delegation_id: String,
    request_id: String,
    caller_run_id: String,
    root_run_id: String,
    callee_run_id: String,
    callee_agent_id: String,
    status: DelegationStatus,
    request_digest: [u8; 32],
    created_at: u64,
    completed_at: Option<u64>,
}

impl DelegationHeader {
    fn from_state(state: &DelegationState) -> Self {
        Self {
            delegation_id: state.view.delegation_id.clone(),
            request_id: state.view.request_id.clone(),
            caller_run_id: state.view.caller_run_id.clone(),
            root_run_id: state.view.root_run_id.clone(),
            callee_run_id: state.view.callee_run_id.clone(),
            callee_agent_id: state.view.callee_agent_id.clone(),
            status: state.view.status,
            request_digest: Sha256::digest(sdk::wire::encode(&state.request)).into(),
            created_at: state.view.created_at,
            completed_at: state.view.completed_at,
        }
    }

    fn view(&self, result: Option<DelegationResult>) -> DelegationView {
        DelegationView {
            delegation_id: self.delegation_id.clone(),
            request_id: self.request_id.clone(),
            caller_run_id: self.caller_run_id.clone(),
            root_run_id: self.root_run_id.clone(),
            callee_run_id: self.callee_run_id.clone(),
            callee_agent_id: self.callee_agent_id.clone(),
            status: self.status,
            result,
            created_at: self.created_at,
            completed_at: self.completed_at,
        }
    }
}

/// a chat run's read-only dispatch preparation: the pinned context plus the
/// fully composed payload, gathered before anything is staged.
#[derive(Debug)]
struct PreparedDispatch {
    thread_root: Option<u64>,
    payload: Vec<u8>,
    account: u64,
    generation: u64,
    /// the requested sink composed into `payload`'s `result_contract` —
    /// captured here so the caller can commit it into `PendingState` (#1835).
    sink: WireSink,
}

// ---- the module -----------------------------------------------------------

pub struct RunsModule {
    id: ModuleId,
    /// genesis config, not state: which module ids the origin router trusts.
    chat: ModuleId,
    /// dead-letter routing only: a saga callback pointed here by a foreign
    /// trigger's `reply_to` must be swallowed, never abort its block.
    saga: ModuleId,
    /// Source reports and authenticated model workflow triggers.
    attribution: ModuleId,
    /// the dispatch plane — every run's recipe registry, executor, and
    /// lifecycle ledger.
    dispatch: ModuleId,
    /// Executor of the account's programmable workflow.
    agent: ModuleId,
    tasks: Option<ModuleId>,
    jobs: Option<ModuleId>,
    /// the forge module id — the PR/merge sink target (O2). genesis config, NOT
    /// committed state (it never enters `root()`), so it adds no consensus
    /// surface. `None` on nodes not wired for the sink; the sink then degrades
    /// to a breadcrumb.
    forge: Option<ModuleId>,
    /// the duckfs/files module id — queried for the committed head every
    /// envelope pins as `source_snapshot` (W2). genesis config, NOT committed
    /// state (never in `root()`), so it adds no consensus surface. every
    /// production composer wires it; unwired (dev tools/tests) the envelope
    /// still composes v1, with a null pin.
    files: Option<ModuleId>,
    /// the pages module id — queried for canonical `duck://<chain>/pages/<id>` refs so a run's
    /// context can carry referenced page subtrees. genesis config, NOT
    /// committed state (never in `root()`). `None` on nodes not wired for
    /// pages; page refs then compose no page section (a silent skip, never
    /// a failure).
    pages: Option<ModuleId>,
    /// the collaboration module id — the target of the two `collaboration.*`
    /// operations. genesis config, NOT committed state (never in `root()`).
    /// `None` on nodes not wired for it, and then those operations refuse
    /// rather than degrade: an unsent message must never look sent.
    collaboration: Option<ModuleId>,
    /// this network's chain id, from the genesis `__config` record
    /// (`sdk::genesis_config::CHAIN_ID`) — the ONLY way a fixed component learns
    /// which network it is running on. Genesis config, NOT committed state
    /// (never in `root()`). Every `duck://` link this module renders into an
    /// agent's context uses it to mint canonical `duck://` addresses; empty
    /// (dev tools, tests) leaves generated page labels unlinked.
    chain_id: String,
    /// Genesis-bound clock scale; duration scheduling refuses absent wiring.
    time_unit: Option<sdk::genesis_config::TimeUnit>,
    /// Legacy collections retained only between an old-layout install and the
    /// first committed migration. Post-B state lives in `receipts` records.
    legacy_models: Option<BTreeMap<String, ModelRecord>>,
    legacy_pending: Option<BTreeMap<String, PendingState>>,
    legacy_sessions: Option<BTreeMap<String, AgentSession>>,
    legacy_state_version: Option<StateVersion>,
    legacy_migration_staged: bool,
    receipts: receipts::Receipts,
    next_action_item: u64,
    staged_next_action_item: Option<u64>,
    /// Legacy embedded delegation edges retained only until the first mutable
    /// operation migrates them into `dlg/*` receipt records.
    delegations: BTreeMap<String, DelegationState>,
    /// the delivered-runs ring (last [`RUN_HISTORY_CAP`], oldest first —
    /// queries serve it reversed). DERIVED state: recorded at delivery,
    /// rebuilt by replay, never in `root()`/snapshot, empty after a
    /// snapshot join.
    history: VecDeque<RunRecord>,
    /// this block's staged history records — merged into the ring only at
    /// `commit_block` (an aborted block must leave no ghost record).
    pending_history: Vec<RunRecord>,
    /// Verified PR allocations update existing history only at commit.
    pending_pr_links: BTreeMap<String, PrRef>,
    /// Authenticated result-action refusals become visible only at commit.
    pending_action_rejections: BTreeSet<String>,
    /// The receipt facts of every effect prepared in the current execute,
    /// keyed by its message digest, so the proposal staged for that message
    /// records which operation produced it. Transient: never committed state.
    prepared_receipts: RefCell<BTreeMap<[u8; 32], action_requests::ReceiptMeta>>,
    /// the lifecycle facts the current op has committed, stamped onto the op
    /// once it applies ([`RunsModule::stamp_journal`]). Transient: never
    /// committed state, cleared at every op's start.
    journal: Vec<RunEvent>,
}

impl RunsModule {
    /// wire the module to its collaborators. the ids must be pairwise
    /// distinct — origin routing is what makes the privileged intakes
    /// spoof-proof, and colliding ids would collapse those namespaces.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<ModuleId>,
        chat: impl Into<ModuleId>,
        saga: impl Into<ModuleId>,
        attribution: impl Into<ModuleId>,
        dispatch: impl Into<ModuleId>,
        agent: impl Into<ModuleId>,
        tasks: Option<ModuleId>,
        jobs: Option<ModuleId>,
    ) -> Self {
        let id = id.into();
        let chat = chat.into();
        let saga = saga.into();
        let attribution = attribution.into();
        let dispatch = dispatch.into();
        let agent = agent.into();
        let core = BTreeSet::from([
            id.clone(),
            chat.clone(),
            saga.clone(),
            attribution.clone(),
            dispatch.clone(),
            agent.clone(),
        ]);
        assert_eq!(
            core.len(),
            6,
            "runs core collaborator module ids must be pairwise distinct"
        );
        // the task board and the job board now live in ONE merged work module,
        // so `tasks` and `jobs` MAY be the same id -- the invariant that keeps
        // the privileged intakes spoof-proof is only that each is distinct from
        // every core collaborator's origin.
        for module in [&tasks, &jobs].into_iter().flatten() {
            assert!(
                !core.contains(module),
                "a task/job module id collides with a core runs collaborator"
            );
        }
        Self {
            id,
            chat,
            saga,
            attribution,
            dispatch,
            agent,
            tasks,
            jobs,
            forge: None,
            files: None,
            pages: None,
            collaboration: None,
            chain_id: String::new(),
            time_unit: None,
            legacy_models: None,
            legacy_pending: None,
            legacy_sessions: None,
            legacy_state_version: None,
            legacy_migration_staged: false,
            receipts: receipts::Receipts::default(),
            next_action_item: 0,
            staged_next_action_item: None,
            delegations: BTreeMap::new(),
            history: VecDeque::new(),
            pending_history: Vec::new(),
            pending_pr_links: BTreeMap::new(),
            pending_action_rejections: BTreeSet::new(),
            prepared_receipts: RefCell::new(BTreeMap::new()),
            journal: Vec::new(),
        }
    }

    /// commit one lifecycle fact about `run_id` to the current op's journal.
    fn record(&mut self, run_id: &str, fact: RunFact) {
        self.journal.push(RunEvent {
            run_id: run_id.to_string(),
            fact,
        });
    }

    /// the one settle writer: a terminal run enters the delivered-runs ring
    /// and its journal in the same step, so the two can never disagree.
    fn record_settled(&mut self, record: RunRecord, reason: Option<String>) {
        self.record(
            &record.run_id,
            RunFact::Settled {
                outcome: record.outcome,
                reason,
                degraded: record.degraded,
                executing_node: record.executing_node.clone(),
                output_ref: record.output_ref.clone(),
                pr: record.pr.clone(),
            },
        );
        self.pending_history.push(record);
    }

    /// stamp the facts the applying op committed onto its trace, as the
    /// assigned stamp the derived tier folds. an op that moved no run
    /// stamps nothing.
    fn stamp_journal(&mut self, ctx: &mut dyn Ctx) {
        if self.journal.is_empty() {
            return;
        }
        ctx.set_assigned(encode_assigned(&std::mem::take(&mut self.journal)));
    }

    /// Emit one prepared effect and remember its receipt facts for the
    /// proposal that will be staged for its exact message.
    fn emit_prepared(&self, ctx: &mut dyn Ctx, prepared: action_requests::Prepared) {
        self.prepared_receipts.borrow_mut().insert(
            action_requests::message_digest(&prepared.message),
            prepared.receipt,
        );
        ctx.emit_msg(prepared.message);
    }

    /// The receipt facts recorded for `message`, or an effect label naming its
    /// target for a message no preparer annotated.
    fn take_prepared_receipt(&self, message: &Msg) -> action_requests::ReceiptMeta {
        self.prepared_receipts
            .borrow_mut()
            .remove(&action_requests::message_digest(message))
            .unwrap_or_else(|| action_requests::ReceiptMeta::effect(message.target.clone()))
    }

    /// wire the forge module as the PR/merge sink target (O2), after
    /// construction — mirrors the injected `Option<ModuleId>` collaborators so
    /// `new` and every existing call site stay untouched. without this wired
    /// the PR sink degrades to a breadcrumb.
    pub fn with_sink_forge(mut self, forge: impl Into<ModuleId>) -> Self {
        let forge = forge.into();
        assert!(
            forge != self.id
                && forge != self.chat
                && forge != self.saga
                && forge != self.attribution
                && forge != self.dispatch
                && forge != self.agent
                && Some(&forge) != self.tasks.as_ref()
                && Some(&forge) != self.jobs.as_ref(),
            "forge sink id must be distinct from every other collaborator"
        );
        self.forge = Some(forge);
        self
    }

    /// wire the duckfs/files module so the envelope pins the committed head
    /// as `source_snapshot` (W2), after construction — mirrors the injected
    /// `Option<ModuleId>` collaborators so `new` and every existing call site
    /// stay untouched. every production composer wires it; unwired, the
    /// envelope composes with a null pin.
    pub fn with_files_module(mut self, files: impl Into<ModuleId>) -> Self {
        let files = files.into();
        assert!(
            files != self.id,
            "files module id must be distinct from the runs module id"
        );
        self.files = Some(files);
        self
    }

    /// wire the pages module so canonical `duck://<chain>/pages/<id>` refs in a run's trigger
    /// message or injected item body render referenced page subtrees into the
    /// composed context, after construction — mirrors the injected
    /// `Option<ModuleId>` collaborators so `new` and every existing call site
    /// stay untouched. unwired, page refs compose no page section.
    pub fn with_pages_module(mut self, pages: impl Into<ModuleId>) -> Self {
        let pages = pages.into();
        assert!(
            pages != self.id,
            "pages module id must be distinct from the runs module id"
        );
        self.pages = Some(pages);
        self
    }

    /// wire the collaboration module so the `collaboration.*` operations have a
    /// target, after construction — mirrors the injected `Option<ModuleId>`
    /// collaborators so `new` and every existing call site stay untouched.
    /// unwired, those operations are refused by name.
    pub fn with_collaboration_module(mut self, collaboration: impl Into<ModuleId>) -> Self {
        let collaboration = collaboration.into();
        assert!(
            collaboration != self.id,
            "collaboration module id must be distinct from the runs module id"
        );
        self.collaboration = Some(collaboration);
        self
    }

    /// wire this network's chain id, after construction — mirrors the injected
    /// collaborators so `new` and every existing call site stay untouched. the
    /// guest reads it out of the genesis `__config` record; unwired, produced
    /// page labels remain unlinked.
    pub fn with_time_unit(mut self, unit: sdk::genesis_config::TimeUnit) -> Self {
        self.time_unit = Some(unit);
        self
    }

    pub fn with_chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = chain_id.into();
        self
    }

    /// the genesis chain id used by canonical `duck://` address helpers.
    pub(crate) fn address_chain_id(&self) -> &str {
        &self.chain_id
    }

    /// Use the SDK's one chain-id grammar for both genesis (`<label>#<salt>`)
    /// and address-authority (`<label>-<salt>`) spellings.
    pub(crate) fn parse_address_chain_id(
        chain_id: &str,
    ) -> Result<duck_address::ChainId, duck_address::Refused> {
        chain_id.parse()
    }

    // ---- staged-over-committed reads ---------------------------------------

    async fn pending_entry(&self, dispatch_id: &str) -> Result<Option<PendingState>, Error> {
        if let Some(legacy) = self.legacy_pending.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy.get(dispatch_id).cloned());
        }
        let Some(bytes) = self.receipts.get(&state::pending_key(dispatch_id)).await? else {
            return Ok(None);
        };
        let (pending, _, _) =
            state::decode_pending_record(dispatch_id, &bytes).map_err(Self::corrupt_record)?;
        Ok(Some(pending))
    }

    async fn session(&self, run_id: &str) -> Result<Option<AgentSession>, Error> {
        if let Some(legacy) = self.legacy_sessions.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy.get(run_id).cloned());
        }
        let Some(pending) = self.pending_entry(&dispatch_id_for(run_id)).await? else {
            return Ok(None);
        };
        self.session_for_pending(run_id, &pending).await
    }

    async fn session_for_pending(
        &self,
        run_id: &str,
        pending: &PendingState,
    ) -> Result<Option<AgentSession>, Error> {
        if let Some(legacy) = self.legacy_sessions.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy.get(run_id).cloned());
        }
        let Some(bytes) = self.receipts.get(&state::session_key(run_id)).await? else {
            return Ok(None);
        };
        state::decode_session_record(run_id, &bytes, pending)
            .map(Some)
            .map_err(Self::corrupt_record)
    }

    async fn pending_list(&self) -> Result<Vec<(String, PendingState)>, Error> {
        if let Some(legacy) = self.legacy_pending.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy
                .iter()
                .map(|(id, pending)| (id.clone(), pending.clone()))
                .collect());
        }
        let Some(bytes) = self.receipts.get(state::RUN_META_KEY).await? else {
            return Ok(Vec::new());
        };
        let (head, count) = state::decode_pending_meta(&bytes).map_err(Self::corrupt_record)?;
        let Some(mut current) = head else {
            if count == 0 {
                return Ok(Vec::new());
            }
            return Err(Self::corrupt_record("pending metadata has no head"));
        };
        let mut result = Vec::with_capacity(count as usize);
        let mut previous = None;
        let mut seen = BTreeSet::new();
        for _ in 0..count {
            if !seen.insert(current.clone()) {
                return Err(Self::corrupt_record("pending list contains a cycle"));
            }
            let Some(bytes) = self.receipts.get(&state::pending_key(&current)).await? else {
                return Err(Self::corrupt_record("pending list names a missing record"));
            };
            let (pending, prev, next) =
                state::decode_pending_record(&current, &bytes).map_err(Self::corrupt_record)?;
            if prev != previous {
                return Err(Self::corrupt_record(
                    "pending list has a broken previous link",
                ));
            }
            previous = Some(current.clone());
            result.push((current.clone(), pending));
            let Some(next) = next else {
                if result.len() != count as usize {
                    return Err(Self::corrupt_record("pending list ended before its count"));
                }
                return Ok(result);
            };
            current = next;
        }
        Err(Self::corrupt_record("pending list exceeds its count"))
    }

    async fn session_list(&self) -> Result<Vec<AgentSession>, Error> {
        if let Some(legacy) = self.legacy_sessions.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy.values().cloned().collect());
        }
        let pending = self.pending_list().await?;
        let mut result = Vec::new();
        for (_, entry) in pending {
            if let Some(session) = self.session_for_pending(&entry.run_id, &entry).await? {
                result.push(session);
            }
        }
        result.sort_by(|left, right| left.run_id.cmp(&right.run_id));
        Ok(result)
    }

    fn legacy_delegations_active(&self) -> bool {
        self.legacy_state_version.is_some() && !self.legacy_migration_staged
    }

    async fn delegation_header(
        &self,
        delegation_id: &str,
    ) -> Result<Option<DelegationHeader>, Error> {
        if self.legacy_delegations_active() {
            return Ok(self
                .delegations
                .get(delegation_id)
                .map(DelegationHeader::from_state));
        }
        let Some(bytes) = self
            .receipts
            .get(&state::delegation_key(delegation_id))
            .await?
        else {
            return Ok(None);
        };
        state::decode_delegation_header(delegation_id, &bytes)
            .map(Some)
            .map_err(Self::corrupt_record)
    }

    async fn delegation_tree(
        &self,
        root_run_id: &str,
    ) -> Result<Option<state::DelegationTree>, Error> {
        if self.legacy_delegations_active() {
            let mut ids = self
                .delegations
                .iter()
                .filter(|(_, delegation)| delegation.view.root_run_id == root_run_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.sort();
            if ids.is_empty() {
                return Ok(None);
            }
            let pending = ids
                .iter()
                .filter(|id| {
                    self.delegations.get(*id).is_some_and(|delegation| {
                        delegation.view.status == DelegationStatus::Pending
                    })
                })
                .count() as u64;
            return Ok(Some(state::DelegationTree { ids, pending }));
        }
        let Some(bytes) = self
            .receipts
            .get(&state::delegation_tree_key(root_run_id))
            .await?
        else {
            return Ok(None);
        };
        state::decode_delegation_tree(root_run_id, &bytes)
            .map(Some)
            .map_err(Self::corrupt_record)
    }

    async fn delegation_run_index(
        &self,
        run_id: &str,
    ) -> Result<Option<state::DelegationRunIndex>, Error> {
        let Some(bytes) = self
            .receipts
            .get(&state::delegation_run_key(run_id))
            .await?
        else {
            return Ok(None);
        };
        state::decode_delegation_run_index(&dispatch_id_for(run_id), &bytes)
            .map(Some)
            .map_err(Self::corrupt_record)
    }

    async fn delegation_view(&self, header: &DelegationHeader) -> Result<DelegationView, Error> {
        if self.legacy_delegations_active() {
            return self
                .delegations
                .get(&header.delegation_id)
                .map(|state| state.view.clone())
                .ok_or_else(|| Self::corrupt_record("legacy delegation header disappeared"));
        }
        let result = if header.status == DelegationStatus::Pending {
            None
        } else {
            let bytes = self
                .receipts
                .get(&state::delegation_reply_key(&header.delegation_id))
                .await?
                .ok_or_else(|| Self::corrupt_record("terminal delegation has no reply"))?;
            Some(
                state::decode_delegation_result(&header.delegation_id, &bytes)
                    .map_err(Self::corrupt_record)?,
            )
        };
        Ok(header.view(result))
    }

    async fn delegations_for_caller(
        &self,
        caller_run_id: &str,
    ) -> Result<Vec<DelegationView>, Error> {
        if self.legacy_delegations_active() {
            return Ok(self
                .delegations
                .values()
                .filter(|state| state.view.caller_run_id == caller_run_id)
                .map(|state| state.view.clone())
                .collect());
        }
        let (root_run_id, locator) =
            if let Some(caller) = self.pending_entry(&dispatch_id_for(caller_run_id)).await? {
                match caller.delegation_id.as_deref() {
                    Some(id) => (
                        self.delegation_header(id)
                            .await?
                            .ok_or_else(|| {
                                Self::corrupt_record("caller run names no delegation header")
                            })?
                            .root_run_id,
                        None,
                    ),
                    None => (caller_run_id.to_string(), None),
                }
            } else {
                let Some(index) = self.delegation_run_index(caller_run_id).await? else {
                    return Ok(Vec::new());
                };
                (index.root_run_id, Some(index.delegation_id))
            };
        let Some(tree) = self.delegation_tree(&root_run_id).await? else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        let mut locator_found = locator.is_none();
        for id in tree.ids {
            let Some(header) = self.delegation_header(&id).await? else {
                return Err(Self::corrupt_record("delegation tree names no header"));
            };
            if locator.as_deref() == Some(id.as_str()) {
                if header.callee_run_id != caller_run_id {
                    return Err(Self::corrupt_record(
                        "delegation run index does not match its header",
                    ));
                }
                locator_found = true;
            }
            if header.caller_run_id == caller_run_id {
                result.push(self.delegation_view(&header).await?);
            }
        }
        if !locator_found {
            return Err(Self::corrupt_record(
                "delegation run index names no tree header",
            ));
        }
        Ok(result)
    }

    fn stage_delegation_header(&mut self, header: &DelegationHeader) -> Result<(), Error> {
        self.receipts.stage(
            state::delegation_key(&header.delegation_id),
            state::encode_delegation_header(header),
        )
    }

    fn stage_delegation_result(
        &mut self,
        delegation_id: &str,
        result: &DelegationResult,
    ) -> Result<(), Error> {
        self.receipts.stage(
            state::delegation_reply_key(delegation_id),
            state::encode_delegation_result(delegation_id, result)?,
        )
    }

    fn stage_delegation_tree(
        &mut self,
        root_run_id: &str,
        tree: &state::DelegationTree,
    ) -> Result<(), Error> {
        self.receipts.stage(
            state::delegation_tree_key(root_run_id),
            state::encode_delegation_tree(root_run_id, tree),
        )
    }

    async fn stage_delegation(&mut self, state: &DelegationState) -> Result<(), Error> {
        let header = DelegationHeader::from_state(state);
        state::validate_delegation_header(&header).map_err(Self::corrupt_record)?;
        let mut tree = self
            .delegation_tree(&header.root_run_id)
            .await?
            .unwrap_or_default();
        match tree.ids.binary_search(&header.delegation_id) {
            Ok(_) => {
                return Err(Self::corrupt_record(
                    "delegation tree already contains edge",
                ));
            }
            Err(index) => tree.ids.insert(index, header.delegation_id.clone()),
        }
        if tree.ids.len() > MAX_DELEGATION_EDGES_PER_RUN {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!(
                    "delegation tree has reached its lifetime limit of {MAX_DELEGATION_EDGES_PER_RUN} edges"
                ),
            });
        }
        if header.status == DelegationStatus::Pending {
            tree.pending = tree
                .pending
                .checked_add(1)
                .ok_or_else(|| Self::corrupt_record("delegation tree pending count overflowed"))?;
        }
        self.stage_delegation_tree(&header.root_run_id, &tree)?;
        self.stage_delegation_header(&header)?;
        self.receipts.stage(
            state::delegation_run_key(&header.callee_run_id),
            state::encode_delegation_run_index(&state::DelegationRunIndex {
                run_id: header.callee_run_id.clone(),
                delegation_id: header.delegation_id.clone(),
                root_run_id: header.root_run_id.clone(),
            }),
        )?;
        self.receipts.stage(
            state::delegation_request_key(&header.delegation_id),
            state::encode_delegation_request(&state.request)?,
        )?;
        Ok(())
    }

    fn remove_delegation_records(&mut self, delegation_id: &str, callee_run_id: &str) {
        self.receipts
            .remove(state::delegation_run_key(callee_run_id));
        self.receipts.remove(state::delegation_key(delegation_id));
        self.receipts
            .remove(state::delegation_request_key(delegation_id));
        self.receipts
            .remove(state::delegation_reply_key(delegation_id));
    }

    fn stage_legacy_pending_sessions(&mut self) -> Result<(), Error> {
        let Some(pending) = self.legacy_pending.as_ref() else {
            return Ok(());
        };
        let ids: Vec<String> = pending.keys().cloned().collect();
        let count = ids.len() as u64;
        self.receipts.stage(
            state::RUN_META_KEY.into(),
            state::encode_pending_meta(ids.first().map(String::as_str), count),
        )?;
        for (index, dispatch_id) in ids.iter().enumerate() {
            let prev = (index > 0).then(|| ids[index - 1].as_str());
            let next = ids.get(index + 1).map(String::as_str);
            let entry = pending.get(dispatch_id).ok_or_else(|| {
                Self::corrupt_record("legacy pending map changed during migration")
            })?;
            self.receipts.stage(
                state::pending_key(dispatch_id),
                state::encode_pending_record(entry, prev, next),
            )?;
        }
        if let Some(sessions) = self.legacy_sessions.as_ref() {
            for session in sessions.values() {
                self.receipts.stage(
                    state::session_key(&session.run_id),
                    state::encode_session_record(session),
                )?;
            }
        }
        Ok(())
    }

    fn stage_legacy_delegations(&mut self) -> Result<(), Error> {
        let mut trees = BTreeMap::<String, state::DelegationTree>::new();
        let delegations = self.delegations.clone();
        for (id, delegation) in &delegations {
            let header = DelegationHeader::from_state(delegation);
            state::validate_delegation_header(&header).map_err(Self::corrupt_record)?;
            let tree = trees.entry(header.root_run_id.clone()).or_default();
            let index = match tree.ids.binary_search(id) {
                Ok(_) => {
                    return Err(Self::corrupt_record(
                        "legacy delegation tree contains a duplicate",
                    ));
                }
                Err(index) => index,
            };
            tree.ids.insert(index, id.clone());
            if header.status == DelegationStatus::Pending {
                tree.pending += 1;
            }
            self.stage_delegation_header(&header)?;
            self.receipts.stage(
                state::delegation_run_key(&header.callee_run_id),
                state::encode_delegation_run_index(&state::DelegationRunIndex {
                    run_id: header.callee_run_id.clone(),
                    delegation_id: header.delegation_id.clone(),
                    root_run_id: header.root_run_id.clone(),
                }),
            )?;
            self.receipts.stage(
                state::delegation_request_key(id),
                state::encode_delegation_request(&delegation.request)?,
            )?;
            // Request bodies are needed only while the callee dispatch is
            // being admitted. All legacy edges are already admitted.
            self.receipts.remove(state::delegation_request_key(id));
            if let Some(result) = &delegation.view.result {
                self.stage_delegation_result(id, result)?;
            }
        }
        for (root, tree) in trees {
            if tree.ids.len() > state::MAX_LEGACY_DELEGATION_EDGES_PER_RUN {
                return Err(Error::Module {
                    reason: refusal::CAPACITY.into(),
                    sentence:
                        "legacy delegation tree exceeds its historical compatibility capacity"
                            .into(),
                });
            }
            self.stage_delegation_tree(&root, &tree)?;
        }
        Ok(())
    }

    fn stage_legacy_state(&mut self) -> Result<(), Error> {
        if self.legacy_state_version.is_none() || self.legacy_migration_staged {
            return Ok(());
        }
        self.stage_legacy_models()?;
        self.stage_legacy_pending_sessions()?;
        self.stage_legacy_delegations()?;
        self.legacy_migration_staged = true;
        Ok(())
    }

    async fn stage_pending_insert(
        &mut self,
        dispatch_id: String,
        pending: PendingState,
    ) -> Result<(), Error> {
        state::validate_decoded_pending(&dispatch_id, &pending).map_err(Self::corrupt_record)?;
        if self
            .receipts
            .get(&state::pending_key(&dispatch_id))
            .await?
            .is_some()
        {
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: format!("pending dispatch already exists: {dispatch_id}"),
            });
        }
        let (head, count) = match self.receipts.get(state::RUN_META_KEY).await? {
            Some(bytes) => state::decode_pending_meta(&bytes).map_err(Self::corrupt_record)?,
            None => (None, 0),
        };
        if count > 0 && head.is_none() {
            return Err(Self::corrupt_record("non-empty pending list has no head"));
        }
        if count >= MAX_PENDING_RUNS {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!("the live pending-run cap is {MAX_PENDING_RUNS}"),
            });
        }
        if count == 0 && head.is_some() {
            return Err(Self::corrupt_record("empty pending list has a head"));
        }
        if let Some(old_head) = &head {
            let bytes = self
                .receipts
                .get(&state::pending_key(old_head))
                .await?
                .ok_or_else(|| Self::corrupt_record("pending metadata names a missing head"))?;
            let (old, prev, next) =
                state::decode_pending_record(old_head, &bytes).map_err(Self::corrupt_record)?;
            if prev.is_some() {
                return Err(Self::corrupt_record("pending head has a previous link"));
            }
            self.receipts.stage(
                state::pending_key(old_head),
                state::encode_pending_record(&old, Some(&dispatch_id), next.as_deref()),
            )?;
        }
        self.receipts.stage(
            state::pending_key(&dispatch_id),
            state::encode_pending_record(&pending, None, head.as_deref()),
        )?;
        self.receipts.stage(
            state::RUN_META_KEY.into(),
            state::encode_pending_meta(Some(&dispatch_id), count + 1),
        )?;
        Ok(())
    }

    async fn stage_pending_remove(&mut self, dispatch_id: &str) -> Result<(), Error> {
        let bytes = self
            .receipts
            .get(&state::pending_key(dispatch_id))
            .await?
            .ok_or_else(|| Self::corrupt_record("pending removal names no record"))?;
        let (_, prev, next) =
            state::decode_pending_record(dispatch_id, &bytes).map_err(Self::corrupt_record)?;
        let (head, count) = self
            .receipts
            .get(state::RUN_META_KEY)
            .await?
            .ok_or_else(|| Self::corrupt_record("pending removal has no metadata"))
            .and_then(|bytes| state::decode_pending_meta(&bytes).map_err(Self::corrupt_record))?;
        let Some(head_id) = head.as_deref() else {
            return Err(Self::corrupt_record("non-empty pending list has no head"));
        };
        if count == 0
            || (prev.is_none() && head_id != dispatch_id)
            || (prev.is_some() && head_id == dispatch_id)
        {
            return Err(Self::corrupt_record(
                "pending removal disagrees with metadata",
            ));
        }
        if head_id != dispatch_id {
            let bytes = self
                .receipts
                .get(&state::pending_key(head_id))
                .await?
                .ok_or_else(|| Self::corrupt_record("pending metadata names a missing head"))?;
            let (_, head_prev, _) =
                state::decode_pending_record(head_id, &bytes).map_err(Self::corrupt_record)?;
            if head_prev.is_some() {
                return Err(Self::corrupt_record("pending head has a previous link"));
            }
        }
        if let Some(prev_id) = &prev {
            let bytes = self
                .receipts
                .get(&state::pending_key(prev_id))
                .await?
                .ok_or_else(|| Self::corrupt_record("pending previous neighbor is missing"))?;
            let (entry, neighbor_prev, neighbor_next) =
                state::decode_pending_record(prev_id, &bytes).map_err(Self::corrupt_record)?;
            if neighbor_next.as_deref() != Some(dispatch_id) {
                return Err(Self::corrupt_record("pending previous neighbor is broken"));
            }
            self.receipts.stage(
                state::pending_key(prev_id),
                state::encode_pending_record(&entry, neighbor_prev.as_deref(), next.as_deref()),
            )?;
        }
        if let Some(next_id) = &next {
            let bytes = self
                .receipts
                .get(&state::pending_key(next_id))
                .await?
                .ok_or_else(|| Self::corrupt_record("pending next neighbor is missing"))?;
            let (entry, neighbor_prev, neighbor_next) =
                state::decode_pending_record(next_id, &bytes).map_err(Self::corrupt_record)?;
            if neighbor_prev.as_deref() != Some(dispatch_id) {
                return Err(Self::corrupt_record("pending next neighbor is broken"));
            }
            self.receipts.stage(
                state::pending_key(next_id),
                state::encode_pending_record(&entry, prev.as_deref(), neighbor_next.as_deref()),
            )?;
        }
        self.receipts.remove(state::pending_key(dispatch_id));
        let new_head = if prev.is_none() {
            next.as_deref()
        } else {
            Some(head_id)
        };
        self.receipts.stage(
            state::RUN_META_KEY.into(),
            state::encode_pending_meta(new_head, count - 1),
        )?;
        Ok(())
    }

    #[cfg(test)]
    async fn stage_pending_update(
        &mut self,
        dispatch_id: &str,
        pending: PendingState,
    ) -> Result<(), Error> {
        state::validate_decoded_pending(dispatch_id, &pending).map_err(Self::corrupt_record)?;
        let bytes = self
            .receipts
            .get(&state::pending_key(dispatch_id))
            .await?
            .ok_or_else(|| Self::corrupt_record("pending update names no record"))?;
        let (_, prev, next) =
            state::decode_pending_record(dispatch_id, &bytes).map_err(Self::corrupt_record)?;
        self.receipts.stage(
            state::pending_key(dispatch_id),
            state::encode_pending_record(&pending, prev.as_deref(), next.as_deref()),
        )
    }

    fn stage_session(&mut self, session: AgentSession) -> Result<(), Error> {
        self.receipts.stage(
            state::session_key(&session.run_id),
            state::encode_session_record(&session),
        )
    }

    fn remove_session(&mut self, run_id: &str) {
        self.receipts.remove(state::session_key(run_id));
    }

    // ---- views ---------------------------------------------------------------

    fn pending_view(dispatch_id: &str, p: &PendingState) -> PendingRun {
        PendingRun {
            run_id: p.run_id(),
            dispatch_id: dispatch_id.to_string(),
            agent_id: p.agent_id.clone(),
            channel_id: p.channel_id.clone(),
            anchor_seq: p.anchor_seq,
            thread_root: p.thread_root,
            job_id: p.job_id.clone(),
            job_claim_height: p.job_claim_height,
            requester: p.requester.clone(),
            created_at: p.created_at,
        }
    }

    // ---- shared validation ----------------------------------------------------

    fn validate_non_empty(field: &str, value: &str) -> Result<(), Error> {
        if value.is_empty() {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!("{field} must not be empty"),
            });
        }
        Ok(())
    }

    /// admin ops take a non-empty external key or a module as the submitter.
    /// the pre-consensus empty external default and the system origin (which
    /// any genesis path could wear) cannot administer model work.
    fn admin_origin(origin: &Origin) -> Result<RunOrigin, Error> {
        match origin {
            Origin::External(key) if key.is_empty() => Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: "runs admin ops require a non-empty submitter id".into(),
            }),
            Origin::System => Err(Error::Module {
                reason: refusal::UNAUTHORIZED.into(),
                sentence: "runs admin ops require an external or module origin".into(),
            }),
            other => canonical_origin(other),
        }
    }

    /// an observability breadcrumb for the no-fail arms: dropped payloads,
    /// skipped engagements, and failed runs leave the state machine as
    /// events, never as errors.
    fn note(&self, ctx: &mut dyn Ctx, what: String) {
        ctx.emit_event(Event {
            source: self.id.clone(),
            payload: what.into_bytes(),
        });
    }
    // ---- state-sync ---------------------------------------------------------
    // hand a joiner the committed continuation state as canonical bytes; the
    // consensus-agreed root — never the serving peer — decides whether they land.

    /// serialize the COMMITTED continuation state (never the staged overlay)
    /// into the canonical encoding `root()` commits to. deterministic across
    /// nodes.
    #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
    fn with_receipt_store(mut self, store: Box<dyn sdk::MerkleStore>) -> Self {
        self.receipts = receipts::Receipts::hosted(store);
        self
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let records = self.receipts.snapshot();
        match self.legacy_state_version {
            Some(StateVersion::V0) => state::encode_legacy_committed(
                &records,
                self.next_action_item,
                self.legacy_pending.as_ref().unwrap(),
                self.legacy_sessions.as_ref().unwrap(),
                &self.delegations,
                self.legacy_models.as_ref().unwrap(),
            ),
            Some(StateVersion::V1) => state::encode_post_a_committed(
                &records,
                self.next_action_item,
                self.legacy_pending.as_ref().unwrap(),
                self.legacy_sessions.as_ref().unwrap(),
                &self.delegations,
            ),
            Some(StateVersion::V2) => {
                state::encode_post_b_committed(&records, self.next_action_item, &self.delegations)
            }
            Some(StateVersion::V3) | None => {
                encode_committed(&records, self.next_action_item, &self.delegations)
            }
        }
    }

    /// adopt a peer's snapshot as own committed state — but only after the
    /// decoded temporaries re-derive `expected` via the exact `root()`
    /// algorithm, so a byzantine snapshot cannot land under an agreed root it
    /// doesn't match. all-or-nothing: on any Err this module (and its root)
    /// is byte-identical to before the call. on success the staged overlay is
    /// dropped — a snapshot describes a block boundary, and nothing
    /// half-applied may shadow it.
    pub fn install(&mut self, bytes: &[u8], expected: StateRoot) -> Result<(), Error> {
        let decoded = decode_committed(bytes).map_err(|sentence| Error::Module {
            reason: refusal::CORRUPT.into(),
            sentence,
        })?;
        sdk::verify_snapshot_root(
            match decoded.version {
                StateVersion::V0 => legacy_root(
                    &decoded.receipts,
                    decoded.next_action_item,
                    &decoded.pending,
                    &decoded.sessions,
                    &decoded.delegations,
                    decoded.legacy_models.as_ref().unwrap(),
                ),
                StateVersion::V1 => post_a_root(
                    &decoded.receipts,
                    decoded.next_action_item,
                    &decoded.pending,
                    &decoded.sessions,
                    &decoded.delegations,
                ),
                StateVersion::V2 => state::post_b_root(
                    &decoded.receipts,
                    decoded.next_action_item,
                    &decoded.delegations,
                ),
                StateVersion::V3 => committed_root(
                    &decoded.receipts,
                    decoded.next_action_item,
                    &decoded.delegations,
                ),
            },
            expected,
        )?;
        self.receipts.install(decoded.receipts)?;
        self.legacy_models = decoded.legacy_models;
        self.legacy_pending = matches!(decoded.version, StateVersion::V0 | StateVersion::V1)
            .then_some(decoded.pending);
        self.legacy_sessions = matches!(decoded.version, StateVersion::V0 | StateVersion::V1)
            .then_some(decoded.sessions);
        self.legacy_state_version =
            (decoded.version != StateVersion::V3).then_some(decoded.version);
        self.legacy_migration_staged = false;
        self.next_action_item = decoded.next_action_item;
        self.staged_next_action_item = None;
        self.delegations = decoded.delegations;
        // the ring is derived per-node state: a snapshot describes a block
        // boundary this node never executed, so its history starts empty.
        self.history.clear();
        self.pending_history.clear();
        self.pending_pr_links.clear();
        self.pending_action_rejections.clear();
        Ok(())
    }

    // ---- the delivered-runs ring, as a portable value ------------------------
    // the ring is DERIVED state — deliberately outside `root()`/`snapshot()`
    // (a native snapshot join starts empty; replay rebuilds it). the WASM PORT
    // has no per-node memory to rebuild into: the guest is re-instantiated per
    // dispatch, so anything not persisted through the host store is lost, and
    // real consumers (the app's runs client, the dogfood receipt lane) read
    // `RunsQuery::RecentRuns`. these two methods are that port's lane: the
    // guest persists the COMMITTED ring as its own host-KV value beside the
    // canonical snapshot. every `RunRecord` field is already a deterministic
    // consensus derivation (the executing-node attribution feeds PR-body
    // breadcrumbs — committed forge state — today), so the ring riding the
    // wasm module's host-KV root is consensus-safe. NATIVE lifecycles never
    // call these.

    /// the committed delivered-runs ring (never the staged records), oldest
    /// first — the exact in-memory order `commit_block` maintains.
    pub fn history_snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&self.history).expect("run records serialize")
    }

    /// adopt a persisted ring (UNTRUSTED input: never panics on malformed
    /// bytes, rejects a ring past the cap no honest writer produces). staged
    /// records are dropped — like [`RunsModule::install`], a persisted ring
    /// describes a dispatch boundary and nothing half-applied may shadow it.
    pub fn install_history(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let history: VecDeque<RunRecord> =
            serde_json::from_slice(bytes).map_err(|e| Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence: format!("run history decode: {e}"),
            })?;
        if history.len() > RUN_HISTORY_CAP {
            return Err(Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence: format!(
                    "run history carries {} records; the cap is {RUN_HISTORY_CAP}",
                    history.len()
                ),
            });
        }
        self.history = history;
        self.pending_history.clear();
        self.pending_pr_links.clear();
        self.pending_action_rejections.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

// the wasm-guest port: the dispatch shell that adapts this module to the
// ducktape:module world. compiled only by the guest-builder's synthesized
// wasm32 cdylib workspace (feature `guest`), never by the native build.
#[cfg(feature = "guest")]
mod guest;
