// The wasm entry: the Sandbox over the call's context, and the Program the runtime dispatches into.

use std::cell::RefCell;

use abi::{Blob, BlobHeader, BlobId, Entry, Env, HashKind, Refusal, Scan};
use guest::{Execute, Program, Query, Reads};

use crate::sandbox::Sandbox;

struct Executing<'a>(RefCell<&'a mut Execute>);

impl Sandbox for Executing<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.borrow().get(key)
    }

    fn set(&self, key: Vec<u8>, value: Vec<u8>) {
        self.0.borrow_mut().set(key, value)
    }

    fn delete(&self, key: Vec<u8>) {
        self.0.borrow_mut().delete(key)
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.0.borrow().scan(scan)
    }

    fn blob_put(&self, hash: HashKind, kind: &str, body: Vec<u8>) -> Result<BlobId, Refusal> {
        self.0.borrow_mut().blob_put(hash, kind, body)
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        self.0.borrow().blob_get(id)
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        self.0.borrow().blob_stat(id)
    }

    fn output(&self, bytes: Vec<u8>) {
        self.0.borrow_mut().output(bytes)
    }

    fn respond(&self, _bytes: Vec<u8>) {
        unreachable!("an execute does not respond")
    }
}

struct Querying<'a>(RefCell<&'a mut Query>);

impl Sandbox for Querying<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.borrow().get(key)
    }

    fn set(&self, _key: Vec<u8>, _value: Vec<u8>) {
        unreachable!("a query does not write")
    }

    fn delete(&self, _key: Vec<u8>) {
        unreachable!("a query does not write")
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.0.borrow().scan(scan)
    }

    fn blob_put(&self, _hash: HashKind, _kind: &str, _body: Vec<u8>) -> Result<BlobId, Refusal> {
        unreachable!("a query does not write")
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        self.0.borrow().blob_get(id)
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        self.0.borrow().blob_stat(id)
    }

    fn output(&self, _bytes: Vec<u8>) {
        unreachable!("a query does not write")
    }

    fn respond(&self, bytes: Vec<u8>) {
        self.0.borrow_mut().respond(bytes)
    }
}

struct Forge;

impl Program for Forge {
    fn init(ctx: &mut Execute, _env: &Env, params: &[u8]) -> Result<(), Refusal> {
        crate::init(&Executing(RefCell::new(ctx)), params)
    }

    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        crate::execute(&Executing(RefCell::new(ctx)), env, payload)
    }

    fn query(ctx: &mut Query, _env: &Env, request: &[u8]) -> Result<(), Refusal> {
        crate::query(&Querying(RefCell::new(ctx)), request)
    }
}

guest::program!(Forge);
