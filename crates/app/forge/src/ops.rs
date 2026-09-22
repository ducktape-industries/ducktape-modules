// The execute path: repository lifecycle, access, a git push and a merge, each a straight walk over the sandbox.

use abi::{Env, Origin, Refusal};
use crate::git::server::{Policy, RefUpdate};
use crate::git::{Error, Limits, server};

use crate::contract::{Bounds, Op, Repo, Settings, valid_repo_name};
use crate::refuse::{already_exists, capacity, invalid, storage, unauthorized};
use crate::repo::{
    delete_ref, is_writer, load_bounds, load_refs, load_repo, repo_exists, repo_hash, save_bounds,
    save_repo, set_ref, writer_key,
};
use crate::sandbox::Sandbox;
use crate::store::Store;

pub fn init<S: Sandbox>(sandbox: &S, params: &[u8]) -> Result<(), Refusal> {
    let bounds: Bounds = abi::decode(params)?;
    if bounds.page_size == 0
        || bounds.log_walk == 0
        || bounds.tree_walk == 0
        || bounds.blob_bytes == 0
        || bounds.record_bytes == 0
        || bounds.diff_bytes < bounds.blob_bytes
    {
        return Err(invalid(
            "bounds need a positive page size and positive read/record budgets; diff_bytes >= blob_bytes",
        ));
    }
    save_bounds(sandbox, &bounds);
    Ok(())
}

pub fn execute<S: Sandbox>(sandbox: &S, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
    let Origin::External(actor) = &env.origin else {
        return Err(unauthorized("a repository op is signed by a member key"));
    };
    if actor.is_empty() {
        return Err(unauthorized("an external origin carries a key"));
    }
    let op: Op = abi::decode(payload)?;
    let name = match &op {
        Op::Create { repo, .. }
        | Op::Configure { repo, .. }
        | Op::Grant { repo, .. }
        | Op::Revoke { repo, .. }
        | Op::Push { repo, .. }
        | Op::Merge { repo, .. }
        | Op::ChangeOpen { repo, .. }
        | Op::ChangeEdit { repo, .. }
        | Op::ChangeClose { repo, .. }
        | Op::ReviewSubmit { repo, .. } => repo.clone(),
    };
    match op {
        Op::Create { repo, hash } => create(sandbox, actor, &repo, hash),
        Op::Configure { repo, settings } => configure(sandbox, actor, &repo, settings),
        Op::Grant { repo, key } => grant(sandbox, actor, &repo, &key),
        Op::Revoke { repo, key } => revoke(sandbox, actor, &repo, &key),
        Op::Push { repo, request } => push(sandbox, actor, &repo, &request),
        op => crate::changes::execute(sandbox, env, actor, op),
    }?;
    let mut repo = load_repo(sandbox, &name)?;
    repo.last_activity = env.height;
    save_repo(sandbox, &name, &repo);
    Ok(())
}

fn create<S: Sandbox>(
    sandbox: &S,
    actor: &[u8],
    name: &str,
    hash: abi::HashKind,
) -> Result<(), Refusal> {
    let well_formed = valid_repo_name(name);
    if !well_formed {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    let taken = repo_exists(sandbox, name);
    if taken {
        return Err(already_exists(format!("a repository named {name} exists")));
    }
    let repo = Repo {
        hash,
        owner: actor.to_vec(),
        settings: Settings::default(),
        refs_count: 0,
        last_activity: 0,
    };
    save_repo(sandbox, name, &repo);
    Ok(())
}

fn configure<S: Sandbox>(
    sandbox: &S,
    actor: &[u8],
    name: &str,
    settings: Settings,
) -> Result<(), Refusal> {
    let mut repo = load_repo(sandbox, name)?;
    require_owner(&repo, actor)?;
    let head_is_a_ref = server::valid_ref_name(&settings.head);
    if !head_is_a_ref {
        return Err(invalid("head names a ref under refs/"));
    }
    repo.settings = settings;
    save_repo(sandbox, name, &repo);
    Ok(())
}

fn grant<S: Sandbox>(sandbox: &S, actor: &[u8], name: &str, key: &[u8]) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    require_owner(&repo, actor)?;
    require_key(key)?;
    sandbox.set(writer_key(name, key), Vec::new());
    Ok(())
}

fn revoke<S: Sandbox>(sandbox: &S, actor: &[u8], name: &str, key: &[u8]) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    require_owner(&repo, actor)?;
    require_key(key)?;
    sandbox.delete(writer_key(name, key));
    Ok(())
}

fn push<S: Sandbox>(sandbox: &S, actor: &[u8], name: &str, request: &[u8]) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    require_writer(sandbox, name, &repo, actor)?;
    let bounds = load_bounds(sandbox)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(sandbox, name, hash)?;
    let policy = Policy {
        allow_force: repo.settings.allow_force,
        allow_delete: repo.settings.allow_delete,
    };
    let mut store = Store::new(sandbox, hash);
    let outcome = server::push(
        &mut store,
        &refs,
        request,
        hash,
        &limits_of(&bounds),
        &policy,
        cap(bounds.push_walk),
    )
    .map_err(|error| refusal_of(&store, error))?;
    let mut repo = repo;
    for (reference, update) in &outcome.moves {
        match update {
            RefUpdate::Set(_) if !refs.contains_key(reference) => repo.refs_count += 1,
            RefUpdate::Delete => repo.refs_count -= 1,
            _ => {}
        }
        match update {
            RefUpdate::Set(target) => set_ref(sandbox, name, reference, target),
            RefUpdate::Delete => delete_ref(sandbox, name, reference),
        }
    }
    save_repo(sandbox, name, &repo);
    sandbox.output(outcome.report);
    Ok(())
}

fn require_owner(repo: &Repo, actor: &[u8]) -> Result<(), Refusal> {
    let is_owner = repo.owner == actor;
    if !is_owner {
        return Err(unauthorized("only the owner changes a repository"));
    }
    Ok(())
}

pub(crate) fn require_writer<S: Sandbox>(
    sandbox: &S,
    name: &str,
    repo: &Repo,
    actor: &[u8],
) -> Result<(), Refusal> {
    let is_owner = repo.owner == actor;
    let is_writer = is_writer(sandbox, name, actor);
    let may_write = is_owner || is_writer;
    if !may_write {
        return Err(unauthorized("only the owner and its writers push"));
    }
    Ok(())
}

fn require_key(key: &[u8]) -> Result<(), Refusal> {
    let is_empty = key.is_empty();
    if is_empty {
        return Err(invalid("a writer is named by its key"));
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

pub fn refusal_of<S: Sandbox>(store: &Store<'_, S>, error: Error) -> Refusal {
    match error {
        Error::Storage => store
            .refusal()
            .unwrap_or_else(|| storage("the blob store refused a write")),
        Error::CapReached | Error::ObjectTooLarge => {
            capacity("query or operation exceeds its configured work/byte bound")
        }
        Error::MissingObject(id) | Error::MissingBase(id) => crate::refuse::object_not_held(id),
        other => invalid(other.to_string()),
    }
}
