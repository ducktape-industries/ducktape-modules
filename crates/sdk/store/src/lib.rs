//! What a program holds in its hands. `Reads` is the read half of the guest
//! context (`guest::Reads`'s surface, one required method: `host`);
//! `Writes` adds what an `Execute` may do. Both are implemented for `guest`'s
//! contexts behind `program` (wasm32) and for [`Memory`] natively, so a
//! program's rules are written once, generic over the store, and run in a
//! native test as they run on the host.

mod key;
#[cfg(not(target_arch = "wasm32"))]
mod memory;
mod page;
pub mod refuse;
mod table;

pub use key::KeyCodec;
#[cfg(not(target_arch = "wasm32"))]
pub use memory::{Memory, Sibling, Verifier, blob_id};
pub use page::{Page, PageReply};
pub use refuse::{
    already_exists, capacity, corrupt, decoded, invalid, not_found, stale, unauthorized,
    wrong_state,
};
pub use table::{Item, Map, Set};

use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, ItemRef,
    ProgramId, Refusal, Root, Scan, Scheme,
};
use borsh::{BorshDeserialize, BorshSerialize};

fn protocol(expected: &str, got: HostReply) -> ! {
    panic!("host answered {got:?} where {expected} was expected")
}

pub trait Reads {
    fn host(&self, op: &HostOp) -> HostReply;

    fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(&HostOp::Get(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        match self.host(&HostOp::Scan(scan)) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    fn committed_get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(&HostOp::CommittedGet(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    fn committed_scan(&self, scan: Scan) -> Vec<Entry> {
        match self.host(&HostOp::CommittedScan(scan)) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    fn record<T: BorshDeserialize>(&self, key: impl AsRef<[u8]>) -> Result<Option<T>, Refusal> {
        self.get(key).map(|bytes| abi::decode(&bytes)).transpose()
    }

    fn records<T: BorshDeserialize>(&self, scan: Scan) -> Result<Vec<(Vec<u8>, T)>, Refusal> {
        self.scan(scan)
            .into_iter()
            .map(|entry| Ok((entry.key, abi::decode(&entry.value)?)))
            .collect()
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        match self.host(&HostOp::BlobGet(id)) {
            HostReply::Blob(blob) => blob,
            other => protocol("blob", other),
        }
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        match self.host(&HostOp::BlobStat(id)) {
            HostReply::BlobHeader(header) => header,
            other => protocol("blob header", other),
        }
    }

    fn blob_read(&self, id: BlobId, offset: u64, len: u64) -> Option<Vec<u8>> {
        match self.host(&HostOp::BlobRead { id, offset, len }) {
            HostReply::Value(bytes) => bytes,
            other => protocol("value", other),
        }
    }

    fn root(&self, program: impl Into<ProgramId>) -> Option<Root> {
        match self.host(&HostOp::Root(program.into())) {
            HostReply::Root(root) => root,
            other => protocol("root", other),
        }
    }

    fn query(
        &self,
        program: impl Into<ProgramId>,
        request: impl Into<Vec<u8>>,
    ) -> Result<Vec<u8>, Refusal> {
        match self.host(&HostOp::Query {
            program: program.into(),
            request: request.into(),
        }) {
            HostReply::Query(answer) => answer,
            other => protocol("query answer", other),
        }
    }

    fn ask<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        program: impl Into<ProgramId>,
        request: &Q,
    ) -> Result<R, Refusal> {
        abi::decode(&self.query(program, abi::encode(request))?)
    }

    fn sha256(&self, bytes: impl Into<Vec<u8>>) -> [u8; 32] {
        match self.host(&HostOp::Crypto(CryptoOp::Sha256(bytes.into()))) {
            HostReply::Crypto(CryptoReply::Digest(digest)) => digest,
            other => protocol("digest", other),
        }
    }

    fn verify(
        &self,
        scheme: Scheme,
        key: impl Into<Vec<u8>>,
        namespace: impl Into<Vec<u8>>,
        message: impl Into<Vec<u8>>,
        signature: impl Into<Vec<u8>>,
    ) -> Result<bool, Refusal> {
        match self.host(&HostOp::Crypto(CryptoOp::Verify {
            scheme,
            key: key.into(),
            namespace: namespace.into(),
            message: message.into(),
            signature: signature.into(),
        })) {
            HostReply::Crypto(CryptoReply::Verified(valid)) => Ok(valid),
            HostReply::Refused(refusal) => Err(refusal),
            other => protocol("verdict", other),
        }
    }
}

/// What an execute may do beyond reading: the state and blob writes, and
/// what leaves the program (`emit`, `event`, `output`). A native `Memory`
/// records the latter for a test to read.
pub trait Writes: Reads {
    fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>);
    fn delete(&mut self, key: impl Into<Vec<u8>>);
    fn blob_put(
        &mut self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Refusal>;
    fn emit(&mut self, target: impl Into<ProgramId>, payload: impl Into<Vec<u8>>) -> ItemRef;
    fn event(&mut self, payload: impl Into<Vec<u8>>);
    fn output(&mut self, bytes: impl Into<Vec<u8>>);
}

#[cfg(all(feature = "program", target_arch = "wasm32"))]
mod guest_impls {
    use super::*;

    impl Reads for guest::Execute {
        fn host(&self, op: &HostOp) -> HostReply {
            guest::Reads::host(self, op)
        }
    }

    impl Reads for guest::Query {
        fn host(&self, op: &HostOp) -> HostReply {
            guest::Reads::host(self, op)
        }
    }

    impl Writes for guest::Execute {
        fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
            guest::Execute::set(self, key, value)
        }
        fn delete(&mut self, key: impl Into<Vec<u8>>) {
            guest::Execute::delete(self, key)
        }
        fn blob_put(
            &mut self,
            hash: HashKind,
            kind: impl Into<String>,
            body: impl Into<Vec<u8>>,
        ) -> Result<BlobId, Refusal> {
            guest::Execute::blob_put(self, hash, kind, body)
        }
        fn emit(&mut self, target: impl Into<ProgramId>, payload: impl Into<Vec<u8>>) -> ItemRef {
            guest::Execute::emit(self, target, payload)
        }
        fn event(&mut self, payload: impl Into<Vec<u8>>) {
            guest::Execute::event(self, payload)
        }
        fn output(&mut self, bytes: impl Into<Vec<u8>>) {
            guest::Execute::output(self, bytes)
        }
    }
}
