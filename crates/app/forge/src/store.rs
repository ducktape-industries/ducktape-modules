// git::Objects over the sandbox's blob store: a git object's blob id is its oid, so no map sits between them.

use std::cell::Cell;

use abi::{BlobHeader, Refusal};
use gitcore::{Error, Hash, Kind, Object, Objects, Oid};

use crate::sandbox::{Sandbox, blob_id_of, hash_kind_of, oid_of_blob};

pub struct Store<'a, S: Sandbox> {
    sandbox: &'a S,
    hash: Hash,
    refused: Cell<Option<Refusal>>,
    budget: Option<Cell<(u64, u64)>>,
    max_object_size: u64,
}

impl<'a, S: Sandbox> Store<'a, S> {
    pub fn new(sandbox: &'a S, hash: Hash) -> Store<'a, S> {
        Store {
            sandbox,
            hash,
            refused: Cell::new(None),
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

    pub fn refusal(&self) -> Option<Refusal> {
        self.refused.take()
    }
}

impl<S: Sandbox> Objects for Store<'_, S> {
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
        let Some(blob) = self.sandbox.blob_get(blob_id_of(id)) else {
            return Ok(None);
        };
        let kind = Kind::parse(blob.kind.as_bytes())?;
        Ok(Some(Object::new(kind, blob.body)))
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.sandbox.blob_stat(blob_id_of(id)).is_some())
    }

    fn put(&mut self, kind: Kind, body: &[u8]) -> gitcore::Result<Oid> {
        // Compare may compute a prospective tree off consensus. Never persist it.
        if self.budget.is_some() {
            return gitcore::oid_of(self.hash, kind, body);
        }
        match self
            .sandbox
            .blob_put(hash_kind_of(self.hash), kind.as_str(), body.to_vec())
        {
            Ok(id) => Ok(oid_of_blob(&id)),
            Err(refusal) => {
                self.refused.set(Some(refusal));
                Err(Error::Storage)
            }
        }
    }
}
