// gitcore's Objects over the sandbox's blob store: a git object's blob id is its oid, so no map sits between them.

use std::cell::Cell;

use abi::Refusal;
use gitcore::{Error, Hash, Kind, Object, Objects, Oid};

use crate::sandbox::{Sandbox, blob_id_of, hash_kind_of, oid_of_blob};

pub struct Store<'a, S: Sandbox> {
    sandbox: &'a S,
    hash: Hash,
    refused: Cell<Option<Refusal>>,
}

impl<'a, S: Sandbox> Store<'a, S> {
    pub fn new(sandbox: &'a S, hash: Hash) -> Store<'a, S> {
        Store {
            sandbox,
            hash,
            refused: Cell::new(None),
        }
    }

    pub fn refusal(&self) -> Option<Refusal> {
        self.refused.take()
    }
}

impl<S: Sandbox> Objects for Store<'_, S> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
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
