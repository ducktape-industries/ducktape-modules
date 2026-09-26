//! The execute path: who acts, which op, and the repository ops (create,
//! configure, grant, revoke, push). Change ops live in `changes`.

use gitcore::server::{Policy, RefUpdate};
use gitcore::{Error as GitError, Limits, server};
use guest::{Error, HashKind};
use guest::{ExecCtx, QueryCtx, already_exists, capacity, decoded, invalid, unauthorized};

use crate::contract::{Bounds, MAX_PATH_BYTES, Principal, Repo, Settings, valid_repo_name};
use crate::objects::{ObjectWriter, object_not_held};
use crate::state::{
    WRITERS, delete_ref, is_writer, load_bounds, load_refs, load_repo, repo_exists, repo_hash,
    save_bounds, save_repo, set_ref, storage,
};

pub const MODULE: &str = "forge";

pub(crate) fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
    let bounds: Bounds = decoded(MODULE, "Bounds", params)?;
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
    save_bounds(ctx, &bounds);
    Ok(())
}

/// Forge is written by keys (a person's or an agent's account), never by a
/// module or the system: the frame is signed, and acts as the account its
/// key holds. A key that holds none, or whose account is not live, never
/// gets here: [`ExecCtx::sender`](guest::ExecCtx::sender) refuses it.
pub(crate) fn person(ctx: &ExecCtx) -> Result<Principal, Error> {
    ctx.env()
        .signer()
        .map_err(|_| unauthorized("a repository op is signed by a key"))?;
    ctx.sender()
}

/// Every accepted op marks its repository active at this height.
pub(crate) fn touch(ctx: &ExecCtx, name: &str, height: u64) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    repo.last_activity = height;
    save_repo(ctx, name, &repo)
}

pub(crate) fn create(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    hash: HashKind,
) -> Result<(), Error> {
    if !valid_repo_name(name) {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    if repo_exists(ctx, name) {
        return Err(already_exists(format!("a repository named {name} exists")));
    }
    let repo = Repo {
        hash,
        owner: actor.clone(),
        settings: Settings::default(),
        refs_count: 0,
        last_activity: 0,
    };
    save_repo(ctx, name, &repo)
}

pub(crate) fn configure(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    settings: Settings,
) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    require_owner(&repo, actor)?;
    let head_is_a_ref =
        settings.head.len() <= MAX_PATH_BYTES && server::valid_ref_name(&settings.head);
    if !head_is_a_ref {
        return Err(invalid("head names a ref under refs/"));
    }
    repo.settings = settings;
    save_repo(ctx, name, &repo)
}

pub(crate) fn grant(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    principal: Principal,
) -> Result<(), Error> {
    require_owner(&load_repo(ctx, name)?, actor)?;
    require_named(&principal)?;
    WRITERS.insert(ctx, &(name.to_owned(), principal));
    Ok(())
}

pub(crate) fn revoke(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    principal: Principal,
) -> Result<(), Error> {
    require_owner(&load_repo(ctx, name)?, actor)?;
    require_named(&principal)?;
    WRITERS.remove(ctx, &(name.to_owned(), principal));
    Ok(())
}

/// A git receive-pack: the objects land as blobs, then each accepted ref
/// moves; git's own report is the op's output.
pub(crate) fn push(
    ctx: &ExecCtx,
    actor: &Principal,
    name: &str,
    request: &[u8],
) -> Result<(), Error> {
    let mut repo = load_repo(ctx, name)?;
    require_writer(ctx, name, &repo, actor)?;
    let bounds = load_bounds(ctx)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(ctx, name, hash)?;
    let policy = Policy {
        allow_force: repo.settings.allow_force,
        allow_delete: repo.settings.allow_delete,
    };
    let mut objects = ObjectWriter::new(ctx, hash);
    let outcome = server::push(
        &mut objects,
        &refs,
        request,
        hash,
        &limits_of(&bounds),
        &policy,
        cap(bounds.push_walk),
    )
    .map_err(refusal_of)?;
    objects.flush()?;
    for (reference, update) in &outcome.moves {
        match update {
            RefUpdate::Set(target) => {
                if !refs.contains_key(reference) {
                    repo.refs_count += 1;
                }
                set_ref(ctx, name, reference, target);
            }
            RefUpdate::Delete => {
                repo.refs_count -= 1;
                delete_ref(ctx, name, reference);
            }
        }
    }
    save_repo(ctx, name, &repo)?;
    ctx.set_return_data(outcome.report);
    Ok(())
}

fn require_owner(repo: &Repo, actor: &Principal) -> Result<(), Error> {
    if repo.owner != *actor {
        return Err(unauthorized("only the owner changes a repository"));
    }
    Ok(())
}

pub(crate) fn require_writer(
    ctx: &QueryCtx,
    name: &str,
    repo: &Repo,
    actor: &Principal,
) -> Result<(), Error> {
    let may_write = repo.owner == *actor || is_writer(ctx, name, actor);
    if !may_write {
        return Err(unauthorized("only the owner and its writers push"));
    }
    Ok(())
}

/// Whom an op names (a writer, a reviewer): an account, never the system.
pub(crate) fn require_named(principal: &Principal) -> Result<(), Error> {
    if principal.account().is_none() {
        return Err(invalid("only an account is named here"));
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

pub fn refusal_of(error: GitError) -> Error {
    match error {
        GitError::Storage => storage("the blob ctx refused a write"),
        GitError::CapReached | GitError::ObjectTooLarge => {
            capacity("query or operation exceeds its configured work/byte bound")
        }
        GitError::MissingObject(id) | GitError::MissingBase(id) => object_not_held(id),
        other => invalid(other.to_string()),
    }
}
