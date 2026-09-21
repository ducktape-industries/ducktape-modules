// The program's state layout: the founding bounds, one record per repository, its writers, and its refs, all under prefixed keys.

use std::collections::BTreeMap;

use abi::{Refusal, Scan, reason};
use gitcore::{Hash, Oid};

use crate::contract::{Bounds, Repo, valid_repo_name};
use crate::sandbox::{Sandbox, hash_of};

const BOUNDS: &[u8] = b"bounds";
const REPO: &str = "p/";
const WRITER: &str = "w/";
const REF: &str = "r/";

pub fn repos_prefix() -> Vec<u8> {
    REPO.as_bytes().to_vec()
}

pub fn repo_key(name: &str) -> Vec<u8> {
    format!("{REPO}{name}").into_bytes()
}

pub fn repo_name(key: &[u8]) -> Option<String> {
    let name = key.strip_prefix(REPO.as_bytes())?;
    String::from_utf8(name.to_vec()).ok()
}

pub fn writers_prefix(name: &str) -> Vec<u8> {
    format!("{WRITER}{name}/").into_bytes()
}

pub fn writer_key(name: &str, key: &[u8]) -> Vec<u8> {
    let mut out = writers_prefix(name);
    out.extend_from_slice(key);
    out
}

pub fn refs_prefix(name: &str) -> Vec<u8> {
    format!("{REF}{name}/").into_bytes()
}

pub fn ref_key(name: &str, reference: &[u8]) -> Vec<u8> {
    let mut out = refs_prefix(name);
    out.extend_from_slice(reference);
    out
}

pub fn save_bounds<S: Sandbox>(sandbox: &S, bounds: &Bounds) {
    sandbox.set(BOUNDS.to_vec(), abi::encode(bounds));
}

pub fn load_bounds<S: Sandbox>(sandbox: &S) -> Result<Bounds, Refusal> {
    let Some(bytes) = sandbox.get(BOUNDS) else {
        return Err(Refusal::new(
            reason::PROTOCOL,
            "the program was founded without bounds",
        ));
    };
    abi::decode(&bytes)
}

pub fn save_repo<S: Sandbox>(sandbox: &S, name: &str, repo: &Repo) {
    sandbox.set(repo_key(name), abi::encode(repo));
}

pub fn load_repo<S: Sandbox>(sandbox: &S, name: &str) -> Result<Repo, Refusal> {
    let well_formed = valid_repo_name(name);
    if !well_formed {
        return Err(Refusal::new(
            reason::INVALID_INPUT,
            format!("{name:?} is not a repository name"),
        ));
    }
    let Some(bytes) = sandbox.get(&repo_key(name)) else {
        return Err(Refusal::new(
            reason::NOT_FOUND,
            format!("no repository named {name}"),
        ));
    };
    abi::decode(&bytes)
}

pub fn repo_exists<S: Sandbox>(sandbox: &S, name: &str) -> bool {
    sandbox.get(&repo_key(name)).is_some()
}

pub fn is_writer<S: Sandbox>(sandbox: &S, name: &str, key: &[u8]) -> bool {
    sandbox.get(&writer_key(name, key)).is_some()
}

pub fn load_refs<S: Sandbox>(
    sandbox: &S,
    name: &str,
    hash: Hash,
) -> Result<BTreeMap<Vec<u8>, Oid>, Refusal> {
    let prefix = refs_prefix(name);
    sandbox
        .scan(Scan::prefix(&prefix))
        .into_iter()
        .map(|entry| {
            let reference = entry.key[prefix.len()..].to_vec();
            let target = Oid::from_bytes(hash, &entry.value).map_err(|error| {
                Refusal::new(reason::PROTOCOL, format!("ref {reference:?} holds {error}"))
            })?;
            Ok((reference, target))
        })
        .collect()
}

pub fn set_ref<S: Sandbox>(sandbox: &S, name: &str, reference: &[u8], target: &Oid) {
    sandbox.set(ref_key(name, reference), target.as_bytes().to_vec());
}

pub fn delete_ref<S: Sandbox>(sandbox: &S, name: &str, reference: &[u8]) {
    sandbox.delete(ref_key(name, reference));
}

pub fn repo_hash(repo: &Repo) -> Hash {
    hash_of(repo.hash)
}
