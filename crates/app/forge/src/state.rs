//! Every table forge keeps, declared once: the records, the indexes over
//! them and the counters. Git objects are not here: an object's blob id is
//! its oid (`objects`). People are [`Party`]s: an account, or a key that
//! holds none.

use std::collections::BTreeMap;

use abi::{Refusal, reason};
use gitcore::{Hash, Oid};
use store::{Item, Map, Reads, Set, Writes, invalid, not_found};

use crate::contract::{Bounds, Change, Party, Repo, Review, Revision, valid_repo_name};
use crate::objects::hash_of;

/// The bounds forge was founded with.
const BOUNDS: Item<Bounds> = Item::new("bounds");
/// One record per repository, by name.
const REPOS: Map<String, Repo> = Map::new("p/");
/// Index: every repository by its last activity, newest first.
pub(crate) const ACTIVITY: Set<(u64, String)> = Set::new("a/");
/// The parties the owner granted writes to, by repository.
pub(crate) const WRITERS: Set<(String, Party)> = Set::new("w/");
/// Each repository's refs and the oid bytes each points at.
pub(crate) const REFS: Map<(String, Vec<u8>), Vec<u8>> = Map::new("r/");
/// The last change number each repository gave out.
const NUMBERS: Map<String, u64> = Map::new("n/");
/// Changes by repository and number; a scan across repositories lists them
/// by name (Judgment pages this table whole).
pub(crate) const CHANGES: Map<(String, u64), Change> = Map::new("c/");
/// Reviews by repository, change number and review id.
pub(crate) const REVIEWS: Map<(String, u64, u64), Review> = Map::new("v/");
/// Index: the reviews one party submitted on one change, oldest first.
pub(crate) const AUTHORED: Set<(String, u64, Party, u64)> = Set::new("u/");
/// Index: the changes a party authored, is asked to review, or reviewed.
pub(crate) const INVOLVED: Set<(Party, String, u64)> = Set::new("i/");
/// The last system message id forge posted into chat.
const MESSAGES: Item<u64> = Item::new("system-message-seq");

/// Stored state that is not what forge wrote: an operator's problem.
pub(crate) fn storage(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::CORRUPT, sentence)
}

pub fn save_bounds(store: &mut impl Writes, bounds: &Bounds) {
    BOUNDS.put(store, bounds);
}

pub fn load_bounds(store: &impl Reads) -> Result<Bounds, Refusal> {
    BOUNDS
        .get(store)?
        .ok_or_else(|| Refusal::new(reason::PROTOCOL, "the program was founded without bounds"))
}

pub fn repo_exists(store: &impl Reads, name: &str) -> bool {
    REPOS.has(store, &name.to_owned())
}

pub fn load_repo(store: &impl Reads, name: &str) -> Result<Repo, Refusal> {
    if !valid_repo_name(name) {
        return Err(invalid(format!("{name:?} is not a repository name")));
    }
    REPOS
        .get(store, &name.to_owned())?
        .ok_or_else(|| not_found(format!("no repository named {name}")))
}

/// Stores the record and moves it in the activity index, so the index
/// holds exactly one row per repository.
pub fn save_repo(store: &mut impl Writes, name: &str, repo: &Repo) -> Result<(), Refusal> {
    if let Some(old) = REPOS.get(store, &name.to_owned())? {
        ACTIVITY.remove(store, &(newest_first(old.last_activity), name.to_owned()));
    }
    ACTIVITY.insert(store, &(newest_first(repo.last_activity), name.to_owned()));
    REPOS.put(store, &name.to_owned(), repo);
    Ok(())
}

/// An activity key: a later height sorts first.
fn newest_first(height: u64) -> u64 {
    u64::MAX - height
}

pub fn repo_hash(repo: &Repo) -> Hash {
    hash_of(repo.hash)
}

pub fn is_writer(store: &impl Reads, name: &str, party: &Party) -> bool {
    WRITERS.has(store, &(name.to_owned(), party.clone()))
}

pub fn ref_key(name: &str, reference: &[u8]) -> (String, Vec<u8>) {
    (name.to_owned(), reference.to_vec())
}

/// Every ref of a repository: what a push is checked against and git is told.
pub fn load_refs(
    store: &impl Reads,
    name: &str,
    hash: Hash,
) -> Result<BTreeMap<Vec<u8>, Oid>, Refusal> {
    REFS.scan(store, REFS.prefix_of(&name.to_owned()))?
        .into_iter()
        .map(|((_, reference), bytes)| {
            let target = Oid::from_bytes(hash, &bytes).map_err(|error| {
                Refusal::new(reason::PROTOCOL, format!("ref {reference:?} holds {error}"))
            })?;
            Ok((reference, target))
        })
        .collect()
}

pub fn load_ref(
    store: &impl Reads,
    name: &str,
    reference: &[u8],
    hash: Hash,
) -> Result<Option<Oid>, Refusal> {
    REFS.get(store, &ref_key(name, reference))?
        .map(|bytes| Oid::from_bytes(hash, &bytes).map_err(|e| storage(e.to_string())))
        .transpose()
}

pub fn set_ref(store: &mut impl Writes, name: &str, reference: &[u8], target: &Oid) {
    REFS.put(
        store,
        &ref_key(name, reference),
        &target.as_bytes().to_vec(),
    );
}

pub fn delete_ref(store: &mut impl Writes, name: &str, reference: &[u8]) {
    REFS.remove(store, &ref_key(name, reference));
}

/// Resolving an op's endpoint reads consensus refs only, never objects.
pub fn resolve(
    store: &impl Reads,
    name: &str,
    revision: &Revision,
    hash: Hash,
) -> Result<Oid, Refusal> {
    match revision {
        Revision::Oid(hex) => parse_oid(hash, hex),
        Revision::Ref(reference) => {
            if !gitcore::server::valid_ref_name(reference) {
                return Err(invalid("revision must name a full ref"));
            }
            load_ref(store, name, reference, hash)?
                .ok_or_else(|| not_found("the ref does not exist"))
        }
    }
}

pub fn parse_oid(hash: Hash, hex: &str) -> Result<Oid, Refusal> {
    let oid = Oid::from_hex(hash, hex)
        .map_err(|_| invalid("oid has the wrong length or hex for this repo"))?;
    if oid.is_zero() {
        return Err(invalid("an object id cannot be zero"));
    }
    Ok(oid)
}

/// The number a new change of this repository takes. Issues would share it.
pub fn next_number(store: &impl Reads, name: &str) -> Result<u64, Refusal> {
    next(NUMBERS.get(store, &name.to_owned())?.unwrap_or(0))
}

/// The id the next system line forge posts into chat takes, unclaimed.
pub fn peek_message(store: &impl Reads) -> Result<String, Refusal> {
    Ok(message_id(next_message_number(store)?))
}

/// Claims the next id of a system line forge posts into chat.
pub fn next_message(store: &mut impl Writes) -> Result<String, Refusal> {
    let n = next_message_number(store)?;
    MESSAGES.put(store, &n);
    Ok(message_id(n))
}

fn next_message_number(store: &impl Reads) -> Result<u64, Refusal> {
    MESSAGES
        .get(store)?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Refusal::new(reason::EXHAUSTED, "system message counter exhausted"))
}

fn message_id(n: u64) -> String {
    format!("forge:{n:016x}")
}

/// A counter one step on, refused rather than wrapped.
pub fn next(n: u64) -> Result<u64, Refusal> {
    n.checked_add(1)
        .ok_or_else(|| Refusal::new(reason::EXHAUSTED, "counter exhausted"))
}

pub fn load_change(store: &impl Reads, repo: &str, n: u64) -> Result<Change, Refusal> {
    CHANGES
        .get(store, &(repo.to_owned(), n))?
        .ok_or_else(|| not_found(format!("no change {repo}#{n}")))
}

/// Stores the change and keeps [`INVOLVED`] in step with it: its author and
/// every requested reviewer are involved; a reviewer taken off the request
/// stays involved only if they reviewed it. A new change claims its number.
pub fn save_change(store: &mut impl Writes, repo: &str, change: &Change) -> Result<(), Refusal> {
    let row = (repo.to_owned(), change.n);
    match CHANGES.get(store, &row)? {
        None => NUMBERS.put(store, &repo.to_owned(), &change.n),
        Some(old) => {
            let dropped: Vec<Party> = old
                .reviewers
                .into_iter()
                .filter(|party| {
                    !change.reviewers.contains(party)
                        && *party != change.author
                        && !has_reviewed(store, repo, change.n, party)
                })
                .collect();
            for party in dropped {
                INVOLVED.remove(store, &(party, repo.to_owned(), change.n));
            }
        }
    }
    for party in std::iter::once(&change.author).chain(&change.reviewers) {
        involve(store, party, repo, change.n);
    }
    CHANGES.put(store, &row, change);
    Ok(())
}

pub fn involve(store: &mut impl Writes, party: &Party, repo: &str, n: u64) {
    INVOLVED.insert(store, &(party.clone(), repo.to_owned(), n));
}

fn has_reviewed(store: &impl Reads, repo: &str, n: u64, party: &Party) -> bool {
    !store
        .scan(
            AUTHORED
                .prefix_of(&(repo.to_owned(), n, party.clone()))
                .limit(1),
        )
        .is_empty()
}

pub fn save_review(store: &mut impl Writes, repo: &str, n: u64, review: &Review) {
    REVIEWS.put(store, &(repo.to_owned(), n, review.id), review);
    AUTHORED.insert(
        store,
        &(repo.to_owned(), n, review.author.clone(), review.id),
    );
    involve(store, &review.author, repo, n);
}

pub fn load_review(store: &impl Reads, repo: &str, n: u64, id: u64) -> Result<Review, Refusal> {
    REVIEWS
        .get(store, &(repo.to_owned(), n, id))?
        .ok_or_else(|| storage("authored review missing"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ChangeState, ReviewCounts};
    use store::Memory;

    fn change(n: u64) -> Change {
        Change {
            n,
            from: Revision::Ref(b"refs/heads/feature".to_vec()),
            into: b"refs/heads/main".to_vec(),
            title: "t".into(),
            body: String::new(),
            author: Party::Key(b"ada".to_vec()),
            state: ChangeState::Open,
            reviewers: Vec::new(),
            created_height: 1,
            updated_height: 1,
            created_time: 1,
            updated_time: 1,
            review_count: 0,
            comment_count: 0,
            verdicts: ReviewCounts::default(),
            merge_oid: None,
            closed_by: None,
            merged_by: None,
            channel: String::new(),
            system_seq: 1,
        }
    }

    /// Judgment pages every change across repositories: by name, not by
    /// name length, and one repository's prefix never reaches a longer name.
    #[test]
    fn changes_across_repositories_list_by_name() {
        let mut store = Memory::default();
        for (repo, n) in [("zz", 1), ("abc", 2), ("ab", 1), ("abc", 1)] {
            save_change(&mut store, repo, &change(n)).unwrap();
        }
        let order: Vec<(String, u64)> = CHANGES
            .all(&store)
            .unwrap()
            .into_iter()
            .map(|((repo, n), _)| (repo, n))
            .collect();
        let expected = [("ab", 1), ("abc", 1), ("abc", 2), ("zz", 1)];
        assert_eq!(order, expected.map(|(r, n)| (r.to_owned(), n)));
        let ab = CHANGES
            .scan(&store, CHANGES.prefix_of(&"ab".to_string()))
            .unwrap();
        assert_eq!(ab.len(), 1);
    }
}
