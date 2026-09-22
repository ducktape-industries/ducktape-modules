// The slice of the ducktape sandbox forge uses, as one trait; the guest implements it over the call's context, MemorySandbox over maps for tests, with chat as its one sibling program.

use std::cell::RefCell;
use std::collections::BTreeMap;

use abi::{Blob, BlobHeader, BlobId, Entry, HashKind, Refusal, Scan, reason};
use gitcore::{Kind, Oid, oid_of};

pub trait Sandbox {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn set(&self, key: Vec<u8>, value: Vec<u8>);
    fn delete(&self, key: Vec<u8>);
    fn scan(&self, scan: Scan) -> Vec<Entry>;
    fn blob_put(&self, hash: HashKind, kind: &str, body: Vec<u8>) -> Result<BlobId, Refusal>;
    fn blob_get(&self, id: BlobId) -> Option<Blob>;
    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader>;
    fn emit(&self, target: &str, payload: Vec<u8>);
    fn query(&self, target: &str, request: Vec<u8>) -> Result<Vec<u8>, Refusal>;
    fn output(&self, bytes: Vec<u8>);
    fn respond(&self, bytes: Vec<u8>);
}

/// Chat's store, the sibling forge queries and its emissions land in.
#[derive(Default)]
pub struct ChatStore(BTreeMap<Vec<u8>, Vec<u8>>);

impl chat::Read for ChatStore {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
    fn scan(&self, scan: Scan) -> Vec<Entry> {
        scan_map(&self.0, &scan)
    }
}

impl chat::Write for ChatStore {
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.0.insert(key, value);
    }
    fn delete(&mut self, key: &[u8]) {
        self.0.remove(key);
    }
}

fn scan_map(map: &BTreeMap<Vec<u8>, Vec<u8>>, scan: &Scan) -> Vec<Entry> {
    let admitted = map
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

#[derive(Default)]
pub struct MemorySandbox {
    state: RefCell<BTreeMap<Vec<u8>, Vec<u8>>>,
    blobs: RefCell<BTreeMap<BlobId, Blob>>,
    output: RefCell<Vec<u8>>,
    response: RefCell<Vec<u8>>,
    emissions: RefCell<Vec<abi::Message>>,
    chat: RefCell<ChatStore>,
}

impl MemorySandbox {
    pub fn take_emissions(&self) -> Vec<abi::Message> {
        std::mem::take(&mut self.emissions.borrow_mut())
    }

    /// Runs one chat message directly, as a key or module would in its own block.
    pub fn chat_execute(&self, frame: &chat::Frame, msg: chat::ChatMsg) -> Result<(), Refusal> {
        chat::execute(&mut *self.chat.borrow_mut(), frame, msg)
    }

    pub fn chat_query(&self, q: chat::ChatViewQuery) -> Result<chat::ChatViewReply, Refusal> {
        chat::query(&*self.chat.borrow(), q)
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&self, height: u64, time: u64) -> Vec<Result<(), Refusal>> {
        let frame = chat::Frame {
            party: chat::Party::Module("forge".into()),
            height,
            time,
        };
        self.take_emissions()
            .into_iter()
            .map(|m| {
                if m.target != "chat" {
                    return Err(Refusal::new(reason::UNKNOWN_PROGRAM, m.target));
                }
                let msg = serde_json::from_slice(&m.payload)
                    .map_err(|e| Refusal::new(reason::PROTOCOL, e.to_string()))?;
                self.chat_execute(&frame, msg)
            })
            .collect()
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
        scan_map(&self.state.borrow(), &scan)
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

    fn emit(&self, target: &str, payload: Vec<u8>) {
        self.emissions.borrow_mut().push(abi::Message {
            target: target.into(),
            payload,
            reply: false,
        });
    }

    fn query(&self, target: &str, request: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        if target != "chat" {
            return Err(Refusal::new(reason::UNKNOWN_PROGRAM, target));
        }
        let q = serde_json::from_slice(&request)
            .map_err(|e| Refusal::new(reason::PROTOCOL, e.to_string()))?;
        let reply = self.chat_query(q)?;
        Ok(serde_json::to_vec(&reply).expect("a chat reply serializes"))
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
