// git::Objects over the store's blobs: a git object's blob id is its oid, so no map sits between them.

use std::cell::Cell;

use abi::{BlobHeader, BlobId, HashKind, Refusal};
use gitcore::{Error, Hash, Kind, Object, Objects, Oid};
use store::{Reads, Writes, not_found};

/// A different serving node or object replication can satisfy this query.
pub(crate) fn object_not_held(oid: impl std::fmt::Display) -> Refusal {
    not_found(format!("object {oid} is not held by this node"))
}

/// Reads objects; a `put` computes the id and persists nothing (a compare
/// may build a prospective tree off consensus). A push writes through
/// [`ObjectWriter`].
pub struct ObjectStore<'a, S: Reads> {
    sandbox: &'a S,
    hash: Hash,
    budget: Option<Cell<(u64, u64)>>,
    max_object_size: u64,
}

impl<'a, S: Reads> ObjectStore<'a, S> {
    pub fn new(sandbox: &'a S, hash: Hash) -> ObjectStore<'a, S> {
        ObjectStore {
            sandbox,
            hash,
            budget: None,
            max_object_size: u64::MAX,
        }
    }

    pub fn querying(sandbox: &'a S, hash: Hash, bounds: &crate::Bounds, reads: u64) -> Self {
        Self {
            budget: Some(Cell::new((reads, bounds.diff_bytes))),
            max_object_size: bounds.max_object_size,
            ..Self::new(sandbox, hash)
        }
    }

    pub fn header(&self, id: &Oid) -> gitcore::Result<BlobHeader> {
        self.sandbox
            .blob_stat(blob_id_of(id))
            .ok_or(Error::MissingObject(*id))
    }
}

impl<S: Reads> Objects for ObjectStore<'_, S> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
        if let Some(budget) = &self.budget {
            let header = match self.sandbox.blob_stat(blob_id_of(id)) {
                Some(header) => header,
                None => return Ok(None),
            };
            let (reads, bytes) = budget.get();
            if reads == 0 || header.len > bytes || header.len > self.max_object_size {
                return Err(Error::CapReached);
            }
            budget.set((reads - 1, bytes - header.len));
        }
        get(self.sandbox, id)
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.sandbox.blob_stat(blob_id_of(id)).is_some())
    }

    fn put(&mut self, kind: Kind, body: &[u8]) -> gitcore::Result<Oid> {
        gitcore::oid_of(self.hash, kind, body)
    }
}

fn get(sandbox: &impl Reads, id: &Oid) -> gitcore::Result<Option<Object>> {
    let Some(blob) = sandbox.blob_get(blob_id_of(id)) else {
        return Ok(None);
    };
    let kind = Kind::parse(blob.kind.as_bytes())?;
    Ok(Some(Object::new(kind, blob.body)))
}

/// The object store a push writes into; a refused blob write is kept for
/// the caller to hand back (`gitcore` sees only `Error::Storage`).
pub struct ObjectWriter<'a, S: Writes> {
    sandbox: &'a mut S,
    hash: Hash,
    pub refused: Option<Refusal>,
}

impl<'a, S: Writes> ObjectWriter<'a, S> {
    pub fn new(sandbox: &'a mut S, hash: Hash) -> Self {
        ObjectWriter {
            sandbox,
            hash,
            refused: None,
        }
    }
}

impl<S: Writes> Objects for ObjectWriter<'_, S> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
        get(&*self.sandbox, id)
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.sandbox.blob_stat(blob_id_of(id)).is_some())
    }

    fn put(&mut self, kind: Kind, body: &[u8]) -> gitcore::Result<Oid> {
        match self
            .sandbox
            .blob_put(hash_kind_of(self.hash), kind.as_str(), body.to_vec())
        {
            Ok(id) => Ok(oid_of_blob(&id)),
            Err(refusal) => {
                self.refused = Some(refusal);
                Err(Error::Storage)
            }
        }
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
