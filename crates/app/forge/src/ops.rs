// The execute path: who signed, which op, and the repository ops (create, configure, grant, revoke, push). Change ops live in `changes`.

use abi::{Env, HashKind, Origin, Refusal};
use gitcore::server::{Policy, RefUpdate};
use gitcore::{Error, Limits, server};
use store::{Reads, Writes, already_exists, capacity, decoded, invalid, unauthorized};

use crate::contract::{Bounds, MAX_KEY_BYTES, MAX_PATH_BYTES, Op, Repo, Settings, valid_repo_name};
use crate::objects::{ObjectWriter, object_not_held};
use crate::state::{
    WRITERS, delete_ref, is_writer, load_bounds, load_refs, load_repo, repo_exists, repo_hash,
    save_bounds, save_repo, set_ref, storage,
};

pub const PROGRAM: &str = "forge";

pub fn init(store: &mut impl Writes, params: &[u8]) -> Result<(), Refusal> {
    let bounds: Bounds = decoded(PROGRAM, "Bounds", params)?;
    let usable = bounds.page_size > 0
        && bounds.log_walk > 0
        && bounds.tree_walk > 0
        && bounds.blob_bytes > 0
        && bounds.record_bytes > 0
        && bounds.diff_bytes >= bounds.blob_bytes;
    if !usable {
        return Err(invalid(
            "bounds need a positive page size and positive read/record budgets; diff_bytes >= blob_bytes",
        ));
    }
    save_bounds(store, &bounds);
    Ok(())
}

pub fn execute(store: &mut impl Writes, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
    let actor = signer(env)?;
    let op: Op = decoded(PROGRAM, "Op", payload)?;
    let repo = op.repo().to_owned();
    match op {
        Op::Create { repo, hash } => create(store, actor, &repo, hash),
        Op::Configure { repo, settings } => configure(store, actor, &repo, settings),
        Op::Grant { repo, key } => grant(store, actor, &repo, key),
        Op::Revoke { repo, key } => revoke(store, actor, &repo, key),
        Op::Push { repo, request } => push(store, actor, &repo, &request),
        change => crate::changes::execute(store, env, actor, change),
    }?;
    touch(store, &repo, env.height)
}

/// The key that signed the op. Forge is written by people, never by a
/// program or the system.
fn signer(env: &Env) -> Result<&[u8], Refusal> {
    let Origin::External(actor) = &env.origin else {
        return Err(unauthorized("a repository op is signed by a member key"));
    };
    if actor.is_empty() {
        return Err(unauthorized("an external origin carries a key"));
    }
    Ok(actor)
}

/// Every accepted op marks its repository active at this height.
fn touch(store: &mut impl Writes, name: &str, height: u64) -> Result<(), Refusal> {
    let mut repo = load_repo(store, name)?;
    repo.last_activity = height;
    save_repo(store, name, &repo)
}

fn create(
    store: &mut impl Writes,
    actor: &[u8],
    name: &str,
    hash: HashKind,
) -> Result<(), Refusal> {
    if !valid_repo_name(name) {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    if repo_exists(store, name) {
        return Err(already_exists(format!("a repository named {name} exists")));
    }
    let repo = Repo {
        hash,
        owner: actor.to_vec(),
        settings: Settings::default(),
        refs_count: 0,
        last_activity: 0,
    };
    save_repo(store, name, &repo)
}

fn configure(
    store: &mut impl Writes,
    actor: &[u8],
    name: &str,
    settings: Settings,
) -> Result<(), Refusal> {
    let mut repo = load_repo(store, name)?;
    require_owner(&repo, actor)?;
    let head_is_a_ref =
        settings.head.len() <= MAX_PATH_BYTES && server::valid_ref_name(&settings.head);
    if !head_is_a_ref {
        return Err(invalid("head names a ref under refs/"));
    }
    repo.settings = settings;
    save_repo(store, name, &repo)
}

fn grant(store: &mut impl Writes, actor: &[u8], name: &str, key: Vec<u8>) -> Result<(), Refusal> {
    require_owner(&load_repo(store, name)?, actor)?;
    require_key(&key)?;
    WRITERS.insert(store, &(name.to_owned(), key));
    Ok(())
}

fn revoke(store: &mut impl Writes, actor: &[u8], name: &str, key: Vec<u8>) -> Result<(), Refusal> {
    require_owner(&load_repo(store, name)?, actor)?;
    require_key(&key)?;
    WRITERS.remove(store, &(name.to_owned(), key));
    Ok(())
}

/// A git receive-pack: the objects land as blobs, then each accepted ref
/// moves; git's own report is the op's output.
fn push(store: &mut impl Writes, actor: &[u8], name: &str, request: &[u8]) -> Result<(), Refusal> {
    let mut repo = load_repo(store, name)?;
    require_writer(store, name, &repo, actor)?;
    let bounds = load_bounds(store)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(store, name, hash)?;
    let policy = Policy {
        allow_force: repo.settings.allow_force,
        allow_delete: repo.settings.allow_delete,
    };
    let mut objects = ObjectWriter::new(store, hash);
    let outcome = server::push(
        &mut objects,
        &refs,
        request,
        hash,
        &limits_of(&bounds),
        &policy,
        cap(bounds.push_walk),
    )
    .map_err(|error| objects.refused.take().unwrap_or_else(|| refusal_of(error)))?;
    for (reference, update) in &outcome.moves {
        match update {
            RefUpdate::Set(target) => {
                if !refs.contains_key(reference) {
                    repo.refs_count += 1;
                }
                set_ref(store, name, reference, target);
            }
            RefUpdate::Delete => {
                repo.refs_count -= 1;
                delete_ref(store, name, reference);
            }
        }
    }
    save_repo(store, name, &repo)?;
    store.output(outcome.report);
    Ok(())
}

fn require_owner(repo: &Repo, actor: &[u8]) -> Result<(), Refusal> {
    if repo.owner != actor {
        return Err(unauthorized("only the owner changes a repository"));
    }
    Ok(())
}

pub(crate) fn require_writer(
    store: &impl Reads,
    name: &str,
    repo: &Repo,
    actor: &[u8],
) -> Result<(), Refusal> {
    let may_write = repo.owner == actor || is_writer(store, name, actor);
    if !may_write {
        return Err(unauthorized("only the owner and its writers push"));
    }
    Ok(())
}

fn require_key(key: &[u8]) -> Result<(), Refusal> {
    if key.is_empty() {
        return Err(invalid("a writer is named by its key"));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(capacity("a key is at most MAX_KEY_BYTES"));
    }
    Ok(())
}

pub fn limits_of(bounds: &Bounds) -> Limits {
    Limits {
        max_objects: cap(bounds.max_objects),
        max_delta_depth: cap(bounds.max_delta_depth),
        max_object_size: cap(bounds.max_object_size),
    }
}

pub fn cap(bound: u64) -> usize {
    usize::try_from(bound).unwrap_or(usize::MAX)
}

pub fn refusal_of(error: Error) -> Refusal {
    match error {
        Error::Storage => storage("the blob store refused a write"),
        Error::CapReached | Error::ObjectTooLarge => {
            capacity("query or operation exceeds its configured work/byte bound")
        }
        Error::MissingObject(id) | Error::MissingBase(id) => object_not_held(id),
        other => invalid(other.to_string()),
    }
}
