// The execute path: repository lifecycle, access, a git push and a merge, each a straight walk over the sandbox.

use abi::{Origin, Refusal};
use gitcore::merge::{MergeOutcome, merge_base, merge_trees};
use gitcore::server::{Policy, RefUpdate};
use gitcore::{Commit, Error, Kind, Limits, Objects, Oid, Signature, server};

use crate::contract::{Bounds, Op, Repo, Settings, valid_repo_name};
use crate::refuse::{
    already_exists, capacity, invalid, not_found, storage, unauthorized, wrong_state,
};
use crate::repo::{
    delete_ref, is_writer, load_bounds, load_refs, load_repo, repo_exists, repo_hash, save_bounds,
    save_repo, set_ref, writer_key,
};
use crate::sandbox::Sandbox;
use crate::store::Store;

pub fn init<S: Sandbox>(sandbox: &S, params: &[u8]) -> Result<(), Refusal> {
    let bounds: Bounds = abi::decode(params)?;
    save_bounds(sandbox, &bounds);
    Ok(())
}

pub fn execute<S: Sandbox>(sandbox: &S, payload: &[u8]) -> Result<(), Refusal> {
    let env = sandbox.env();
    let Origin::External(actor) = env.origin else {
        return Err(unauthorized("a repository op is signed by a member key"));
    };
    match abi::decode(payload)? {
        Op::Create { repo, hash } => create(sandbox, &actor, &repo, hash),
        Op::Configure { repo, settings } => configure(sandbox, &actor, &repo, settings),
        Op::Grant { repo, key } => grant(sandbox, &actor, &repo, &key),
        Op::Revoke { repo, key } => revoke(sandbox, &actor, &repo, &key),
        Op::Push { repo, request } => push(sandbox, &actor, &repo, &request),
        Op::Merge {
            repo,
            into,
            from,
            message,
        } => merge(sandbox, &actor, env.time, &repo, &into, &from, &message),
    }
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
    for (reference, update) in &outcome.moves {
        match update {
            RefUpdate::Set(target) => set_ref(sandbox, name, reference, target),
            RefUpdate::Delete => delete_ref(sandbox, name, reference),
        }
    }
    sandbox.output(outcome.report);
    Ok(())
}

enum Merged {
    Nothing,
    Unrelated,
    FastForward(Oid),
    Commit(Oid),
    Conflicts(Vec<Vec<u8>>),
}

fn merge<S: Sandbox>(
    sandbox: &S,
    actor: &[u8],
    time: u64,
    name: &str,
    into: &[u8],
    from: &[u8],
    message: &[u8],
) -> Result<(), Refusal> {
    let repo = load_repo(sandbox, name)?;
    require_writer(sandbox, name, &repo, actor)?;
    let bounds = load_bounds(sandbox)?;
    let hash = repo_hash(&repo);
    let refs = load_refs(sandbox, name, hash)?;
    let Some(ours) = refs.get(into).copied() else {
        return Err(not_found(format!(
            "no ref {}",
            String::from_utf8_lossy(into)
        )));
    };
    let Some(theirs) = refs.get(from).copied() else {
        return Err(not_found(format!(
            "no ref {}",
            String::from_utf8_lossy(from)
        )));
    };
    let mut store = Store::new(sandbox, hash);
    let author = Signature {
        name: abi::hex(actor).into_bytes(),
        email: Vec::new(),
        time: time as i64,
        offset_minutes: 0,
    };
    let merged = merge_commits(&mut store, &bounds, ours, theirs, author, message)
        .map_err(|error| refusal_of(&store, error))?;
    match merged {
        Merged::Nothing => Err(wrong_state(format!(
            "{} is already in {}",
            String::from_utf8_lossy(from),
            String::from_utf8_lossy(into)
        ))),
        Merged::Unrelated => Err(wrong_state("the two histories are unrelated")),
        Merged::Conflicts(paths) => Err(wrong_state(format!(
            "conflicts in {}",
            paths
                .iter()
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
        Merged::FastForward(target) | Merged::Commit(target) => {
            set_ref(sandbox, name, into, &target);
            sandbox.output(target.to_hex().into_bytes());
            Ok(())
        }
    }
}

fn merge_commits<O: Objects>(
    store: &mut O,
    bounds: &Bounds,
    ours: Oid,
    theirs: Oid,
    author: Signature,
    message: &[u8],
) -> gitcore::Result<Merged> {
    let Some(base) = merge_base(store, &ours, &theirs, cap(bounds.push_walk))? else {
        return Ok(Merged::Unrelated);
    };
    let theirs_already_in_ours = base == theirs;
    if theirs_already_in_ours {
        return Ok(Merged::Nothing);
    }
    let ours_behind_theirs = base == ours;
    if ours_behind_theirs {
        return Ok(Merged::FastForward(theirs));
    }
    let base_tree = tree_of(store, &base)?;
    let our_tree = tree_of(store, &ours)?;
    let their_tree = tree_of(store, &theirs)?;
    let outcome = merge_trees(
        store,
        Some(&base_tree),
        &our_tree,
        &their_tree,
        cap(bounds.merge_cost),
    )?;
    match outcome {
        MergeOutcome::Conflicts(conflicts) => Ok(Merged::Conflicts(
            conflicts
                .into_iter()
                .map(|conflict| conflict.path)
                .collect(),
        )),
        MergeOutcome::Clean(tree) => {
            let commit = Commit {
                tree,
                parents: vec![ours, theirs],
                author: author.clone(),
                committer: author,
                extra: Vec::new(),
                message: message.to_vec(),
            };
            let id = store.put(Kind::Commit, &commit.serialize())?;
            Ok(Merged::Commit(id))
        }
    }
}

fn tree_of<O: Objects>(store: &O, commit: &Oid) -> gitcore::Result<Oid> {
    let Some(object) = store.get(commit)? else {
        return Err(Error::MissingObject(*commit));
    };
    let is_commit = object.kind == Kind::Commit;
    if !is_commit {
        return Err(Error::WrongKind {
            id: *commit,
            expected: Kind::Commit,
        });
    }
    Ok(Commit::parse(&object.body, commit.hash())?.tree)
}

fn require_owner(repo: &Repo, actor: &[u8]) -> Result<(), Refusal> {
    let is_owner = repo.owner == actor;
    if !is_owner {
        return Err(unauthorized("only the owner changes a repository"));
    }
    Ok(())
}

fn require_writer<S: Sandbox>(
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
        Error::CapReached => capacity("history too long to walk within the bound"),
        other => invalid(other.to_string()),
    }
}
