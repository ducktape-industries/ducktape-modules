//! The execute path: who acts, which op, and the repository ops (create,
//! configure, grant, revoke, push). Change ops live in `changes`.

use abi::{HashKind, Refusal};
use gitcore::server::{Policy, RefUpdate};
use gitcore::{Error, Limits, server};
use store::{Reads, Writes, already_exists, capacity, decoded, invalid, unauthorized};

use crate::changes::{self, Draft, Edit, MergeRequest};
use crate::contract::{Bounds, Frame, MAX_PATH_BYTES, Op, Party, Repo, Settings, valid_repo_name};
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

/// Runs one op as `frame.party`, whom the program resolved from the
/// signer ([`identity::party_of`]). Every op names its repository; an accepted one
/// marks it active.
pub fn execute(store: &mut impl Writes, frame: &Frame, op: Op) -> Result<(), Refusal> {
    let actor = person(&frame.party)?;
    let repo = op.repo().to_owned();
    let reply = match op {
        Op::Create { repo, hash } => create(store, actor, &repo, hash).map(|()| None),
        Op::Configure { repo, settings } => configure(store, actor, &repo, settings).map(|()| None),
        Op::Grant { repo, party } => grant(store, actor, &repo, party).map(|()| None),
        Op::Revoke { repo, party } => revoke(store, actor, &repo, party).map(|()| None),
        Op::Push { repo, request } => push(store, actor, &repo, &request).map(|()| None),
        Op::Merge {
            repo,
            into,
            from,
            expected_into,
            expected_from,
            result,
            change,
        } => {
            let merge = MergeRequest {
                into,
                from,
                expected_into,
                expected_from,
                result,
                change,
            };
            changes::merge_heads(store, frame, actor, &repo, merge).map(Some)
        }
        Op::ChangeOpen {
            repo,
            from,
            into,
            title,
            body,
            reviewers,
        } => {
            let draft = Draft {
                from,
                into,
                title,
                body,
                reviewers,
            };
            changes::open(store, frame, actor, &repo, draft).map(Some)
        }
        Op::ChangeEdit {
            repo,
            n,
            title,
            body,
            reviewers,
        } => {
            let fields = Edit {
                title,
                body,
                reviewers,
            };
            changes::edit(store, frame, actor, &repo, n, fields).map(Some)
        }
        Op::ChangeClose { repo, n } => changes::close(store, frame, actor, &repo, n).map(Some),
        Op::ReviewSubmit { repo, n, review } => {
            changes::submit_review(store, frame, actor, &repo, n, review).map(Some)
        }
    }?;
    if let Some(reply) = reply {
        store.output(abi::encode(&reply));
    }
    touch(store, &repo, frame.height)
}

/// Forge is written by people (an account), never by a program or the
/// system. A key that holds no account never gets here: identity's
/// [`party_of`](identity::party_of) refuses it.
fn person(party: &Party) -> Result<&Party, Refusal> {
    if !party.is_person() {
        return Err(unauthorized("a repository op is signed by a person"));
    }
    Ok(party)
}

/// Every accepted op marks its repository active at this height.
fn touch(store: &mut impl Writes, name: &str, height: u64) -> Result<(), Refusal> {
    let mut repo = load_repo(store, name)?;
    repo.last_activity = height;
    save_repo(store, name, &repo)
}

fn create(
    store: &mut impl Writes,
    actor: &Party,
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
        owner: actor.clone(),
        settings: Settings::default(),
        refs_count: 0,
        last_activity: 0,
    };
    save_repo(store, name, &repo)
}

fn configure(
    store: &mut impl Writes,
    actor: &Party,
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

fn grant(store: &mut impl Writes, actor: &Party, name: &str, party: Party) -> Result<(), Refusal> {
    require_owner(&load_repo(store, name)?, actor)?;
    require_named(&party)?;
    WRITERS.insert(store, &(name.to_owned(), party));
    Ok(())
}

fn revoke(store: &mut impl Writes, actor: &Party, name: &str, party: Party) -> Result<(), Refusal> {
    require_owner(&load_repo(store, name)?, actor)?;
    require_named(&party)?;
    WRITERS.remove(store, &(name.to_owned(), party));
    Ok(())
}

/// A git receive-pack: the objects land as blobs, then each accepted ref
/// moves; git's own report is the op's output.
fn push(store: &mut impl Writes, actor: &Party, name: &str, request: &[u8]) -> Result<(), Refusal> {
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
    .map_err(refusal_of)?;
    objects.flush()?;
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

fn require_owner(repo: &Repo, actor: &Party) -> Result<(), Refusal> {
    if repo.owner != *actor {
        return Err(unauthorized("only the owner changes a repository"));
    }
    Ok(())
}

pub(crate) fn require_writer(
    store: &impl Reads,
    name: &str,
    repo: &Repo,
    actor: &Party,
) -> Result<(), Refusal> {
    let may_write = repo.owner == *actor || is_writer(store, name, actor);
    if !may_write {
        return Err(unauthorized("only the owner and its writers push"));
    }
    Ok(())
}

/// A person an op names (a writer, a reviewer): an account.
pub(crate) fn require_named(party: &Party) -> Result<(), Refusal> {
    if !party.is_person() {
        return Err(invalid("only a person is named here"));
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
