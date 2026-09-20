use super::{
    AgentSession, BTreeMap, DelegationHeader, DelegationRequest, DelegationResult, DelegationState,
    DelegationStatus, Digest, Error, MAX_ACTIONS_PER_SESSION, MAX_DELEGATION_INSTRUCTION_BYTES,
    MAX_DELEGATIONS_BYTES, MAX_DELEGATIONS_PER_RUN, MAX_REPLY_BLOCKS_BYTES, MAX_REQUEST_ID_BYTES,
    PendingState, RUN_KEY_SEPARATOR, RunOrigin, SESSION_KEY_LEN, Sha256, StateRoot, WireSink,
    delegated_run_id_for, delegation_id_for, dispatch_id_for,
};
use sdk::codec;
use sdk::refusal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// ---- canonical encoding -------------------------------------------------------
// u64-le counts, sorted keys, every field in declaration order: u64-le length
// prefixes for byte strings, single-byte discriminants for enums, a 0/1 tag
// byte for options, u64-le integers (via the shared `sdk::codec` writers). this
// is the exact preimage [`Module::root`] hashes, so a snapshot and the root that
// must authenticate it cannot drift. The post-A shape starts with an impossible
// old length followed by a version; v0 starts directly with the receipts field.

const POST_A_MAGIC: u64 = u64::MAX - 1;
const POST_A_VERSION: u8 = 1;
const POST_B_VERSION: u8 = 2;
const POST_C_VERSION: u8 = 3;

// The old embedded map encoded each entry as an 8-byte length, a 64-byte
// delegation id, another 8-byte JSON length, and DelegationState JSON. Exact
// Task B validation permits every status with omitted optional result and
// completion fields, so use the simple 256-byte floor below rather than a
// producer-specific terminal minimum. The all-status minimum is 263 bytes;
// 256 is deliberately looser and leaves the read proof below 4096. This is a
// size bound only, not a new historical validation rule.
const LEGACY_JSON_MIN_BYTES: usize = 256;
const LEGACY_ENTRY_MIN_BYTES: usize = 8 + 64 + 8 + LEGACY_JSON_MIN_BYTES;
pub(super) const MAX_LEGACY_DELEGATION_EDGES_PER_RUN: usize =
    sdk::MAX_STORE_VALUE_BYTES / LEGACY_ENTRY_MIN_BYTES;

pub(super) const RUN_META_KEY: &str = "run/meta";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingRecord {
    pending: PendingState,
    prev: Option<String>,
    next: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingMeta {
    head: Option<String>,
    count: u64,
}

pub(super) fn pending_key(dispatch_id: &str) -> String {
    format!("run/{dispatch_id}")
}

pub(super) fn session_key(run_id: &str) -> String {
    format!("session/{run_id}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DelegationTree {
    pub(super) ids: Vec<String>,
    pub(super) pending: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DelegationRunIndex {
    pub(super) run_id: String,
    pub(super) delegation_id: String,
    pub(super) root_run_id: String,
}

pub(super) fn delegation_tree_key(root_run_id: &str) -> String {
    format!("dlg/tree/{root_run_id}")
}

pub(super) fn delegation_key(delegation_id: &str) -> String {
    format!("dlg/{delegation_id}")
}

pub(super) fn delegation_run_key(run_id: &str) -> String {
    format!("dlg/run/{}", dispatch_id_for(run_id))
}

pub(super) fn delegation_request_key(delegation_id: &str) -> String {
    format!("dlg/req/{delegation_id}")
}

pub(super) fn delegation_reply_key(delegation_id: &str) -> String {
    format!("dlg/reply/{delegation_id}")
}

pub(super) fn encode_delegation_tree(root_run_id: &str, tree: &DelegationTree) -> Vec<u8> {
    debug_assert!(tree.ids.len() <= MAX_LEGACY_DELEGATION_EDGES_PER_RUN);
    serde_json::to_vec(&(root_run_id, tree)).expect("delegation tree serializes")
}

pub(super) fn decode_delegation_tree(
    root_run_id: &str,
    bytes: &[u8],
) -> Result<DelegationTree, String> {
    let (stored_root, tree): (String, DelegationTree) = serde_json::from_slice(bytes)
        .map_err(|error| format!("delegation tree failed to decode: {error}"))?;
    if stored_root != root_run_id {
        return Err("delegation tree key does not match its root".into());
    }
    if tree.ids.is_empty()
        || tree.ids.len() > MAX_LEGACY_DELEGATION_EDGES_PER_RUN
        || tree.pending > tree.ids.len() as u64
        || tree.pending > MAX_DELEGATIONS_PER_RUN as u64
        || !tree.ids.windows(2).all(|ids| ids[0] < ids[1])
        || tree.ids.iter().any(|id| !valid_delegation_id(id))
    {
        return Err("delegation tree has invalid ids or counts".into());
    }
    Ok(tree)
}

pub(super) fn encode_delegation_header(header: &DelegationHeader) -> Vec<u8> {
    serde_json::to_vec(header).expect("delegation header serializes")
}

pub(super) fn decode_delegation_header(
    delegation_id: &str,
    bytes: &[u8],
) -> Result<DelegationHeader, String> {
    let header: DelegationHeader = serde_json::from_slice(bytes)
        .map_err(|error| format!("delegation header failed to decode: {error}"))?;
    if header.delegation_id != delegation_id {
        return Err("delegation header key does not match its id".into());
    }
    validate_delegation_header(&header)?;
    Ok(header)
}

pub(super) fn encode_delegation_run_index(index: &DelegationRunIndex) -> Vec<u8> {
    serde_json::to_vec(index).expect("delegation run index serializes")
}

pub(super) fn decode_delegation_run_index(
    dispatch_id: &str,
    bytes: &[u8],
) -> Result<DelegationRunIndex, String> {
    let index: DelegationRunIndex = serde_json::from_slice(bytes)
        .map_err(|error| format!("delegation run index failed to decode: {error}"))?;
    if index.run_id.is_empty()
        || dispatch_id_for(&index.run_id) != dispatch_id
        || !valid_delegation_id(&index.delegation_id)
        || index.root_run_id.is_empty()
    {
        return Err("delegation run index has invalid identity fields".into());
    }
    Ok(index)
}

pub(super) fn encode_delegation_request(request: &DelegationRequest) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(request).expect("delegation request serializes");
    if bytes.len() > MAX_DELEGATIONS_BYTES {
        return Err(Error::Module {
            reason: refusal::CAPACITY.into(),
            sentence: "delegation request exceeds its protocol cap".into(),
        });
    }
    Ok(bytes)
}

pub(super) fn encode_delegation_result(
    delegation_id: &str,
    result: &DelegationResult,
) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(result).expect("delegation result serializes");
    if bytes.len() > MAX_REPLY_BLOCKS_BYTES + 4096 {
        return Err(Error::Module {
            reason: refusal::CAPACITY.into(),
            sentence: format!("delegation result for {delegation_id} exceeds its protocol cap"),
        });
    }
    Ok(bytes)
}

pub(super) fn decode_delegation_result(
    delegation_id: &str,
    bytes: &[u8],
) -> Result<DelegationResult, String> {
    if bytes.len() > MAX_REPLY_BLOCKS_BYTES + 4096 {
        return Err(format!(
            "delegation result for {delegation_id} exceeds its protocol cap"
        ));
    }
    let result: DelegationResult = serde_json::from_slice(bytes)
        .map_err(|error| format!("delegation result failed to decode: {error}"))?;
    if serde_json::to_vec(&result)
        .map_err(|error| format!("delegation result failed to encode: {error}"))?
        .len()
        > MAX_REPLY_BLOCKS_BYTES + 4096
    {
        return Err(format!(
            "delegation result for {delegation_id} exceeds its protocol cap"
        ));
    }
    Ok(result)
}

fn valid_delegation_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn validate_delegation_header(header: &DelegationHeader) -> Result<(), String> {
    if !valid_delegation_id(&header.delegation_id)
        || header.delegation_id != delegation_id_for(&header.caller_run_id, &header.request_id)
        || header.request_id.is_empty()
        || header.request_id.len() > MAX_REQUEST_ID_BYTES
        || contains_run_separator(&header.request_id)
        || header.caller_run_id.is_empty()
        || header.root_run_id.is_empty()
        || header.callee_run_id.is_empty()
        || header.callee_agent_id.is_empty()
    {
        return Err("delegation header has invalid identity fields".into());
    }
    if crate::validate_agent_id(&header.callee_agent_id).is_err()
        || header.callee_run_id
            != delegated_run_id_for(&header.delegation_id, &header.callee_agent_id)
    {
        return Err("delegation header has a non-canonical callee".into());
    }
    if header.status == DelegationStatus::Pending && header.completed_at.is_some() {
        return Err("pending delegation header has a completion timestamp".into());
    }
    if header.status != DelegationStatus::Pending && header.completed_at.is_none() {
        return Err("terminal delegation header has no completion timestamp".into());
    }
    Ok(())
}

pub(super) fn encode_pending_meta(head: Option<&str>, count: u64) -> Vec<u8> {
    serde_json::to_vec(&PendingMeta {
        head: head.map(str::to_owned),
        count,
    })
    .expect("pending metadata serializes")
}

pub(super) fn decode_pending_meta(bytes: &[u8]) -> Result<(Option<String>, u64), String> {
    let meta: PendingMeta = serde_json::from_slice(bytes)
        .map_err(|error| format!("pending metadata failed to decode: {error}"))?;
    if meta.count > super::MAX_PENDING_RUNS {
        return Err("pending metadata exceeds its capacity".into());
    }
    if meta.count == 0 && meta.head.is_some() {
        return Err("empty pending metadata has a head".into());
    }
    if let Some(head) = &meta.head {
        validate_dispatch_key(head)?;
    }
    Ok((meta.head, meta.count))
}

fn validate_dispatch_key(dispatch_id: &str) -> Result<(), String> {
    if dispatch_id.len() != 64 || !dispatch_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("pending link is not a 64-byte hexadecimal dispatch id".into());
    }
    Ok(())
}

fn validate_link(link: &Option<String>) -> Result<(), String> {
    if let Some(link) = link {
        validate_dispatch_key(link)?;
    }
    Ok(())
}

pub(super) fn encode_pending_record(
    pending: &PendingState,
    prev: Option<&str>,
    next: Option<&str>,
) -> Vec<u8> {
    serde_json::to_vec(&PendingRecord {
        pending: pending.clone(),
        prev: prev.map(str::to_owned),
        next: next.map(str::to_owned),
    })
    .expect("pending record serializes")
}

pub(super) fn decode_pending_record(
    dispatch_id: &str,
    bytes: &[u8],
) -> Result<(PendingState, Option<String>, Option<String>), String> {
    validate_dispatch_key(dispatch_id)?;
    let record: PendingRecord = serde_json::from_slice(bytes)
        .map_err(|error| format!("pending record failed to decode: {error}"))?;
    validate_decoded_pending(dispatch_id, &record.pending)?;
    validate_link(&record.prev)?;
    validate_link(&record.next)?;
    if record.prev.as_deref() == Some(dispatch_id) || record.next.as_deref() == Some(dispatch_id) {
        return Err("pending record links to itself".into());
    }
    Ok((record.pending, record.prev, record.next))
}

pub(super) fn encode_session_record(session: &AgentSession) -> Vec<u8> {
    serde_json::to_vec(session).expect("session record serializes")
}

pub(super) fn decode_session_record(
    run_id: &str,
    bytes: &[u8],
    pending: &PendingState,
) -> Result<AgentSession, String> {
    let session: AgentSession = serde_json::from_slice(bytes)
        .map_err(|error| format!("session record failed to decode: {error}"))?;
    if session.run_id != run_id {
        return Err("session record key does not match its run id".into());
    }
    let mut pending_map = BTreeMap::new();
    pending_map.insert(dispatch_id_for(&pending.run_id), pending.clone());
    validate_decoded_session(&pending_map, &session)?;
    Ok(session)
}

fn put_opt_string(out: &mut Vec<u8>, opt: &Option<String>) {
    codec::push_opt_str(out, opt.as_deref());
}

/// the committed sink, as a length-prefixed JSON blob — same shape as the
/// wire (`WireSink` is `runs`' own contract, not a wider committed-state one),
/// but always present here (never optional): every pending entry commits a
/// sink at dispatch, `Chain` included.
fn put_sink(out: &mut Vec<u8>, sink: &WireSink) {
    codec::push_bytes(
        out,
        &serde_json::to_vec(sink).expect("committed sink serializes"),
    );
}

fn take_sink(cur: &mut codec::Cursor) -> Result<WireSink, String> {
    serde_json::from_slice(&take_lp_bytes(cur)?)
        .map_err(|error| format!("snapshot sink failed to decode: {error}"))
}

fn put_origin(out: &mut Vec<u8>, origin: &RunOrigin) {
    match origin {
        RunOrigin::External(key) => {
            out.push(0);
            codec::push_bytes(out, key);
        }
        RunOrigin::Module(module) => {
            out.push(1);
            codec::push_bytes(out, module.as_bytes());
        }
        RunOrigin::System => out.push(2),
        RunOrigin::Program(account) => {
            out.push(3);
            out.extend_from_slice(&account.to_le_bytes());
        }
    }
}

fn encode_body(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    pending: &BTreeMap<String, PendingState>,
    sessions: &BTreeMap<String, AgentSession>,
    delegations: &BTreeMap<String, DelegationState>,
    models: Option<&BTreeMap<String, crate::ModelRecord>>,
) -> Vec<u8> {
    let mut out = Vec::new();

    codec::push_bytes(&mut out, &sdk::wire::encode(action_requests));
    out.extend_from_slice(&next_action_item.to_le_bytes());

    out.extend_from_slice(&(pending.len() as u64).to_le_bytes());
    for (dispatch_id, p) in pending {
        codec::push_bytes(&mut out, dispatch_id.as_bytes());
        out.extend_from_slice(&p.account.to_le_bytes());
        out.extend_from_slice(&p.generation.to_le_bytes());
        codec::push_bytes(&mut out, &sdk::wire::encode(&p.cause));
        codec::push_bytes(&mut out, p.run_id.as_bytes());
        codec::push_bytes(&mut out, p.agent_id.as_bytes());
        codec::push_bytes(&mut out, p.workspace_agent_id.as_bytes());
        put_opt_string(&mut out, &p.delegation_id);
        codec::push_bytes(&mut out, p.channel_id.as_bytes());
        out.extend_from_slice(&p.anchor_seq.to_le_bytes());
        codec::push_opt_u64(&mut out, p.thread_root);
        put_opt_string(&mut out, &p.job_id);
        out.extend_from_slice(&p.job_claim_height.to_le_bytes());
        put_origin(&mut out, &p.requester);
        put_sink(&mut out, &p.sink);
        out.extend_from_slice(&p.created_at.to_le_bytes());
    }

    // the live agent sessions, keyed by run id. the action counter is the
    // spent budget AND the deterministic id salt, so it is committed like
    // every other field — a validator that replayed a different count would
    // mint different ids.
    out.extend_from_slice(&(sessions.len() as u64).to_le_bytes());
    for (run_id, s) in sessions {
        codec::push_bytes(&mut out, run_id.as_bytes());
        codec::push_bytes(&mut out, s.agent_id.as_bytes());
        codec::push_bytes(&mut out, &s.session_key);
        codec::push_bytes(&mut out, &s.lease.holder);
        out.extend_from_slice(&u64::from(s.lease.attempt).to_le_bytes());
        out.extend_from_slice(&s.opened_at.to_le_bytes());
        out.extend_from_slice(&u64::from(s.actions).to_le_bytes());
    }

    out.extend_from_slice(&(delegations.len() as u64).to_le_bytes());
    for (delegation_id, delegation) in delegations {
        codec::push_bytes(&mut out, delegation_id.as_bytes());
        codec::push_bytes(
            &mut out,
            &serde_json::to_vec(delegation).expect("delegation state serializes"),
        );
    }

    if let Some(models) = models {
        codec::push_bytes(&mut out, &sdk::wire::encode(models));
    }
    out
}

pub(super) fn encode_committed(
    receipts: &crate::receipts::Records,
    next_action_item: u64,
    _legacy_delegations: &BTreeMap<String, DelegationState>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&POST_A_MAGIC.to_le_bytes());
    out.push(POST_C_VERSION);
    codec::push_bytes(&mut out, &sdk::wire::encode(receipts));
    out.extend_from_slice(&next_action_item.to_le_bytes());
    out
}

fn encode_delegations(out: &mut Vec<u8>, delegations: &BTreeMap<String, DelegationState>) {
    out.extend_from_slice(&(delegations.len() as u64).to_le_bytes());
    for (delegation_id, delegation) in delegations {
        codec::push_bytes(out, delegation_id.as_bytes());
        codec::push_bytes(
            out,
            &serde_json::to_vec(delegation).expect("delegation state serializes"),
        );
    }
}

pub(super) fn encode_post_b_committed(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    delegations: &BTreeMap<String, DelegationState>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&POST_A_MAGIC.to_le_bytes());
    out.push(POST_B_VERSION);
    codec::push_bytes(&mut out, &sdk::wire::encode(action_requests));
    out.extend_from_slice(&next_action_item.to_le_bytes());
    encode_delegations(&mut out, delegations);
    out
}

pub(super) fn encode_post_a_committed(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    pending: &BTreeMap<String, PendingState>,
    sessions: &BTreeMap<String, AgentSession>,
    delegations: &BTreeMap<String, DelegationState>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&POST_A_MAGIC.to_le_bytes());
    out.push(POST_A_VERSION);
    out.extend_from_slice(&encode_body(
        action_requests,
        next_action_item,
        pending,
        sessions,
        delegations,
        None,
    ));
    out
}

/// The exact pre-Task-A six-field encoding, retained for old-state carry-over.
pub(super) fn encode_legacy_committed(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    pending: &BTreeMap<String, PendingState>,
    sessions: &BTreeMap<String, AgentSession>,
    delegations: &BTreeMap<String, DelegationState>,
    models: &BTreeMap<String, crate::ModelRecord>,
) -> Vec<u8> {
    encode_body(
        action_requests,
        next_action_item,
        pending,
        sessions,
        delegations,
        Some(models),
    )
}

/// the state-based commitment over the committed maps — shared by `root()`
/// and `install()` so the verification a snapshot must pass is definitionally
/// the same algorithm the live module answers with.
pub(super) fn committed_root(
    receipts: &crate::receipts::Records,
    next_action_item: u64,
    delegations: &BTreeMap<String, DelegationState>,
) -> StateRoot {
    StateRoot(Sha256::digest(encode_committed(receipts, next_action_item, delegations)).into())
}

pub(super) fn post_a_root(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    pending: &BTreeMap<String, PendingState>,
    sessions: &BTreeMap<String, AgentSession>,
    delegations: &BTreeMap<String, DelegationState>,
) -> StateRoot {
    StateRoot(
        Sha256::digest(encode_post_a_committed(
            action_requests,
            next_action_item,
            pending,
            sessions,
            delegations,
        ))
        .into(),
    )
}

pub(super) fn post_b_root(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    delegations: &BTreeMap<String, DelegationState>,
) -> StateRoot {
    StateRoot(
        Sha256::digest(encode_post_b_committed(
            action_requests,
            next_action_item,
            delegations,
        ))
        .into(),
    )
}

pub(super) fn legacy_root(
    action_requests: &crate::receipts::Records,
    next_action_item: u64,
    pending: &BTreeMap<String, PendingState>,
    sessions: &BTreeMap<String, AgentSession>,
    delegations: &BTreeMap<String, DelegationState>,
    models: &BTreeMap<String, crate::ModelRecord>,
) -> StateRoot {
    StateRoot(
        Sha256::digest(encode_legacy_committed(
            action_requests,
            next_action_item,
            pending,
            sessions,
            delegations,
            models,
        ))
        .into(),
    )
}

// ---- canonical decoding (UNTRUSTED input) ---------------------------------
// bounds are validated against the remaining input BEFORE any allocation,
// keys must be strictly ascending (one encoding per state, uniqueness for
// free), unknown discriminants/tags and trailing bytes are rejected. never
// panics on malformed input.

// primitives delegate to the shared `sdk::codec::Cursor` (each accessor
// bounds-checks before it reads); the Cursor error is mapped to this module's
// `String` decode contract. the module-specific readers thread one `Cursor`
// through the whole decode.

fn take_byte(cur: &mut codec::Cursor, what: &str) -> Result<u8, String> {
    cur.byte(what).map_err(|e| e.to_string())
}

fn take_u64(cur: &mut codec::Cursor) -> Result<u64, String> {
    cur.u64("snapshot u64").map_err(|e| e.to_string())
}

fn take_lp_bytes(cur: &mut codec::Cursor) -> Result<Vec<u8>, String> {
    cur.bytes("snapshot bytes")
        .map(<[u8]>::to_vec)
        .map_err(|e| e.to_string())
}

fn take_lp_string(cur: &mut codec::Cursor) -> Result<String, String> {
    cur.string("snapshot string").map_err(|e| e.to_string())
}

fn take_opt_u64(cur: &mut codec::Cursor) -> Result<Option<u64>, String> {
    cur.opt_u64("snapshot opt u64").map_err(|e| e.to_string())
}

fn take_opt_string(cur: &mut codec::Cursor) -> Result<Option<String>, String> {
    match take_byte(cur, "snapshot opt tag")? {
        0 => Ok(None),
        1 => Ok(Some(take_lp_string(cur)?)),
        t => Err(format!("snapshot has unknown option tag {t}")),
    }
}

fn take_origin(cur: &mut codec::Cursor) -> Result<RunOrigin, String> {
    match take_byte(cur, "snapshot origin discriminant")? {
        0 => Ok(RunOrigin::External(take_lp_bytes(cur)?)),
        1 => Ok(RunOrigin::Module(take_lp_string(cur)?)),
        2 => Ok(RunOrigin::System),
        3 => Ok(RunOrigin::Program(take_u64(cur)?)),
        d => Err(format!("snapshot has unknown origin discriminant {d}")),
    }
}

/// a section count, bounded by what the remaining input could possibly hold
/// given each entry's minimum encoded size — rejected before the loop builds
/// anything (`Cursor::bound`).
fn take_count(cur: &mut codec::Cursor, min_entry_bytes: u64, what: &str) -> Result<u64, String> {
    let count = take_u64(cur)?;
    cur.bound(count, min_entry_bytes, what)
        .map_err(|e| e.to_string())?;
    Ok(count)
}

/// enforce strictly-ascending map keys while inserting.
fn insert_ascending<V>(map: &mut BTreeMap<String, V>, key: String, value: V) -> Result<(), String> {
    if let Some((last, _)) = map.iter().next_back()
        && last.as_str() >= key.as_str()
    {
        return Err("snapshot keys not strictly ascending".into());
    }
    map.insert(key, value);
    Ok(())
}

pub(super) fn contains_run_separator(value: &str) -> bool {
    value.contains(RUN_KEY_SEPARATOR)
}

pub(super) fn reject_run_separator(field: &str, value: &str) -> Result<(), Error> {
    if contains_run_separator(value) {
        return Err(Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence: format!("{field} must not contain the reserved unit separator"),
        });
    }
    Ok(())
}

/// a decoded pending entry must derive exactly its own key: the dispatch id
/// is the hex sha256 of the run id its fields produce, and the field shapes
/// must match the chat/job keyspace they claim.
pub(super) fn validate_decoded_pending(dispatch_id: &str, p: &PendingState) -> Result<(), String> {
    if contains_run_separator(&p.agent_id) {
        return Err("snapshot agent_id contains reserved unit separator".into());
    }
    if p.run_id.is_empty() {
        return Err("snapshot pending run id is empty".into());
    }
    if contains_run_separator(&p.workspace_agent_id) {
        return Err("snapshot workspace agent id contains reserved unit separator".into());
    }
    match &p.job_id {
        Some(job_id) => {
            if contains_run_separator(job_id) {
                return Err("snapshot job_id contains reserved unit separator".into());
            }
            if !p.channel_id.is_empty() || p.anchor_seq != 0 || p.thread_root.is_some() {
                return Err("snapshot job entry carries chat coordinates".into());
            }
        }
        None => {
            if p.job_claim_height != 0 {
                return Err("snapshot chat entry has non-zero job claim height".into());
            }
            if contains_run_separator(&p.channel_id) {
                return Err("snapshot channel_id contains reserved unit separator".into());
            }
        }
    }
    if dispatch_id != dispatch_id_for(&p.run_id()) {
        return Err("snapshot dispatch id does not match its run fields".into());
    }
    Ok(())
}

fn validate_post_b_records(
    records: &crate::receipts::Records,
) -> Result<BTreeMap<String, PendingState>, String> {
    let mut pending = BTreeMap::new();
    let mut links = BTreeMap::new();
    for (key, bytes) in records {
        if bytes.len() > sdk::MAX_STORE_VALUE_BYTES {
            return Err("receipt record exceeds the store value bound".into());
        }
        if key == RUN_META_KEY {
            continue;
        }
        if let Some(dispatch_id) = key.strip_prefix("run/") {
            let (entry, prev, next) = decode_pending_record(dispatch_id, bytes)?;
            pending.insert(dispatch_id.to_owned(), entry);
            links.insert(dispatch_id.to_owned(), (prev, next));
        } else if key.starts_with("session/") {
            continue;
        }
    }
    let (head, count) = match records.get(RUN_META_KEY) {
        Some(bytes) => decode_pending_meta(bytes)?,
        None => (None, 0),
    };
    if count != pending.len() as u64 {
        return Err("pending metadata count does not match records".into());
    }
    if count == 0 {
        if head.is_some() {
            return Err("empty pending list has a head".into());
        }
    } else {
        let mut current = head.ok_or_else(|| "pending list has no head".to_string())?;
        let mut previous = None;
        let mut seen = BTreeSet::new();
        let mut tail = None;
        let mut ended = false;
        for _ in 0..count {
            if !seen.insert(current.clone()) {
                return Err("pending list contains a cycle".into());
            }
            let Some((prev, next)) = links.get(&current) else {
                return Err("pending list names a missing record".into());
            };
            if prev.as_deref() != previous.as_deref() {
                return Err("pending list has a broken previous link".into());
            }
            previous = Some(current.clone());
            tail = Some(current.clone());
            current = match next {
                Some(next) => next.clone(),
                None => {
                    ended = true;
                    current
                }
            };
            if ended {
                break;
            }
        }
        if seen.len() != count as usize || !ended || tail != Some(current) {
            return Err("pending list has an invalid tail or count".into());
        }
        for (id, (prev, next)) in &links {
            if let Some(next) = next
                && links.get(next).and_then(|(prev, _)| prev.as_ref()) != Some(id)
            {
                return Err("pending list has a broken next link".into());
            }
            if let Some(prev) = prev
                && links.get(prev).and_then(|(_, next)| next.as_ref()) != Some(id)
            {
                return Err("pending list has a broken previous link".into());
            }
        }
    }
    for (key, bytes) in records {
        let Some(run_id) = key.strip_prefix("session/") else {
            continue;
        };
        let dispatch_id = dispatch_id_for(run_id);
        let entry = pending
            .get(&dispatch_id)
            .ok_or_else(|| "snapshot session names no in-flight run".to_string())?;
        decode_session_record(run_id, bytes, entry)?;
    }
    super::conversations::validate_records(records)?;
    Ok(pending)
}

fn validate_decoded_delegations(
    pending: &BTreeMap<String, PendingState>,
    delegations: &BTreeMap<String, DelegationState>,
) -> Result<(), String> {
    let mut roots = BTreeMap::<&str, usize>::new();
    let mut totals = BTreeMap::<&str, usize>::new();
    let mut caller_totals = BTreeMap::<&str, usize>::new();
    let mut graphs = BTreeMap::<&str, Vec<(&str, &str)>>::new();
    for (id, state) in delegations {
        let view = &state.view;
        if id != &view.delegation_id
            || id != &delegation_id_for(&view.caller_run_id, &view.request_id)
        {
            return Err("snapshot delegation id does not match its caller request".into());
        }
        if view.request_id.is_empty()
            || view.request_id.len() > MAX_REQUEST_ID_BYTES
            || contains_run_separator(&view.request_id)
        {
            return Err("snapshot delegation request id is invalid".into());
        }
        if view.callee_agent_id != state.request.agent_id {
            return Err("snapshot delegation callee does not match its request".into());
        }
        validate_delegation_header(&DelegationHeader::from_state(state))?;
        if (view.status == DelegationStatus::Pending) != view.result.is_none() {
            return Err("snapshot delegation status does not match its result".into());
        }
        if state.request.instruction.trim().is_empty()
            || state.request.instruction.len() > MAX_DELEGATION_INSTRUCTION_BYTES
            || serde_json::to_vec(&state.request)
                .map_err(|error| format!("snapshot delegation request failed to encode: {error}"))?
                .len()
                > MAX_DELEGATIONS_BYTES
        {
            return Err("snapshot delegation request exceeds its protocol bounds".into());
        }
        if let Some(result) = &view.result
            && serde_json::to_vec(result)
                .map_err(|error| format!("snapshot delegation result failed to encode: {error}"))?
                .len()
                > MAX_REPLY_BLOCKS_BYTES + 4096
        {
            return Err("snapshot delegation result exceeds its protocol bounds".into());
        }
        let root = pending
            .get(&dispatch_id_for(&view.root_run_id))
            .ok_or_else(|| "snapshot delegation root is not in flight".to_string())?;
        if root.delegation_id.is_some() {
            return Err("snapshot delegation tree root is itself delegated".into());
        }
        if view.status == DelegationStatus::Pending {
            let child = pending
                .get(&dispatch_id_for(&view.callee_run_id))
                .ok_or_else(|| "snapshot pending delegation has no callee run".to_string())?;
            if child.delegation_id.as_deref() != Some(id.as_str())
                || child.agent_id != view.callee_agent_id
            {
                return Err("snapshot callee run points at a different delegation".into());
            }
        } else if pending.contains_key(&dispatch_id_for(&view.callee_run_id)) {
            return Err("snapshot terminal delegation still has a pending callee".into());
        }
        graphs
            .entry(&view.root_run_id)
            .or_default()
            .push((&view.caller_run_id, &view.callee_run_id));
        let caller_total = caller_totals.entry(&view.caller_run_id).or_default();
        *caller_total += 1;
        if *caller_total > MAX_ACTIONS_PER_SESSION as usize {
            return Err("snapshot caller exceeds its historical action budget".into());
        }
        let total = totals.entry(&view.root_run_id).or_default();
        *total += 1;
        if *total > MAX_LEGACY_DELEGATION_EDGES_PER_RUN {
            return Err(
                "snapshot delegation tree exceeds its historical compatibility limit".into(),
            );
        }
        if view.status == DelegationStatus::Pending {
            *roots.entry(&view.root_run_id).or_default() += 1;
        }
    }
    if roots.values().any(|count| *count > MAX_DELEGATIONS_PER_RUN) {
        return Err("snapshot delegation tree exceeds its concurrency limit".into());
    }
    for (root, edges) in graphs {
        validate_delegation_graph(root, edges)?;
    }
    for pending_run in pending.values() {
        let Some(id) = pending_run.delegation_id.as_deref() else {
            continue;
        };
        let edge = delegations
            .get(id)
            .ok_or_else(|| "snapshot delegated run names no call edge".to_string())?;
        if edge.view.status != DelegationStatus::Pending
            || edge.view.callee_run_id != pending_run.run_id
        {
            return Err("snapshot delegated run does not match its pending call edge".into());
        }
    }
    Ok(())
}

fn validate_delegation_graph<'a>(
    root: &str,
    edges: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    let mut reachable = BTreeSet::from([root.to_owned()]);
    let mut remaining: Vec<_> = edges
        .into_iter()
        .map(|(caller, callee)| (caller.to_owned(), callee.to_owned()))
        .collect();
    while !remaining.is_empty() {
        let mut progressed = false;
        remaining.retain(|(caller, callee)| {
            if reachable.contains(caller) {
                reachable.insert(callee.clone());
                progressed = true;
                false
            } else {
                true
            }
        });
        if !progressed {
            return Err("delegation caller is disconnected from its root".into());
        }
    }
    Ok(())
}

fn validate_post_c_delegations(
    records: &crate::receipts::Records,
    pending: &BTreeMap<String, PendingState>,
) -> Result<(), String> {
    let mut trees = BTreeMap::<String, DelegationTree>::new();
    let mut headers = BTreeMap::<String, DelegationHeader>::new();
    let mut indexes = BTreeMap::<String, DelegationRunIndex>::new();
    let mut replies = BTreeSet::new();
    let mut memberships = BTreeMap::<String, String>::new();

    for (key, bytes) in records {
        if let Some(root) = key.strip_prefix("dlg/tree/") {
            if root.is_empty() {
                return Err("delegation tree has an empty root".into());
            }
            let tree = decode_delegation_tree(root, bytes)?;
            if trees.insert(root.to_owned(), tree).is_some() {
                return Err("duplicate delegation tree".into());
            }
            continue;
        }
        if let Some(id) = key.strip_prefix("dlg/reply/") {
            if !valid_delegation_id(id) {
                return Err("delegation reply has an invalid id".into());
            }
            decode_delegation_result(id, bytes)?;
            if !replies.insert(id.to_owned()) {
                return Err("duplicate delegation reply".into());
            }
            continue;
        }
        if let Some(dispatch_id) = key.strip_prefix("dlg/run/") {
            if !valid_delegation_id(dispatch_id) {
                return Err("delegation run index has an invalid dispatch id".into());
            }
            let index = decode_delegation_run_index(dispatch_id, bytes)?;
            if indexes.insert(index.run_id.clone(), index).is_some() {
                return Err("duplicate delegation run index".into());
            }
            continue;
        }
        if key.starts_with("dlg/req/") {
            return Err("post-C state retains a consumed delegation request".into());
        }
        if let Some(id) = key.strip_prefix("dlg/") {
            if id.is_empty() || id.contains('/') {
                return Err("delegation header key is not point-addressed".into());
            }
            let header = decode_delegation_header(id, bytes)?;
            if headers.insert(id.to_owned(), header).is_some() {
                return Err("duplicate delegation header".into());
            }
            continue;
        }
        if key.starts_with("dlg") {
            return Err("unknown delegation record namespace".into());
        }
    }

    for (root, tree) in &trees {
        let root_entry = pending
            .get(&dispatch_id_for(root))
            .ok_or_else(|| "delegation tree root is not in flight".to_string())?;
        if root_entry.delegation_id.is_some() {
            return Err("delegation tree root is itself delegated".into());
        }
        let mut pending_count = 0;
        let mut edges = Vec::with_capacity(tree.ids.len());
        for id in &tree.ids {
            if memberships.insert(id.clone(), root.clone()).is_some() {
                return Err("delegation header belongs to multiple trees".into());
            }
            let header = headers
                .get(id)
                .ok_or_else(|| "delegation tree names a missing header".to_string())?;
            if header.root_run_id != *root {
                return Err("delegation header is in the wrong tree".into());
            }
            if header.status == DelegationStatus::Pending {
                pending_count += 1;
                let child = pending
                    .get(&dispatch_id_for(&header.callee_run_id))
                    .ok_or_else(|| "pending delegation has no callee run".to_string())?;
                if child.delegation_id.as_deref() != Some(id.as_str())
                    || child.agent_id != header.callee_agent_id
                {
                    return Err("pending callee points at a different delegation".into());
                }
            } else {
                if !replies.contains(id) {
                    return Err("terminal delegation has no reply record".into());
                }
                if pending.contains_key(&dispatch_id_for(&header.callee_run_id)) {
                    return Err("terminal delegation still has a pending callee".into());
                }
            }
            edges.push((header.caller_run_id.as_str(), header.callee_run_id.as_str()));
        }
        if pending_count != tree.pending {
            return Err("delegation tree pending count does not match headers".into());
        }
        if pending_count > MAX_DELEGATIONS_PER_RUN as u64 {
            return Err("delegation tree exceeds its concurrency limit".into());
        }
        validate_delegation_graph(root, edges)?;
    }
    for (id, header) in &headers {
        match memberships.get(id) {
            Some(root) if root == &header.root_run_id => {}
            Some(_) => return Err("delegation header is in the wrong tree".into()),
            None => {
                return Err("orphan delegation header".into());
            }
        }
    }
    for header in headers.values() {
        let index = indexes
            .get(&header.callee_run_id)
            .ok_or_else(|| "delegation header has no run index".to_string())?;
        if index.delegation_id != header.delegation_id || index.root_run_id != header.root_run_id {
            return Err("delegation run index does not match its header".into());
        }
    }
    if indexes.len() != headers.len() {
        return Err("orphan delegation run index".into());
    }
    for child in pending.values() {
        let Some(id) = child.delegation_id.as_deref() else {
            continue;
        };
        let header = headers
            .get(id)
            .ok_or_else(|| "delegated run names no delegation header".to_string())?;
        if header.status != DelegationStatus::Pending
            || header.callee_run_id != child.run_id
            || header.callee_agent_id != child.agent_id
        {
            return Err("delegated run does not match its pending header".into());
        }
    }
    for id in replies {
        let header = headers
            .get(&id)
            .ok_or_else(|| "orphan delegation reply".to_string())?;
        if header.status == DelegationStatus::Pending {
            return Err("pending delegation has a reply record".into());
        }
    }
    Ok(())
}

/// a decoded session must be one a live module could have staged: the key IS
/// the run id, the key length is the one this module admits, and the counter is
/// within the budget the action lane enforces. `pending` is the run's own
/// existence check — a session may never outlive its run, so a snapshot whose
/// session names no in-flight run is not one any honest node could produce.
fn validate_decoded_session(
    pending: &BTreeMap<String, PendingState>,
    s: &AgentSession,
) -> Result<(), String> {
    if s.session_key.len() != SESSION_KEY_LEN {
        return Err("snapshot session key is not a 32-byte ed25519 key".into());
    }
    if s.lease.holder.is_empty() {
        return Err("snapshot session names no lease holder".into());
    }
    if contains_run_separator(&s.agent_id) {
        return Err("snapshot session agent_id contains reserved unit separator".into());
    }
    if s.actions > MAX_ACTIONS_PER_SESSION {
        return Err("snapshot session has spent more than its action budget".into());
    }
    let entry = pending
        .get(&dispatch_id_for(&s.run_id))
        .ok_or_else(|| "snapshot session names no in-flight run".to_string())?;
    if entry.agent_id != s.agent_id {
        return Err("snapshot session agent does not match its run".into());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StateVersion {
    V0,
    V1,
    V2,
    V3,
}

pub(super) struct Committed {
    pub(super) version: StateVersion,
    pub(super) receipts: crate::receipts::Records,
    pub(super) next_action_item: u64,
    pub(super) pending: BTreeMap<String, PendingState>,
    pub(super) sessions: BTreeMap<String, AgentSession>,
    pub(super) delegations: BTreeMap<String, DelegationState>,
    pub(super) legacy_models: Option<BTreeMap<String, crate::ModelRecord>>,
}

fn decode_delegations(
    cur: &mut codec::Cursor,
    min_entry_bytes: u64,
) -> Result<BTreeMap<String, DelegationState>, String> {
    let mut delegations = BTreeMap::new();
    let count = take_count(cur, min_entry_bytes, "delegation")?;
    for _ in 0..count {
        let id = take_lp_string(cur)?;
        let state: DelegationState = serde_json::from_slice(&take_lp_bytes(cur)?)
            .map_err(|error| format!("snapshot delegation failed to decode: {error}"))?;
        insert_ascending(&mut delegations, id, state)?;
    }
    Ok(delegations)
}

fn decode_post_b(cur: &mut codec::Cursor) -> Result<Committed, String> {
    const MIN_DELEGATION_BYTES: u64 = 8 + 8;
    let receipts: crate::receipts::Records = sdk::wire::decode(&take_lp_bytes(cur)?)?;
    let next_action_item = take_u64(cur)?;
    let delegations = decode_delegations(cur, MIN_DELEGATION_BYTES)?;
    if cur.remaining() != 0 {
        return Err("snapshot has trailing bytes".into());
    }
    let pending = validate_post_b_records(&receipts)?;
    validate_decoded_delegations(&pending, &delegations)?;
    Ok(Committed {
        version: StateVersion::V2,
        receipts,
        next_action_item,
        pending: BTreeMap::new(),
        sessions: BTreeMap::new(),
        delegations,
        legacy_models: None,
    })
}

fn decode_post_c(cur: &mut codec::Cursor) -> Result<Committed, String> {
    let receipts: crate::receipts::Records = sdk::wire::decode(&take_lp_bytes(cur)?)?;
    let next_action_item = take_u64(cur)?;
    if cur.remaining() != 0 {
        return Err("snapshot has trailing bytes".into());
    }
    let pending = validate_post_b_records(&receipts)?;
    validate_post_c_delegations(&receipts, &pending)?;
    Ok(Committed {
        version: StateVersion::V3,
        receipts,
        next_action_item,
        pending: BTreeMap::new(),
        sessions: BTreeMap::new(),
        delegations: BTreeMap::new(),
        legacy_models: None,
    })
}

pub(super) fn decode_committed(bytes: &[u8]) -> Result<Committed, String> {
    // Each pending entry includes identity generation, causal provenance,
    // model/workspace coordinates, requester and the committed sink. The
    // minimum excludes variable bodies, which each decoder checks below.
    const MIN_PENDING_BYTES: u64 =
        8 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1 + 8 + 8 + 1 + 1 + 8 + 1 + 8 + 8;
    const MIN_SESSION_BYTES: u64 = 8 + 8 + 8 + 8 + 8 + 8 + 8;
    const MIN_DELEGATION_BYTES: u64 = 8 + 8;

    // Old state is bounded by the store value ceiling, so its first
    // length-prefixed field cannot equal POST_A_MAGIC. Shape selection is
    // therefore unambiguous before any untrusted allocation.
    let post_a = bytes.len() >= 8
        && u64::from_le_bytes(bytes[..8].try_into().expect("length checked")) == POST_A_MAGIC;
    let mut cur = codec::Cursor::new(bytes);
    let version = if post_a {
        let _ = take_u64(&mut cur)?;
        take_byte(&mut cur, "post-A state version")?
    } else {
        0
    };
    if post_a && version == POST_B_VERSION {
        return decode_post_b(&mut cur);
    }
    if post_a && version == POST_C_VERSION {
        return decode_post_c(&mut cur);
    }
    if post_a && version != POST_A_VERSION {
        return Err(format!("unsupported state version {version}"));
    }
    let action_requests = sdk::wire::decode(&take_lp_bytes(&mut cur)?)?;
    let next_action_item = take_u64(&mut cur)?;

    let mut pending: BTreeMap<String, PendingState> = BTreeMap::new();
    let count = take_count(&mut cur, MIN_PENDING_BYTES, "pending")?;
    if count > super::MAX_PENDING_RUNS {
        return Err("pending state exceeds its capacity".into());
    }
    for _ in 0..count {
        let dispatch_id = take_lp_string(&mut cur)?;
        let account = take_u64(&mut cur)?;
        let generation = take_u64(&mut cur)?;
        let cause = sdk::wire::decode(&take_lp_bytes(&mut cur)?)?;
        let run_id = take_lp_string(&mut cur)?;
        let agent_id = take_lp_string(&mut cur)?;
        let workspace_agent_id = take_lp_string(&mut cur)?;
        let delegation_id = take_opt_string(&mut cur)?;
        let channel_id = take_lp_string(&mut cur)?;
        let anchor_seq = take_u64(&mut cur)?;
        let thread_root = take_opt_u64(&mut cur)?;
        let job_id = take_opt_string(&mut cur)?;
        let job_claim_height = take_u64(&mut cur)?;
        let requester = take_origin(&mut cur)?;
        let sink = take_sink(&mut cur)?;
        let created_at = take_u64(&mut cur)?;
        let entry = PendingState {
            account,
            generation,
            cause,
            run_id,
            agent_id,
            workspace_agent_id,
            delegation_id,
            channel_id,
            anchor_seq,
            thread_root,
            job_id,
            job_claim_height,
            requester,
            sink,
            created_at,
        };
        validate_decoded_pending(&dispatch_id, &entry)?;
        insert_ascending(&mut pending, dispatch_id, entry)?;
    }

    let mut sessions: BTreeMap<String, AgentSession> = BTreeMap::new();
    let count = take_count(&mut cur, MIN_SESSION_BYTES, "session")?;
    for _ in 0..count {
        let run_id = take_lp_string(&mut cur)?;
        let agent_id = take_lp_string(&mut cur)?;
        let session_key = take_lp_bytes(&mut cur)?;
        let holder = take_lp_bytes(&mut cur)?;
        let attempt = u32::try_from(take_u64(&mut cur)?)
            .map_err(|_| "snapshot session attempt exceeds u32".to_string())?;
        let opened_at = take_u64(&mut cur)?;
        let actions = u32::try_from(take_u64(&mut cur)?)
            .map_err(|_| "snapshot session action count exceeds u32".to_string())?;
        let session = AgentSession {
            run_id: run_id.clone(),
            agent_id,
            session_key,
            lease: crate::ExecutionLease { holder, attempt },
            opened_at,
            actions,
        };
        validate_decoded_session(&pending, &session)?;
        insert_ascending(&mut sessions, run_id, session)?;
    }

    let delegations = decode_delegations(&mut cur, MIN_DELEGATION_BYTES)?;
    validate_decoded_delegations(&pending, &delegations)?;
    super::conversations::validate_records(&action_requests)?;

    let models = if post_a {
        None
    } else {
        let models: BTreeMap<String, crate::ModelRecord> =
            sdk::wire::decode(&take_lp_bytes(&mut cur)?)?;
        let mut owners = BTreeMap::<String, usize>::new();
        for (id, record) in &models {
            if id != &record.agent_id
                || record.account == 0
                || sdk::wire::encode(record).len() > crate::MAX_AGENT_RECORD_BYTES
            {
                return Err("invalid model record".into());
            }
            crate::validate_agent_id(id)?;
            let owner = serde_json::to_vec(&record.owner)
                .map_err(|error| format!("legacy model owner failed to encode: {error}"))?;
            *owners
                .entry(String::from_utf8_lossy(&owner).into_owned())
                .or_default() += 1;
        }
        if models.len() > crate::MAX_REGISTERED_AGENTS {
            return Err("legacy model registry exceeds its capacity".into());
        }
        if owners
            .values()
            .any(|count| *count > crate::MAX_AGENTS_PER_OWNER)
        {
            return Err("legacy model owner index exceeds its capacity".into());
        }
        Some(models)
    };
    if cur.remaining() != 0 {
        return Err("snapshot has trailing bytes".into());
    }
    Ok(Committed {
        version: if post_a {
            StateVersion::V1
        } else {
            StateVersion::V0
        },
        receipts: action_requests,
        next_action_item,
        pending,
        sessions,
        delegations,
        legacy_models: models,
    })
}
