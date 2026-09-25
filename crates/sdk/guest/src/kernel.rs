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
//! | `Env { network, me, .. }`                 | [`Env`] `{ chain_id, module, .. }`       |
//! | `ProgramId`                               | [`ModuleId`]                             |
//! | `ItemRef { source, item }`                | [`MessageId`] `{ module, seq }`          |
//! | `Cause::{Direct, Delivery, Completion}`   | [`Cause`]`::{Direct, Message, Reply}`    |
//! | `Outcome` (carries a `Refusal`)           | [`Outcome`] (carries an [`Error`])       |
//! | `Scan { lo, hi, reverse, limit }`         | [`Range`] `{ start, end, order, limit }` |

use borsh::{BorshDeserialize, BorshSerialize};

/// A module's id on the chain (`"chat"`, `"module-registry"`).
pub type ModuleId = abi::ProgramId;

/// Why a call failed: a [`code`] naming the class of failure (how a caller
/// recovers) and one sentence for a person.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// An [`Error`]'s code: two errors share one exactly when a caller does the
/// same thing about them. The kernel's `reason` tokens, same strings.
pub mod code {
    pub use abi::reason::*;
}

impl From<abi::Refusal> for Error {
    fn from(r: abi::Refusal) -> Self {
        Error::new(r.reason, r.sentence)
    }
}

impl From<Error> for abi::Refusal {
    fn from(e: Error) -> Self {
        abi::Refusal::new(e.code, e.message)
    }
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

/// A message a module sent: the sender and its sequence there.
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
            abi::Outcome::Rejected(r) => Outcome::Rejected(r.into()),
        }
    }
}

impl From<Outcome> for abi::Outcome {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Applied { output } => abi::Outcome::Applied { output },
            Outcome::Rejected(e) => abi::Outcome::Rejected(e.into()),
        }
    }
}

/// Why this call runs: a transaction, a message another module sent, or the
/// reply to a message this module sent with `call`.
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
            abi::Cause::Delivery(item) => Cause::Message(item.into()),
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
            Cause::Message(id) => abi::Cause::Delivery(id.into()),
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
        for (cause, origin) in causes.zip(origins.into_iter().cycle()) {
            let env = Env {
                chain_id: b"n".to_vec(),
                height: 7,
                time: 9,
                module: "a".into(),
                origin,
                cause,
            };
            let kernel: abi::Env = env.clone().into();
            assert_eq!(abi::encode(&env), abi::encode(&kernel));
            assert_eq!(abi::decode::<Env>(&abi::encode(&kernel)).unwrap(), env);
            assert_eq!(Env::from(kernel), env);
        }
        let error = Error::new(code::STALE, "behind");
        let refusal: abi::Refusal = error.clone().into();
        assert_eq!(abi::encode(&error), abi::encode(&refusal));
        assert_eq!(Error::from(refusal), error);
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
}
