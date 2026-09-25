//! The host a native test runs a module over: maps for state and blobs, and
//! what the module sent (`output`, `response`, `emissions`, `events`) kept
//! for the test to read. Sibling programs answer through `siblings`;
//! signatures verify through `verifier` (none set: verification is refused).
//! A `MockHost` is a shared handle: the contexts made over it and the test
//! see one host.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, Env, HashKind, HostOp, HostReply,
    ItemRef, Message, ProgramId, Refusal, Scan, Scheme, reason,
};
use sha1::Digest as _;

use crate::{ExecCtx, QueryCtx};

pub type Sibling = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, Refusal>>;
pub type Verifier = Box<dyn Fn(Scheme, &[u8], &[u8], &[u8], &[u8]) -> bool>;

#[derive(Default)]
pub struct MockState {
    pub state: BTreeMap<Vec<u8>, Vec<u8>>,
    pub blobs: BTreeMap<BlobId, Blob>,
    /// The last `output`.
    pub output: Vec<u8>,
    /// The last query's response.
    pub response: Vec<u8>,
    pub emissions: Vec<Message>,
    pub events: Vec<Vec<u8>>,
    pub siblings: BTreeMap<ProgramId, Sibling>,
    pub verifier: Option<Verifier>,
}

#[derive(Clone, Default)]
pub struct MockHost(Rc<RefCell<MockState>>);

impl MockHost {
    /// An execute's context over this host.
    pub fn exec(&self, env: Env) -> ExecCtx {
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
    pub fn attempt<T>(&self, write: impl FnOnce() -> Result<T, Refusal>) -> Result<T, Refusal> {
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
    pub fn refused<T: std::fmt::Debug>(
        &self,
        write: impl FnOnce() -> Result<T, Refusal>,
    ) -> Refusal {
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

    /// One host call, as the real host answers it.
    pub(crate) fn serve(&self, op: HostOp) -> HostReply {
        if let HostOp::Query { program, request } = op {
            // The sibling leaves the map while it answers, so it may use
            // this host itself.
            let sibling = self.borrow_mut().siblings.remove(&program);
            return HostReply::Query(match sibling {
                Some(sibling) => {
                    let answer = sibling(&request);
                    self.borrow_mut().siblings.insert(program, sibling);
                    answer
                }
                None => Err(Refusal::new(reason::UNKNOWN_PROGRAM, program)),
            });
        }
        let mut mock = self.borrow_mut();
        match op {
            HostOp::Get(key) | HostOp::CommittedGet(key) => {
                HostReply::Value(mock.state.get(&key).cloned())
            }
            HostOp::Scan(scan) | HostOp::CommittedScan(scan) => {
                HostReply::Entries(mock.scan_state(&scan))
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
                None => HostReply::Refused(Refusal::new(
                    reason::UNSUPPORTED,
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
                Err(refusal) => HostReply::Refused(refusal),
            },
            HostOp::Emit(message) => {
                mock.emissions.push(message);
                HostReply::Item(ItemRef {
                    source: String::new(),
                    item: mock.emissions.len() as u64,
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

impl MockState {
    fn scan_state(&self, scan: &Scan) -> Vec<Entry> {
        let admitted = self
            .state
            .iter()
            .filter(|(key, _)| scan.admits(key))
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
pub fn blob_id(hash: HashKind, kind: &str, body: &[u8]) -> Result<BlobId, Refusal> {
    let kind_is_a_word = !kind.is_empty() && !kind.contains([' ', '\0']);
    if !kind_is_a_word {
        return Err(Refusal::new(
            reason::INVALID_INPUT,
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
