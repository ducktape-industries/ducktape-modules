// The slice of the ducktape sandbox forge uses, as one trait; the guest implements it over host calls, MemorySandbox over maps for tests.

use std::cell::RefCell;
use std::collections::BTreeMap;

use abi::{Blob, BlobHeader, BlobId, Entry, Env, HashKind, Refusal, Scan, reason};
use gitcore::{Kind, Oid, oid_of};

pub trait Sandbox {
    fn env(&self) -> Env;
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn set(&self, key: Vec<u8>, value: Vec<u8>);
    fn delete(&self, key: Vec<u8>);
    fn scan(&self, scan: Scan) -> Vec<Entry>;
    fn blob_put(&self, hash: HashKind, kind: &str, body: Vec<u8>) -> Result<BlobId, Refusal>;
    fn blob_get(&self, id: BlobId) -> Option<Blob>;
    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader>;
    fn output(&self, bytes: Vec<u8>);
    fn respond(&self, bytes: Vec<u8>);
}

pub struct MemorySandbox {
    env: RefCell<Env>,
    state: RefCell<BTreeMap<Vec<u8>, Vec<u8>>>,
    blobs: RefCell<BTreeMap<BlobId, Blob>>,
    output: RefCell<Vec<u8>>,
    response: RefCell<Vec<u8>>,
}

impl MemorySandbox {
    pub fn new(env: Env) -> MemorySandbox {
        MemorySandbox {
            env: RefCell::new(env),
            state: RefCell::new(BTreeMap::new()),
            blobs: RefCell::new(BTreeMap::new()),
            output: RefCell::new(Vec::new()),
            response: RefCell::new(Vec::new()),
        }
    }

    pub fn set_env(&self, env: Env) {
        *self.env.borrow_mut() = env;
    }

    pub fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.output.borrow_mut())
    }

    pub fn take_response(&self) -> Vec<u8> {
        std::mem::take(&mut self.response.borrow_mut())
    }

    pub fn blob_count(&self) -> usize {
        self.blobs.borrow().len()
    }

    pub fn state_snapshot(&self) -> BTreeMap<Vec<u8>, Vec<u8>> {
        self.state.borrow().clone()
    }
}

impl Sandbox for MemorySandbox {
    fn env(&self) -> Env {
        self.env.borrow().clone()
    }

    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.state.borrow().get(key).cloned()
    }

    fn set(&self, key: Vec<u8>, value: Vec<u8>) {
        self.state.borrow_mut().insert(key, value);
    }

    fn delete(&self, key: Vec<u8>) {
        self.state.borrow_mut().remove(&key);
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        let state = self.state.borrow();
        let admitted = state
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

    fn blob_put(&self, hash: HashKind, kind: &str, body: Vec<u8>) -> Result<BlobId, Refusal> {
        let git_kind = Kind::parse(kind.as_bytes()).map_err(|_| {
            Refusal::new(
                reason::UNSUPPORTED,
                "the memory sandbox frames git kinds only",
            )
        })?;
        let oid = oid_of(hash_of(hash), git_kind, &body)
            .map_err(|error| Refusal::new(reason::INVALID_INPUT, error.to_string()))?;
        let id = blob_id_of(&oid);
        self.blobs.borrow_mut().insert(
            id,
            Blob {
                kind: kind.to_owned(),
                body,
            },
        );
        Ok(id)
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        self.blobs.borrow().get(&id).cloned()
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        self.blobs.borrow().get(&id).map(|blob| BlobHeader {
            kind: blob.kind.clone(),
            len: blob.body.len() as u64,
        })
    }

    fn output(&self, bytes: Vec<u8>) {
        *self.output.borrow_mut() = bytes;
    }

    fn respond(&self, bytes: Vec<u8>) {
        self.response.borrow_mut().extend_from_slice(&bytes);
    }
}

pub fn hash_of(kind: HashKind) -> gitcore::Hash {
    match kind {
        HashKind::Sha1 => gitcore::Hash::Sha1,
        HashKind::Sha256 => gitcore::Hash::Sha256,
    }
}

pub fn hash_kind_of(hash: gitcore::Hash) -> HashKind {
    match hash {
        gitcore::Hash::Sha1 => HashKind::Sha1,
        gitcore::Hash::Sha256 => HashKind::Sha256,
    }
}

pub fn blob_id_of(oid: &Oid) -> BlobId {
    match oid {
        Oid::Sha1(digest) => BlobId::Sha1(*digest),
        Oid::Sha256(digest) => BlobId::Sha256(*digest),
    }
}

pub fn oid_of_blob(id: &BlobId) -> Oid {
    match id {
        BlobId::Sha1(digest) => Oid::Sha1(*digest),
        BlobId::Sha256(digest) => Oid::Sha256(*digest),
    }
}
