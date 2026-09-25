//! The native test harness: [`MockHost`], the store a test runs a module's
//! rules over, and [`env`], the call context it runs them in.
//!
//! ```ignore
//! let mut host = MockHost::default();
//! rules::execute(&mut host, &env(Origin::Signed(key)), op)?;          // must succeed
//! let err = host.refused(|host| rules::execute(host, &env, bad_op));  // must fail, untouched
//! assert_eq!(err.code, code::UNAUTHORIZED);
//! ```
//!
//! [`MockHost::attempt`] runs a write and, when it fails, checks it left the
//! host as it found it; [`MockHost::refused`] is `attempt` for a write that
//! must fail. Every refusal a module test checks goes through one of them.

use std::collections::BTreeMap;

use abi::{CryptoOp, CryptoReply, HostOp, HostReply, Scan};
use borsh::{BorshDeserialize, BorshSerialize};
use sha1::Digest as _;

use crate::{
    Blob, BlobHeader, BlobId, Cause, Entry, Env, Error, HashKind, Message, ModuleId, Origin, Range,
    Reads, Scheme, Writes, code, decode, encode,
};

/// Another module's query, answered natively: request bytes in, reply bytes out.
pub type MockModule = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, Error>>;
pub type Verifier = Box<dyn Fn(Scheme, &[u8], &[u8], &[u8], &[u8]) -> bool>;

/// A call context for a test: height 1, time 1000, module `"test"`, a
/// direct call from `origin`. Adjust fields with struct update syntax.
pub fn env(origin: Origin) -> Env {
    Env {
        chain_id: b"test".to_vec(),
        height: 1,
        time: 1_000,
        module: "test".into(),
        origin,
        cause: Cause::Direct,
    }
}

/// Maps for state and blobs, and what the rules sent (`return_data`,
/// `sent`, `events`) kept for the test to read. Other modules answer queries
/// through `modules`; signatures verify through `verifier` (none set:
/// verification is refused).
#[derive(Default)]
pub struct MockHost {
    pub state: BTreeMap<Vec<u8>, Vec<u8>>,
    pub blobs: BTreeMap<BlobId, Blob>,
    /// The last `set_return_data`.
    pub return_data: Vec<u8>,
    pub sent: Vec<Message>,
    pub events: Vec<Vec<u8>>,
    pub modules: BTreeMap<ModuleId, MockModule>,
    pub verifier: Option<Verifier>,
}

impl MockHost {
    pub fn take_return_data(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.return_data)
    }

    pub fn take_sent(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.sent)
    }

    /// Answers `module`'s queries with `answer`, typed.
    pub fn mock_module<Q: BorshDeserialize, R: BorshSerialize>(
        &mut self,
        module: impl Into<ModuleId>,
        answer: impl Fn(Q) -> Result<R, Error> + 'static,
    ) {
        self.modules.insert(
            module.into(),
            Box::new(move |request| Ok(encode(&answer(decode(request)?)?))),
        );
    }

    /// Runs `write` and, when it fails, checks it left this host as it
    /// found it: no state, blob, message, event or return data. A rule
    /// checks before it writes, so a failure needs no rollback.
    #[track_caller]
    pub fn attempt<T>(
        &mut self,
        write: impl FnOnce(&mut MockHost) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let state = self.state.clone();
        let blobs = self.blobs.clone();
        let sent = (
            self.return_data.clone(),
            self.sent.clone(),
            self.events.clone(),
        );
        let result = write(self);
        if result.is_err() {
            assert_eq!(self.state, state, "a refused write changed state");
            assert_eq!(self.blobs, blobs, "a refused write stored a blob");
            let after = (
                self.return_data.clone(),
                self.sent.clone(),
                self.events.clone(),
            );
            assert_eq!(after, sent, "a refused write sent something");
        }
        result
    }

    /// [`MockHost::attempt`] a write that must fail: its error.
    #[track_caller]
    pub fn refused<T: std::fmt::Debug>(
        &mut self,
        write: impl FnOnce(&mut MockHost) -> Result<T, Error>,
    ) -> Error {
        self.attempt(write).expect_err("the write was refused")
    }

    fn scan_state(&self, scan: &Scan) -> Vec<Entry> {
        let range = Range::from(scan.clone());
        let admitted = self
            .state
            .iter()
            .filter(|(key, _)| range.contains(key))
            .map(|(key, value)| Entry {
                key: key.clone(),
                value: value.clone(),
            });
        let ordered: Vec<Entry> = if scan.reverse {
            admitted.rev().collect()
        } else {
            admitted.collect()
        };
        match scan.limit {
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

impl Reads for MockHost {
    fn host(&self, op: &HostOp) -> HostReply {
        match op {
            HostOp::Get(key) | HostOp::CommittedGet(key) => {
                HostReply::Value(self.state.get(key).cloned())
            }
            HostOp::Scan(scan) | HostOp::CommittedScan(scan) => {
                HostReply::Entries(self.scan_state(scan))
            }
            HostOp::BlobGet(id) => HostReply::Blob(self.blobs.get(id).cloned()),
            HostOp::BlobStat(id) => {
                HostReply::BlobHeader(self.blobs.get(id).map(|blob| BlobHeader {
                    kind: blob.kind.clone(),
                    len: blob.body.len() as u64,
                }))
            }
            HostOp::BlobRead { id, offset, len } => HostReply::Value(self.blobs.get(id).map(|b| {
                let start = (*offset as usize).min(b.body.len());
                let end = start.saturating_add(*len as usize).min(b.body.len());
                b.body[start..end].to_vec()
            })),
            HostOp::Root(_) => HostReply::Root(None),
            HostOp::Query { program, request } => HostReply::Query(
                match self.modules.get(program) {
                    Some(module) => module(request),
                    None => Err(Error::new(code::UNKNOWN_PROGRAM, program.clone())),
                }
                .map_err(Into::into),
            ),
            HostOp::Crypto(CryptoOp::Sha256(bytes)) => {
                HostReply::Crypto(CryptoReply::Digest(sha2::Sha256::digest(bytes).into()))
            }
            HostOp::Crypto(CryptoOp::Verify {
                scheme,
                key,
                namespace,
                message,
                signature,
            }) => match &self.verifier {
                Some(verify) => HostReply::Crypto(CryptoReply::Verified(verify(
                    *scheme, key, namespace, message, signature,
                ))),
                None => HostReply::Refused(abi::Refusal::new(
                    code::UNSUPPORTED,
                    "MockHost verifies nothing until a verifier is set",
                )),
            },
            other => {
                panic!("MockHost serves reads through `host`; {other:?} goes through `Writes`")
            }
        }
    }
}

impl Writes for MockHost {
    fn host_mut(&mut self, op: HostOp) -> HostReply {
        match op {
            HostOp::Set { key, value } => {
                self.state.insert(key, value);
            }
            HostOp::Delete(key) => {
                self.state.remove(&key);
            }
            HostOp::BlobPut { hash, kind, body } => {
                return match blob_id(hash, &kind, &body) {
                    Ok(id) => {
                        self.blobs.insert(id, Blob { kind, body });
                        HostReply::BlobId(id)
                    }
                    Err(e) => HostReply::Refused(e.into()),
                };
            }
            HostOp::Emit(message) => {
                self.sent.push(message);
                return HostReply::Item(abi::ItemRef {
                    source: String::new(),
                    item: self.sent.len() as u64,
                });
            }
            HostOp::Event(payload) => self.events.push(payload),
            HostOp::Output(bytes) | HostOp::Respond(bytes) => self.return_data = bytes,
            read => return self.host(&read),
        }
        HostReply::Done
    }
}
