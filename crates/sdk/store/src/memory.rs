//! The store a test runs a program's rules over natively: maps for state and
//! blobs, and what the rules sent (`output`, `emissions`, `events`) kept for
//! the test to read. Sibling programs answer through `siblings`; signatures
//! verify through `verifier` (none set: verification is refused).

use std::collections::BTreeMap;

use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, ItemRef,
    Message, ProgramId, Refusal, Scan, Scheme, reason,
};
use sha1::Digest as _;

use crate::{Reads, Writes};

pub type Sibling = Box<dyn Fn(&[u8]) -> Result<Vec<u8>, Refusal>>;
pub type Verifier = Box<dyn Fn(Scheme, &[u8], &[u8], &[u8], &[u8]) -> bool>;

#[derive(Default)]
pub struct Memory {
    pub state: BTreeMap<Vec<u8>, Vec<u8>>,
    pub blobs: BTreeMap<BlobId, Blob>,
    /// The last `output`.
    pub output: Vec<u8>,
    pub emissions: Vec<Message>,
    pub events: Vec<Vec<u8>>,
    pub siblings: BTreeMap<ProgramId, Sibling>,
    pub verifier: Option<Verifier>,
}

impl Memory {
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub fn take_emissions(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.emissions)
    }

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

impl Reads for Memory {
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
            HostOp::Query { program, request } => {
                HostReply::Query(match self.siblings.get(program) {
                    Some(sibling) => sibling(request),
                    None => Err(Refusal::new(reason::UNKNOWN_PROGRAM, program.clone())),
                })
            }
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
                None => HostReply::Refused(Refusal::new(
                    reason::UNSUPPORTED,
                    "Memory verifies nothing until a verifier is set",
                )),
            },
            other => panic!("Memory serves reads through `host`; {other:?} goes through `Writes`"),
        }
    }
}

impl Writes for Memory {
    fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.state.insert(key.into(), value.into());
    }

    fn delete(&mut self, key: impl Into<Vec<u8>>) {
        self.state.remove(&key.into());
    }

    fn blob_put(
        &mut self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Refusal> {
        let (kind, body) = (kind.into(), body.into());
        let id = blob_id(hash, &kind, &body)?;
        self.blobs.insert(id, Blob { kind, body });
        Ok(id)
    }

    fn emit(&mut self, target: impl Into<ProgramId>, payload: impl Into<Vec<u8>>) -> ItemRef {
        self.emissions.push(Message {
            target: target.into(),
            payload: payload.into(),
            reply: false,
        });
        ItemRef {
            source: String::new(),
            item: self.emissions.len() as u64,
        }
    }

    fn event(&mut self, payload: impl Into<Vec<u8>>) {
        self.events.push(payload.into());
    }

    fn output(&mut self, bytes: impl Into<Vec<u8>>) {
        self.output = bytes.into();
    }
}
