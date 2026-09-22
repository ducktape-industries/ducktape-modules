// The program's state layout: the founding bounds, one record per repository, its writers, and its refs, all under prefixed keys.

use std::collections::BTreeMap;

use abi::{Refusal, Scan, reason};
use crate::git::{Hash, Oid};

use crate::contract::{Bounds, Repo, valid_repo_name};
use crate::sandbox::{Sandbox, hash_of};

const BOUNDS: &[u8] = b"bounds";
const REPO: &str = "p/";
const WRITER: &str = "w/";
const REF: &str = "r/";

pub fn repo_key(name: &str) -> Vec<u8> {
    format!("{REPO}{name}").into_bytes()
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
    if let Some(old) = sandbox
        .get(&repo_key(name))
        .and_then(|b| abi::decode::<Repo>(&b).ok())
    {
        sandbox.delete(activity_key(name, old.last_activity));
    }
    sandbox.set(
        activity_key(name, repo.last_activity),
        name.as_bytes().to_vec(),
    );
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

pub fn activity_key(name: &str, height: u64) -> Vec<u8> {
    [
        b"a/".as_slice(),
        &(u64::MAX - height).to_be_bytes(),
        b"/",
        name.as_bytes(),
    ]
    .concat()
}

pub fn load_ref<S: Sandbox>(
    sandbox: &S,
    name: &str,
    reference: &[u8],
    hash: Hash,
) -> Result<Option<Oid>, Refusal> {
    sandbox
        .get(&ref_key(name, reference))
        .map(|b| Oid::from_bytes(hash, &b).map_err(|e| crate::refuse::storage(e.to_string())))
        .transpose()
}

/// Resolving an op's endpoint reads consensus refs only, never objects.
pub fn resolve<S: Sandbox>(
    sandbox: &S,
    name: &str,
    revision: &crate::Revision,
    hash: Hash,
) -> Result<Oid, Refusal> {
    match revision {
        crate::Revision::Oid(hex) => parse_oid(hash, hex),
        crate::Revision::Ref(reference) => {
            if !crate::git::server::valid_ref_name(reference) {
                return Err(crate::refuse::invalid("revision must name a full ref"));
            }
            load_ref(sandbox, name, reference, hash)?
                .ok_or_else(|| crate::refuse::not_found("the ref does not exist"))
        }
    }
}

pub fn parse_oid(hash: Hash, hex: &str) -> Result<Oid, Refusal> {
    let oid = Oid::from_hex(hash, hex)
        .map_err(|_| crate::refuse::invalid("oid has the wrong length or hex for this repo"))?;
    if oid.is_zero() {
        return Err(crate::refuse::invalid("an object id cannot be zero"));
    }
    Ok(oid)
}
