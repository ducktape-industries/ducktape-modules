// The wasm entry: the Sandbox over the guest's host calls, and the Program the runtime dispatches into.

use abi::{Blob, BlobHeader, BlobId, Entry, Env, HashKind, Refusal, Scan};
use guest::Program;

use crate::sandbox::Sandbox;

struct Guest;

impl Sandbox for Guest {
    fn env(&self) -> Env {
        guest::env()
    }

    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        guest::get(key)
    }

    fn set(&self, key: Vec<u8>, value: Vec<u8>) {
        guest::set(key, value)
    }

    fn delete(&self, key: Vec<u8>) {
        guest::delete(key)
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        guest::scan(scan)
    }

    fn blob_put(&self, hash: HashKind, kind: &str, body: Vec<u8>) -> Result<BlobId, Refusal> {
        guest::blob_put(hash, kind, body)
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        guest::blob_get(id)
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        guest::blob_stat(id)
    }

    fn output(&self, bytes: Vec<u8>) {
        guest::output(bytes)
    }

    fn respond(&self, bytes: Vec<u8>) {
        guest::respond(bytes)
    }
}

struct Forge;

impl Program for Forge {
    fn init(params: &[u8]) -> Result<(), Refusal> {
        crate::init(&Guest, params)
    }

    fn execute(payload: &[u8]) -> Result<(), Refusal> {
        crate::execute(&Guest, payload)
    }

    fn query(request: &[u8]) -> Result<(), Refusal> {
        crate::query(&Guest, request)
    }
}

guest::program!(Forge);
