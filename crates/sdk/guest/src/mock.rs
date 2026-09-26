//! The host a native test runs a module over: maps for state and blobs, and
//! what the module sent (`output`, `response`, `emissions`, `events`) kept
//! for the test to read; a harness runs the emissions itself, as the kernel
//! runs them once the handler returns. Sibling modules answer through `siblings`;
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

use crate::{Env, Error, ExecCtx, ModuleId, Order, QueryCtx, Range, Roles, code};

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
    /// message is kept, numbered in order, for the harness to run once the
    /// handler returns.
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
                let item = mock.emissions.len() as u64;
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
