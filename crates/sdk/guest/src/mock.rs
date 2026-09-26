//! The host a native test runs a module over: maps for state and blobs, and
//! what the module sent (`output`, `response`, `emissions`, `events`) kept
//! for the test to read. [`MockChain`] runs the emissions as the kernel runs
//! them once the handler returns, in the same frame. Sibling modules answer through `siblings`;
//! signatures verify through `verifier` (none set: verification is refused).
//! A `MockHost` is a shared handle: the contexts made over it and the test
//! see one host.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::role::identity::Profile;
use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, Message,
    Scheme,
};
use sha1::Digest as _;

use crate::{
    Cause, Env, Error, ExecCtx, MessageId, ModuleId, Order, Origin, Outcome, Principal, QueryCtx,
    Range, Roles, code,
};

pub type Sibling = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, Error>>;

/// The identity role over a fixed roster, for a native test of a module
/// that names accounts: `Profile` and `Profiles` (paged by number) out of
/// `profiles`, in any order given. `Account` and `OfModule` are the
/// kernel's to ask, so a module asking them is refused, as a sibling
/// answers only what its role says.
pub fn identity_role(profiles: &[Profile], request: &[u8]) -> Result<Vec<u8>, Error> {
    use abi::role::identity::{Query, Reply};
    let reply = match abi::decode(request).map_err(crate::kernel::error_from)? {
        Query::Profile(number) => {
            Reply::Profile(profiles.iter().find(|p| p.number == number).cloned())
        }
        Query::Profiles { after, limit } => {
            let mut page: Vec<Profile> = profiles
                .iter()
                .filter(|p| after.is_none_or(|after| p.number > after))
                .cloned()
                .collect();
            page.sort_by_key(|p| p.number);
            let limit = limit.max(1) as usize;
            let more = page.len() > limit;
            page.truncate(limit);
            let next = page.last().filter(|_| more).map(|p| p.number);
            Reply::Profiles {
                profiles: page,
                next,
            }
        }
        Query::Account(_) | Query::OfModule(_) => {
            return Err(Error::new(
                code::UNSUPPORTED,
                "the kernel alone asks identity who holds a key or a module",
            ));
        }
    };
    Ok(abi::encode(&reply))
}
pub type Verifier = Box<dyn Fn(Scheme, &[u8], &[u8], &[u8], &[u8]) -> bool>;

#[derive(Default)]
pub struct MockState {
    pub state: BTreeMap<Vec<u8>, Vec<u8>>,
    pub blobs: BTreeMap<BlobId, Blob>,
    /// The last `set_return_data`.
    pub output: Vec<u8>,
    /// The last query's response.
    pub response: Vec<u8>,
    pub emissions: Vec<Message>,
    /// The number the next emitted message takes in its frame: 0 in each
    /// new [`MockHost::exec`]; [`MockChain`] carries it across the frame.
    pub next_item: u64,
    pub events: Vec<Vec<u8>>,
    pub siblings: BTreeMap<ModuleId, Sibling>,
    pub verifier: Option<Verifier>,
}

#[derive(Clone, Default)]
pub struct MockHost(Rc<RefCell<MockState>>);

impl MockHost {
    /// The roles as the suite's genesis binds them, for a native test's
    /// env: a sibling registered under `identity` answers the identity role.
    pub fn roles() -> Roles {
        Roles {
            registry: "module-registry".into(),
            validators: "valset".into(),
            identity: "identity".into(),
        }
    }

    /// An execute's context over this host, in a frame of its own: its
    /// messages are numbered from 0.
    pub fn exec(&self, env: Env) -> ExecCtx {
        self.borrow_mut().next_item = 0;
        ExecCtx::over(self.clone(), env)
    }

    /// A query's context over this host.
    pub fn query(&self, env: Env) -> QueryCtx {
        QueryCtx::over(self.clone(), env)
    }

    pub fn borrow(&self) -> Ref<'_, MockState> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, MockState> {
        self.0.borrow_mut()
    }

    pub fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.borrow_mut().output)
    }

    pub fn take_emissions(&self) -> Vec<Message> {
        std::mem::take(&mut self.borrow_mut().emissions)
    }

    /// Runs `write` and, when it refuses, checks it left this host as it
    /// found it: no state, blob, emission, event or output. The attack
    /// check every module's harness shares: a rule checks before it
    /// writes, so a refusal needs no rollback.
    #[track_caller]
    pub fn attempt<T>(&self, write: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        let before = self.written();
        let result = write();
        if result.is_err() {
            let after = self.written();
            assert_eq!(after.0, before.0, "a refused write changed state");
            assert_eq!(after.1, before.1, "a refused write stored a blob");
            assert_eq!(after.2, before.2, "a refused write sent something");
        }
        result
    }

    /// [`MockHost::attempt`] a write that must refuse: its refusal.
    #[track_caller]
    pub fn refused<T: std::fmt::Debug>(&self, write: impl FnOnce() -> Result<T, Error>) -> Error {
        self.attempt(write).expect_err("the write was refused")
    }

    #[allow(clippy::type_complexity)]
    fn written(
        &self,
    ) -> (
        BTreeMap<Vec<u8>, Vec<u8>>,
        BTreeMap<BlobId, Blob>,
        (Vec<u8>, Vec<Message>, Vec<Vec<u8>>),
    ) {
        let mock = self.borrow();
        (
            mock.state.clone(),
            mock.blobs.clone(),
            (
                mock.output.clone(),
                mock.emissions.clone(),
                mock.events.clone(),
            ),
        )
    }

    /// One host call by `me`, as the real host answers it. An emitted
    /// message is kept, numbered in its frame, for [`MockChain`] (or the
    /// test) to run once the handler returns.
    pub(crate) fn serve(&self, me: &str, op: HostOp) -> HostReply {
        if let HostOp::Query { program, request } = op {
            // The sibling leaves the map while it answers, so it may use
            // this host itself.
            let sibling = self.borrow_mut().siblings.remove(&program);
            return HostReply::Query(match sibling {
                Some(sibling) => {
                    let answer = sibling(&request).map_err(crate::kernel::refusal_from);
                    self.borrow_mut().siblings.insert(program, sibling);
                    answer
                }
                None => Err(abi::Refusal::new(code::UNKNOWN_MODULE, program)),
            });
        }
        let mut mock = self.borrow_mut();
        match op {
            HostOp::Get(key) | HostOp::CommittedGet(key) => {
                HostReply::Value(mock.state.get(&key).cloned())
            }
            HostOp::Scan(scan) | HostOp::CommittedScan(scan) => {
                HostReply::Entries(mock.scan_state(&scan.into()))
            }
            HostOp::BlobGet(id) => HostReply::Blob(mock.blobs.get(&id).cloned()),
            HostOp::BlobStat(id) => {
                HostReply::BlobHeader(mock.blobs.get(&id).map(|blob| BlobHeader {
                    kind: blob.kind.clone(),
                    len: blob.body.len() as u64,
                }))
            }
            HostOp::BlobRead { id, offset, len } => {
                HostReply::Value(mock.blobs.get(&id).map(|b| {
                    let start = (offset as usize).min(b.body.len());
                    let end = start.saturating_add(len as usize).min(b.body.len());
                    b.body[start..end].to_vec()
                }))
            }
            HostOp::Root(_) => HostReply::Root(None),
            HostOp::Crypto(CryptoOp::Sha256(bytes)) => {
                HostReply::Crypto(CryptoReply::Digest(sha2::Sha256::digest(bytes).into()))
            }
            HostOp::Crypto(CryptoOp::Verify {
                scheme,
                key,
                namespace,
                message,
                signature,
            }) => match &mock.verifier {
                Some(verify) => HostReply::Crypto(CryptoReply::Verified(verify(
                    scheme, &key, &namespace, &message, &signature,
                ))),
                None => HostReply::Refused(abi::Refusal::new(
                    code::UNSUPPORTED,
                    "MockHost verifies nothing until a verifier is set",
                )),
            },
            HostOp::Set { key, value } => {
                mock.state.insert(key, value);
                HostReply::Done
            }
            HostOp::Delete(key) => {
                mock.state.remove(&key);
                HostReply::Done
            }
            HostOp::BlobPut { hash, kind, body } => match blob_id(hash, &kind, &body) {
                Ok(id) => {
                    mock.blobs.insert(id, Blob { kind, body });
                    HostReply::BlobId(id)
                }
                Err(error) => HostReply::Refused(crate::kernel::refusal_from(error)),
            },
            HostOp::Emit(message) => {
                let item = mock.next_item;
                mock.next_item += 1;
                mock.emissions.push(message);
                HostReply::Item(abi::ItemRef {
                    source: me.to_owned(),
                    item,
                })
            }
            HostOp::Event(payload) => {
                mock.events.push(payload);
                HostReply::Done
            }
            HostOp::Output(bytes) => {
                mock.output = bytes;
                HostReply::Done
            }
            HostOp::Respond(bytes) => {
                mock.response = bytes;
                HostReply::Done
            }
            HostOp::Query { .. } => unreachable!("answered above"),
        }
    }
}

/// A module's execute as a frame runs it, over its context: its op bytes
/// (empty for a [`Cause::Reply`], as the kernel sends them). A
/// [`Module`](crate::Module)'s is [`execute::<M>`](crate::execute), which
/// hands a reply to [`Module::reply`](crate::Module::reply).
pub type Execute = fn(&ExecCtx, &[u8]) -> Result<(), Error>;

/// How deep messages nest in one frame, the kernel's `MAX_DEPTH`: a frame's
/// first run is at depth 0, each message or reply one deeper than its emitter.
pub const MAX_DEPTH: u32 = 8;

/// Native modules, each over its own [`MockHost`], and a frame run as the
/// kernel runs one: the first run, then each message it emitted in order,
/// depth first, as [`Cause::Message`] from the emitter acting as its account;
/// a message with [`Reply::Wanted`](crate::Reply::Wanted) comes back to the
/// emitter as [`Cause::Reply`] (an empty payload), the target's writes undone
/// when it refused. A refused message without a reply, a refused reply, or a
/// run past [`MAX_DEPTH`] fails the whole frame: every seated host's state
/// and blobs as they were before it. Messages are numbered from 0 in each
/// frame, whichever run emitted them. No fuel: a native run is not metered.
/// Events are not undone: each host keeps every run's, as the kernel's
/// receipts keep a refused run's events under its refusal.
#[derive(Clone, Default)]
pub struct MockChain {
    seats: BTreeMap<ModuleId, Seat>,
}

/// Each seated host's state and blobs, in seat order.
type Checkpoint = Vec<(BTreeMap<Vec<u8>, Vec<u8>>, BTreeMap<BlobId, Blob>)>;

#[derive(Clone)]
struct Seat {
    host: MockHost,
    account: Option<Principal>,
    execute: Execute,
}

impl MockChain {
    /// Seats `module` over `host`, acting as `account` in what it emits.
    pub fn seat(
        &mut self,
        module: impl Into<ModuleId>,
        host: MockHost,
        account: Option<Principal>,
        execute: Execute,
    ) {
        let seat = Seat {
            host,
            account,
            execute,
        };
        self.seats.insert(module.into(), seat);
    }

    /// One frame: `env.module` runs `payload`, then what it emitted. The
    /// first run's outcome, or the rejection that failed the frame.
    pub fn execute(&self, env: Env, payload: &[u8]) -> Outcome {
        self.run(env, payload, &mut 0, 0)
    }

    fn run(&self, env: Env, payload: &[u8], next_item: &mut u64, depth: u32) -> Outcome {
        if depth > MAX_DEPTH {
            let deep = format!("messages nest deeper than {MAX_DEPTH}");
            return Outcome::Rejected(Error::new(code::CAPACITY, deep));
        }
        let Some(seat) = self.seats.get(&env.module) else {
            return Outcome::Rejected(Error::new(code::UNKNOWN_MODULE, env.module));
        };
        let checkpoint = self.checkpoint();
        let emitter = env.clone();
        let ctx = seat.host.exec(env);
        let first = *next_item;
        {
            let mut mock = seat.host.borrow_mut();
            mock.next_item = first;
            // each run's output is its own, as the kernel's is
            mock.emissions.clear();
            mock.output.clear();
        }
        let ran = (seat.execute)(&ctx, payload);
        *next_item = seat.host.borrow().next_item;
        let emitted = seat.host.take_emissions();
        if let Err(error) = ran {
            self.restore(checkpoint);
            return Outcome::Rejected(error);
        }
        let output = seat.host.take_output();
        let env = |module: &ModuleId, from: &ModuleId, cause| Env {
            module: module.clone(),
            origin: Origin::Module(from.clone()),
            sender: self.seats.get(from).and_then(|seat| seat.account.clone()),
            cause,
            ..emitter.clone()
        };
        let me = &emitter.module;
        for (seq, message) in (first..).zip(emitted) {
            let id = MessageId {
                module: me.clone(),
                seq,
            };
            let cause = Cause::Message(id.clone());
            let outcome = self.run(
                env(&message.target, me, cause),
                &message.payload,
                next_item,
                depth + 1,
            );
            let failed = match (outcome, message.reply) {
                (Outcome::Applied { .. }, false) => continue,
                (rejected, false) => rejected,
                (outcome, true) => {
                    let cause = Cause::Reply { id, outcome };
                    let reply = env(me, &message.target, cause);
                    match self.run(reply, &[], next_item, depth + 1) {
                        Outcome::Applied { .. } => continue,
                        rejected => rejected,
                    }
                }
            };
            self.restore(checkpoint);
            return failed;
        }
        Outcome::Applied { output }
    }

    // ponytail: copies every seated host's state and blobs per run; an
    // undo log if a harness's state grows large enough to feel it.
    fn checkpoint(&self) -> Checkpoint {
        let seats = self.seats.values().map(|seat| seat.host.borrow());
        seats
            .map(|mock| (mock.state.clone(), mock.blobs.clone()))
            .collect()
    }

    fn restore(&self, checkpoint: Checkpoint) {
        for (seat, (state, blobs)) in self.seats.values().zip(checkpoint) {
            let mut mock = seat.host.borrow_mut();
            (mock.state, mock.blobs) = (state, blobs);
        }
    }
}

impl MockState {
    fn scan_state(&self, range: &Range) -> Vec<Entry> {
        let admitted =
            self.state
                .iter()
                .filter(|(key, _)| range.admits(key))
                .map(|(key, value)| Entry {
                    key: key.clone(),
                    value: value.clone(),
                });
        let ordered: Vec<Entry> = if range.order == Order::Descending {
            admitted.rev().collect()
        } else {
            admitted.collect()
        };
        match range.limit {
            Some(limit) => ordered.into_iter().take(limit as usize).collect(),
            None => ordered,
        }
    }
}

/// The host's framing: `<kind> <len>\0<body>`, hashed whole.
pub fn blob_id(hash: HashKind, kind: &str, body: &[u8]) -> Result<BlobId, Error> {
    let kind_is_a_word = !kind.is_empty() && !kind.contains([' ', '\0']);
    if !kind_is_a_word {
        return Err(Error::new(
            code::INVALID_INPUT,
            "a blob kind is one non-empty word without spaces or NUL",
        ));
    }
    let mut framed = format!("{kind} {}\0", body.len()).into_bytes();
    framed.extend_from_slice(body);
    Ok(match hash {
        HashKind::Sha256 => BlobId::Sha256(sha2::Sha256::digest(&framed).into()),
        HashKind::Sha1 => BlobId::Sha1(sha1::Sha1::digest(&framed).into()),
    })
}

/// [`MockChain`] against the kernel's frame (ducktape's
/// `crates/kernel/host/src/lib.rs`, `run_frame`): a probe module seated
/// under several names, each run of it logged with its env.
#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use borsh::{BorshDeserialize, BorshSerialize};

    use super::*;
    use crate::Reply;

    #[derive(BorshSerialize, BorshDeserialize)]
    enum Probe {
        /// writes `wrote`, then emits each `(target, op, reply wanted)`
        Emit(Vec<(String, Vec<u8>, bool)>),
        /// writes `wrote`, then refuses
        Refuse,
        /// writes `wrote`, then emits `Chain(n - 1)` to itself while `n > 0`
        Chain(u32),
    }

    thread_local! {
        static RUNS: RefCell<Vec<Env>> = const { RefCell::new(Vec::new()) };
    }

    /// The probe, a plain [`Module`](crate::Module): each run logged; a
    /// reply absorbed, except by `sour`, which refuses every reply.
    struct Prober;

    impl crate::Module for Prober {
        type Op = Probe;
        type Query = ();
        type Response = ();

        fn execute(ctx: &ExecCtx, op: Probe) -> Result<(), Error> {
            RUNS.with(|runs| runs.borrow_mut().push(ctx.env().clone()));
            ctx.set("wrote", b"yes".to_vec());
            match op {
                Probe::Emit(messages) => {
                    for (target, op, reply) in messages {
                        let reply = if reply { Reply::Wanted } else { Reply::None };
                        ctx.emit(target, op, reply);
                    }
                    ctx.set_return_data(b"out".to_vec());
                    Ok(())
                }
                Probe::Refuse => Err(Error::new(code::WRONG_STATE, "refused")),
                Probe::Chain(0) => Ok(()),
                Probe::Chain(n) => {
                    let me = ctx.env().module.clone();
                    ctx.emit(me, abi::encode(&Probe::Chain(n - 1)), Reply::None);
                    Ok(())
                }
            }
        }

        fn reply(ctx: &ExecCtx, _: &MessageId, _: &Outcome) -> Result<(), Error> {
            RUNS.with(|runs| runs.borrow_mut().push(ctx.env().clone()));
            match ctx.env().module.as_str() {
                "sour" => Err(Error::new(code::INVALID_INPUT, "sour")),
                _ => Ok(()),
            }
        }

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    const MODULES: [(&str, u64); 4] = [("a", 1), ("b", 2), ("c", 3), ("sour", 4)];

    fn chain() -> MockChain {
        RUNS.with(|runs| runs.borrow_mut().clear());
        let mut chain = MockChain::default();
        for (module, account) in MODULES {
            let account = Some(Principal::Account(account));
            chain.seat(
                module,
                MockHost::default(),
                account,
                crate::execute::<Prober>,
            );
        }
        chain
    }

    fn frame(chain: &MockChain, module: &str, op: &Probe) -> Outcome {
        let env = Env {
            chain_id: b"n".to_vec(),
            height: 1,
            time: 2,
            module: module.into(),
            origin: Origin::Root,
            sender: Some(Principal::Root),
            roles: MockHost::roles(),
            cause: Cause::Direct,
        };
        chain.execute(env, &abi::encode(op))
    }

    fn wrote(chain: &MockChain, module: &str) -> bool {
        chain.seats[module]
            .host
            .borrow()
            .state
            .contains_key(&b"wrote"[..])
    }

    fn runs() -> Vec<Env> {
        RUNS.with(|runs| runs.borrow().clone())
    }

    fn to(target: &str, op: &Probe, reply: bool) -> (String, Vec<u8>, bool) {
        (target.into(), abi::encode(op), reply)
    }

    fn id(module: &str, seq: u64) -> MessageId {
        MessageId {
            module: module.into(),
            seq,
        }
    }

    #[test]
    fn a_message_runs_as_a_message_numbered_in_its_frame_from_zero() {
        let chain = chain();
        let inner = Probe::Emit(vec![to("c", &Probe::Emit(vec![]), false)]);
        let op = Probe::Emit(vec![
            to("b", &inner, false),
            to("c", &Probe::Emit(vec![]), false),
        ]);
        for _ in 0..2 {
            RUNS.with(|runs| runs.borrow_mut().clear());
            let outcome = frame(&chain, "a", &op);
            assert_eq!(
                outcome,
                Outcome::Applied {
                    output: b"out".to_vec()
                }
            );
            let seen: Vec<_> = runs()
                .into_iter()
                .map(|env| (env.module, env.origin, env.sender, env.cause))
                .collect();
            let from = |module: &str, account| {
                (
                    Origin::Module(module.into()),
                    Some(Principal::Account(account)),
                )
            };
            let (a, b) = (from("a", 1), from("b", 2));
            assert_eq!(
                seen,
                [
                    (
                        "a".into(),
                        Origin::Root,
                        Some(Principal::Root),
                        Cause::Direct
                    ),
                    (
                        "b".into(),
                        a.0.clone(),
                        a.1.clone(),
                        Cause::Message(id("a", 0))
                    ),
                    // b's message is the frame's third: a's two were numbered first
                    ("c".into(), b.0, b.1, Cause::Message(id("b", 2))),
                    ("c".into(), a.0, a.1, Cause::Message(id("a", 1))),
                ]
            );
        }
    }

    #[test]
    fn a_refused_message_without_a_reply_fails_the_whole_frame() {
        let chain = chain();
        let op = Probe::Emit(vec![
            to("c", &Probe::Emit(vec![]), false),
            to("b", &Probe::Refuse, false),
        ]);
        let outcome = frame(&chain, "a", &op);
        assert_eq!(
            outcome,
            Outcome::Rejected(Error::new(code::WRONG_STATE, "refused"))
        );
        assert!(!wrote(&chain, "a") && !wrote(&chain, "b") && !wrote(&chain, "c"));
    }

    #[test]
    fn a_wanted_reply_undoes_the_refusal_and_comes_back_in_the_frame() {
        let chain = chain();
        let op = Probe::Emit(vec![
            to("b", &Probe::Refuse, true),
            to("c", &Probe::Emit(vec![]), true),
        ]);
        let outcome = frame(&chain, "a", &op);
        assert_eq!(
            outcome,
            Outcome::Applied {
                output: b"out".to_vec()
            }
        );
        assert!(wrote(&chain, "a") && !wrote(&chain, "b") && wrote(&chain, "c"));
        let replies: Vec<_> = runs()
            .into_iter()
            .filter(|env| matches!(env.cause, Cause::Reply { .. }))
            .map(|env| (env.module, env.origin, env.sender, env.cause))
            .collect();
        let refused = Outcome::Rejected(Error::new(code::WRONG_STATE, "refused"));
        let applied = Outcome::Applied {
            output: b"out".to_vec(),
        };
        assert_eq!(
            replies,
            [
                (
                    "a".into(),
                    Origin::Module("b".into()),
                    Some(Principal::Account(2)),
                    Cause::Reply {
                        id: id("a", 0),
                        outcome: refused
                    },
                ),
                (
                    "a".into(),
                    Origin::Module("c".into()),
                    Some(Principal::Account(3)),
                    Cause::Reply {
                        id: id("a", 1),
                        outcome: applied
                    },
                ),
            ]
        );
    }

    #[test]
    fn a_refused_reply_fails_the_whole_frame() {
        let chain = chain();
        let op = Probe::Emit(vec![to("b", &Probe::Emit(vec![]), true)]);
        let outcome = frame(&chain, "sour", &op);
        assert_eq!(
            outcome,
            Outcome::Rejected(Error::new(code::INVALID_INPUT, "sour"))
        );
        assert!(!wrote(&chain, "sour") && !wrote(&chain, "b"));
    }

    #[test]
    fn messages_nest_eight_deep_and_no_deeper() {
        let chain = chain();
        let outcome = frame(&chain, "a", &Probe::Chain(MAX_DEPTH));
        assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
        assert_eq!(runs().len(), MAX_DEPTH as usize + 1);
        chain.seats["a"].host.borrow_mut().state.clear();
        let outcome = frame(&chain, "a", &Probe::Chain(MAX_DEPTH + 1));
        let deep = Error::new(code::CAPACITY, "messages nest deeper than 8");
        assert_eq!(outcome, Outcome::Rejected(deep));
        assert!(!wrote(&chain, "a"));
    }

    #[test]
    fn a_message_to_no_module_fails_the_frame() {
        let chain = chain();
        let op = Probe::Emit(vec![to("nobody", &Probe::Emit(vec![]), false)]);
        let outcome = frame(&chain, "a", &op);
        assert_eq!(
            outcome,
            Outcome::Rejected(Error::new(code::UNKNOWN_MODULE, "nobody"))
        );
        assert!(!wrote(&chain, "a"));
    }

    #[test]
    fn a_refused_runs_output_is_not_a_later_runs() {
        fn leaky(ctx: &ExecCtx, payload: &[u8]) -> Result<(), Error> {
            if payload.is_empty() {
                return Ok(());
            }
            ctx.set_return_data(b"stale".to_vec());
            Err(Error::new(code::WRONG_STATE, "refused"))
        }
        let mut chain = MockChain::default();
        chain.seat("x", MockHost::default(), None, leaky);
        let env = Env {
            chain_id: b"n".to_vec(),
            height: 1,
            time: 2,
            module: "x".into(),
            origin: Origin::Root,
            sender: Some(Principal::Root),
            roles: MockHost::roles(),
            cause: Cause::Direct,
        };
        let refused = chain.execute(env.clone(), &[1]);
        assert!(matches!(refused, Outcome::Rejected(_)));
        let outcome = chain.execute(env, &[]);
        assert_eq!(outcome, Outcome::Applied { output: Vec::new() });
    }
}
