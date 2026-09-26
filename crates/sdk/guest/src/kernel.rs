//! The one place the kernel's names become the module SDK's.
//!
//! `abi` is a byte-for-byte copy of the kernel's contract and keeps the
//! kernel's names. A module author reads these instead; every type here has
//! the same borsh layout as its kernel twin (borsh encodes no names), so the
//! conversions below move fields and never touch bytes.
//!
//! | kernel (`abi`)                            | module SDK (`guest`)                     |
//! |-------------------------------------------|------------------------------------------|
//! | `Refusal { reason, sentence }`, `reason`  | [`Error`] `{ code, message }`, [`code`]  |
//! | `Origin::{External, Program, System}`     | [`Origin`]`::{Signed, Module, Root}`     |
//! | `Principal::{Account, System}`            | [`Principal`]`::{Account, Root}`         |
//! | `Env { network, me, .. }`                 | [`Env`] `{ chain_id, module, .. }`       |
//! | `Roles`                                   | [`Roles`] (the same type)                |
//! | `ProgramId`                               | [`ModuleId`]                             |
//! | `ItemRef { source, item }`                | [`MessageId`] `{ module, seq }`          |
//! | `Cause::{Direct, Message, Completion}`    | [`Cause`]`::{Direct, Message, Reply}`    |
//! | `Outcome` (carries a `Refusal`)           | [`Outcome`] (carries an [`Error`])       |
//! | `Scan { lo, hi, reverse, limit }`         | [`Range`] `{ start, end, order, limit }` |

use borsh::{BorshDeserialize, BorshSerialize};

/// A module's id on the chain (`"chat"`, `"module-registry"`).
pub type ModuleId = abi::ProgramId;

pub use abi::Roles;
pub use error::{Error, code};

/// The kernel's refusal as the SDK's [`Error`]: the same two strings.
pub fn error_from(refusal: abi::Refusal) -> Error {
    Error::new(refusal.reason, refusal.sentence)
}

/// The SDK's [`Error`] as the kernel's refusal.
pub fn refusal_from(error: Error) -> abi::Refusal {
    abi::Refusal::new(error.code, error.message)
}

/// Who called: a signed transaction (the signer's key), another module, or
/// the chain itself (genesis and system-internal calls).
#[derive(Clone, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub enum Origin {
    Signed(Vec<u8>),
    Module(ModuleId),
    Root,
}

impl From<abi::Origin> for Origin {
    fn from(o: abi::Origin) -> Self {
        match o {
            abi::Origin::External(key) => Origin::Signed(key),
            abi::Origin::Program(id) => Origin::Module(id),
            abi::Origin::System => Origin::Root,
        }
    }
}

impl From<Origin> for abi::Origin {
    fn from(o: Origin) -> Self {
        match o {
            Origin::Signed(key) => abi::Origin::External(key),
            Origin::Module(id) => abi::Origin::Program(id),
            Origin::Root => abi::Origin::System,
        }
    }
}

/// An account's number: the identity role's (`abi::role::identity`).
pub type AccountNumber = abi::role::identity::AccountNumber;

/// Who a write acts as, resolved by the host once per frame through the
/// identity role: a signed frame is the account its key holds, a message
/// the account of the module that sent it, genesis `Root`. A key that holds
/// no account (or an account that is not live) acts as no one
/// ([`ExecCtx::sender`](crate::ExecCtx::sender) refuses), so no row ever
/// names a bare key.
///
/// There is no default principal: "nobody" is `Option<Principal>::None`,
/// never the most trusted variant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum Principal {
    /// a person, an agent or a module: an identity account.
    Account(AccountNumber),
    /// genesis and system-internal writes.
    Root,
}

impl Principal {
    /// The account this principal is, if it is one.
    pub fn account(&self) -> Option<AccountNumber> {
        match self {
            Principal::Account(account) => Some(*account),
            Principal::Root => None,
        }
    }

    /// The principal a reader writes as, from the account their seated key
    /// holds. None while it holds none: every view gates its writes on this,
    /// the way [`ExecCtx::sender`](crate::ExecCtx::sender) refuses them.
    pub fn writer(account: Option<AccountNumber>) -> Option<Principal> {
        account.map(Principal::Account)
    }

    /// A principal typed by a person: an account number, `acct:<n>` too.
    pub fn parse(text: &str) -> Option<Principal> {
        let text = text.trim();
        let number = text.strip_prefix("acct:").unwrap_or(text);
        number.parse().ok().map(Principal::Account)
    }
}

impl From<abi::Principal> for Principal {
    fn from(p: abi::Principal) -> Self {
        match p {
            abi::Principal::Account(number) => Principal::Account(number),
            abi::Principal::System => Principal::Root,
        }
    }
}

impl From<Principal> for abi::Principal {
    fn from(p: Principal) -> Self {
        match p {
            Principal::Account(number) => abi::Principal::Account(number),
            Principal::Root => abi::Principal::System,
        }
    }
}

/// A message a module emitted: the emitter and the message's number in
/// its frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
pub struct MessageId {
    pub module: ModuleId,
    pub seq: u64,
}

impl From<abi::ItemRef> for MessageId {
    fn from(i: abi::ItemRef) -> Self {
        MessageId {
            module: i.source,
            seq: i.item,
        }
    }
}

impl From<MessageId> for abi::ItemRef {
    fn from(m: MessageId) -> Self {
        abi::ItemRef {
            source: m.module,
            item: m.seq,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Outcome {
    Applied { output: Vec<u8> },
    Rejected(Error),
}

impl From<abi::Outcome> for Outcome {
    fn from(o: abi::Outcome) -> Self {
        match o {
            abi::Outcome::Applied { output } => Outcome::Applied { output },
            abi::Outcome::Rejected(r) => Outcome::Rejected(error_from(r)),
        }
    }
}

impl From<Outcome> for abi::Outcome {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Applied { output } => abi::Outcome::Applied { output },
            Outcome::Rejected(e) => abi::Outcome::Rejected(refusal_from(e)),
        }
    }
}

/// Why this call runs: a transaction, a message another module emitted in
/// this frame, or the reply to a message this module emitted with
/// [`Reply::Wanted`](crate::Reply::Wanted).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Cause {
    Direct,
    Message(MessageId),
    Reply { id: MessageId, outcome: Outcome },
}

impl From<abi::Cause> for Cause {
    fn from(c: abi::Cause) -> Self {
        match c {
            abi::Cause::Direct => Cause::Direct,
            abi::Cause::Message(item) => Cause::Message(item.into()),
            abi::Cause::Completion { item, outcome } => Cause::Reply {
                id: item.into(),
                outcome: outcome.into(),
            },
        }
    }
}

impl From<Cause> for abi::Cause {
    fn from(c: Cause) -> Self {
        match c {
            Cause::Direct => abi::Cause::Direct,
            Cause::Message(id) => abi::Cause::Message(id.into()),
            Cause::Reply { id, outcome } => abi::Cause::Completion {
                item: id.into(),
                outcome: outcome.into(),
            },
        }
    }
}

/// What a call runs in: the chain, the block, this module, who called and why.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Env {
    pub chain_id: Vec<u8>,
    pub height: u64,
    pub time: u64,
    /// This module's own id.
    pub module: ModuleId,
    pub origin: Origin,
    /// Who the frame acts as: `None` for a query, and for a frame whose
    /// key or module holds no account.
    pub sender: Option<Principal>,
    /// The module genesis bound to each role: ask identity at
    /// `roles.identity`, never by a name assumed.
    pub roles: Roles,
    pub cause: Cause,
}

impl From<abi::Env> for Env {
    fn from(e: abi::Env) -> Self {
        Env {
            chain_id: e.network,
            height: e.height,
            time: e.time,
            module: e.me,
            origin: e.origin.into(),
            sender: e.sender.map(Into::into),
            roles: e.roles,
            cause: e.cause.into(),
        }
    }
}

impl From<Env> for abi::Env {
    fn from(e: Env) -> Self {
        abi::Env {
            network: e.chain_id,
            height: e.height,
            time: e.time,
            me: e.module,
            origin: e.origin.into(),
            sender: e.sender.map(Into::into),
            roles: e.roles,
            cause: e.cause.into(),
        }
    }
}

/// A [`Range`]'s direction; the kernel's `reverse` flag (`false`, `true`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Order {
    #[default]
    Ascending,
    Descending,
}

/// The keys from `start` (inclusive) to `end` (exclusive; `None`: to the
/// last key), in `order`, at most `limit` of them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Range {
    pub start: Vec<u8>,
    pub end: Option<Vec<u8>>,
    pub order: Order,
    pub limit: Option<u64>,
}

impl Range {
    pub fn new(start: impl Into<Vec<u8>>, end: Option<Vec<u8>>) -> Self {
        Range {
            start: start.into(),
            end,
            order: Order::Ascending,
            limit: None,
        }
    }

    pub fn prefix(prefix: impl AsRef<[u8]>) -> Self {
        let prefix = prefix.as_ref();
        Range::new(prefix.to_vec(), abi::prefix_end(prefix))
    }

    pub fn after(mut self, key: impl AsRef<[u8]>) -> Self {
        let mut start = key.as_ref().to_vec();
        start.push(0);
        self.start = start;
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn reverse(mut self) -> Self {
        self.order = Order::Descending;
        self
    }

    pub fn admits(&self, key: &[u8]) -> bool {
        let above_start = key >= self.start.as_slice();
        let below_end = self.end.as_ref().is_none_or(|end| key < end.as_slice());
        above_start && below_end
    }
}

impl From<Range> for abi::Scan {
    fn from(r: Range) -> Self {
        abi::Scan {
            lo: r.start,
            hi: r.end,
            reverse: r.order == Order::Descending,
            limit: r.limit,
        }
    }
}

impl From<abi::Scan> for Range {
    fn from(s: abi::Scan) -> Self {
        Range {
            start: s.lo,
            end: s.hi,
            order: if s.reverse {
                Order::Descending
            } else {
                Order::Ascending
            },
            limit: s.limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every renamed type is its kernel twin's bytes, both ways.
    #[test]
    fn every_sdk_type_is_its_kernel_twins_bytes() {
        let replies = [
            Outcome::Rejected(Error::new("x", "y")),
            Outcome::Applied { output: vec![4] },
        ];
        let causes = replies
            .into_iter()
            .map(|outcome| Cause::Reply {
                id: MessageId {
                    module: "a".into(),
                    seq: 3,
                },
                outcome,
            })
            .chain([
                Cause::Direct,
                Cause::Message(MessageId {
                    module: "c".into(),
                    seq: 1,
                }),
            ]);
        let origins = [
            Origin::Signed(vec![1]),
            Origin::Module("b".into()),
            Origin::Root,
        ];
        let senders = [Some(Principal::Account(4)), Some(Principal::Root), None];
        let calls = origins.into_iter().cycle().zip(senders.into_iter().cycle());
        for (cause, (origin, sender)) in causes.zip(calls) {
            let env = Env {
                chain_id: b"n".to_vec(),
                height: 7,
                time: 9,
                module: "a".into(),
                origin,
                sender,
                roles: crate::MockHost::roles(),
                cause,
            };
            let kernel: abi::Env = env.clone().into();
            assert_eq!(abi::encode(&env), abi::encode(&kernel));
            assert_eq!(abi::decode::<Env>(&abi::encode(&kernel)).unwrap(), env);
            assert_eq!(Env::from(kernel), env);
        }
        let error = Error::new(code::STALE, "behind");
        let refusal = refusal_from(error.clone());
        assert_eq!(abi::encode(&error), abi::encode(&refusal));
        assert_eq!(error_from(refusal), error);
        for range in [
            Range::prefix(b"t/").after(b"t/7").reverse().limit(2),
            Range::new(b"a".to_vec(), None),
        ] {
            let scan: abi::Scan = range.clone().into();
            assert_eq!(abi::encode(&range), abi::encode(&scan));
            assert_eq!(Range::from(scan), range);
        }
        let range = Range::prefix(b"t/").after(b"t/7");
        assert!(!range.admits(b"t/7"));
        assert!(range.admits(b"t/8"));
        assert!(!range.admits(b"u"));
    }

    #[test]
    fn a_principal_reads_back_from_input() {
        assert_eq!(Principal::parse(" 7 "), Some(Principal::Account(7)));
        assert_eq!(Principal::parse("acct:7"), Some(Principal::Account(7)));
        for nothing in ["acct:x", "user:ab01", "ab01", "", "someone"] {
            assert_eq!(Principal::parse(nothing), None, "{nothing}");
        }
        assert_eq!(Principal::writer(Some(3)), Some(Principal::Account(3)));
        assert_eq!(Principal::writer(None), None);
    }

    #[test]
    fn codes_are_the_kernels_reasons() {
        use abi::reason as r;
        let pairs = [
            (code::UNKNOWN_MODULE, r::UNKNOWN_PROGRAM),
            (code::TRAP, r::TRAP),
            (code::PROTOCOL, r::PROTOCOL),
            (code::SEQUENCE, r::SEQUENCE),
            (code::NOT_FOUND, r::NOT_FOUND),
            (code::ALREADY_EXISTS, r::ALREADY_EXISTS),
            (code::STALE, r::STALE),
            (code::WRONG_STATE, r::WRONG_STATE),
            (code::INVALID_INPUT, r::INVALID_INPUT),
            (code::CAPACITY, r::CAPACITY),
            (code::NOT_YET, r::NOT_YET),
            (code::EXHAUSTED, r::EXHAUSTED),
            (code::UNAUTHORIZED, r::UNAUTHORIZED),
            (code::UNSUPPORTED, r::UNSUPPORTED),
            (code::CORRUPT, r::CORRUPT),
            (code::UNEXPECTED_REPLY, r::UNEXPECTED_REPLY),
        ];
        for (code, reason) in pairs {
            assert_eq!(code, reason);
        }
        // one for one: a token added on either side is missing here
        let count = |text: &str| text.matches("pub const").count();
        let reasons = include_str!("../../abi/src/lib.rs");
        let reasons = &reasons[reasons.find("pub mod reason").unwrap()..];
        let reasons = &reasons[..reasons.find("\n}").unwrap()];
        let codes = include_str!("../../error/src/lib.rs");
        let codes = &codes[codes.find("pub mod code").unwrap()..];
        assert_eq!(count(reasons), pairs.len());
        assert_eq!(count(codes), pairs.len());
    }
}
