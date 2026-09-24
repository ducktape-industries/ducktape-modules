// History walks over the store: ancestry checks, wants-minus-haves, reachable trees and blobs, path lookup.

use super::error::{Error, Result};
use crate::object::{Commit, Kind, Tree, TreeEntry};
use crate::oid::Oid;
use crate::store::{Objects, load_kind};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    No,
    CapReached,
}

pub(crate) fn commit_of<S: Objects + ?Sized>(store: &S, id: &Oid) -> Result<Commit> {
    let object = load_kind(store, id, Kind::Commit)?;
    Commit::parse(&object.body, id.hash())
}

pub(crate) fn tree_of<S: Objects + ?Sized>(store: &S, id: &Oid) -> Result<Tree> {
    let object = load_kind(store, id, Kind::Tree)?;
    Tree::parse(&object.body, id.hash())
}

pub fn is_ancestor<S: Objects + ?Sized>(
    store: &S,
    ancestor: &Oid,
    descendant: &Oid,
    cap: usize,
) -> Result<Verdict> {
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    queue.push_back(*descendant);
    visited.insert(*descendant);
    while let Some(id) = queue.pop_front() {
        let found = id == *ancestor;
        if found {
            return Ok(Verdict::Yes);
        }
        let over_cap = visited.len() > cap;
        if over_cap {
            return Ok(Verdict::CapReached);
        }
        for parent in commit_of(store, &id)?.parents {
            let first_visit = visited.insert(parent);
            if first_visit {
                queue.push_back(parent);
            }
        }
    }
    Ok(Verdict::No)
}

struct Mark {
    uninteresting: bool,
    popped: bool,
    time: i64,
    parents: Vec<Oid>,
}

struct Frontier<'a, S: ?Sized> {
    store: &'a S,
    cap: usize,
    marks: BTreeMap<Oid, Mark>,
    queue: BinaryHeap<(i64, Reverse<Oid>)>,
    pending_interesting: usize,
}

impl<S: Objects + ?Sized> Frontier<'_, S> {
    fn enqueue(&mut self, id: Oid, uninteresting: bool) -> Result<()> {
        let over_cap = self.marks.len() >= self.cap;
        if over_cap {
            return Err(Error::CapReached);
        }
        let commit = commit_of(self.store, &id)?;
        self.marks.insert(
            id,
            Mark {
                uninteresting,
                popped: false,
                time: commit.committer.time,
                parents: commit.parents,
            },
        );
        self.queue.push((commit.committer.time, Reverse(id)));
        if !uninteresting {
            self.pending_interesting += 1;
        }
        Ok(())
    }

    fn mark_uninteresting(&mut self, id: Oid) -> Result<()> {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let Some(mark) = self.marks.get_mut(&current) else {
                let held_commit = self
                    .store
                    .get(&current)?
                    .is_some_and(|object| object.kind == Kind::Commit);
                if held_commit {
                    self.enqueue(current, true)?;
                }
                continue;
            };
            if mark.uninteresting {
                continue;
            }
            mark.uninteresting = true;
            if mark.popped {
                stack.extend(mark.parents.iter().copied());
            } else {
                self.pending_interesting -= 1;
            }
        }
        Ok(())
    }
}

pub fn commits<S: Objects + ?Sized>(
    store: &S,
    from: &[Oid],
    stop_at: &[Oid],
    cap: usize,
) -> Result<Vec<Oid>> {
    let mut frontier = Frontier {
        store,
        cap,
        marks: BTreeMap::new(),
        queue: BinaryHeap::new(),
        pending_interesting: 0,
    };
    for stop in stop_at {
        frontier.mark_uninteresting(*stop)?;
    }
    for tip in from {
        let known = frontier.marks.contains_key(tip);
        if !known {
            frontier.enqueue(*tip, false)?;
        }
    }
    while frontier.pending_interesting > 0 {
        let Some((_, Reverse(id))) = frontier.queue.pop() else {
            break;
        };
        let (uninteresting, parents) = {
            let mark = frontier.marks.get_mut(&id).ok_or(Error::Cycle)?;
            mark.popped = true;
            (mark.uninteresting, mark.parents.clone())
        };
        if uninteresting {
            for parent in parents {
                frontier.mark_uninteresting(parent)?;
            }
            continue;
        }
        frontier.pending_interesting -= 1;
        for parent in parents {
            let known = frontier.marks.contains_key(&parent);
            if !known {
                frontier.enqueue(parent, false)?;
            }
        }
    }
    let mut found: Vec<(Reverse<i64>, Oid)> = frontier
        .marks
        .iter()
        .filter(|(_, mark)| mark.popped && !mark.uninteresting)
        .map(|(id, mark)| (Reverse(mark.time), *id))
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, id)| id).collect())
}

pub fn reachable_objects<S: Objects + ?Sized>(
    store: &S,
    commits: &[Oid],
    seen: &BTreeSet<Oid>,
) -> Result<BTreeSet<Oid>> {
    let mut roots = Vec::with_capacity(commits.len());
    for commit in commits {
        roots.push(commit_of(store, commit)?.tree);
    }
    reachable_from_trees(store, &roots, seen)
}

pub fn reachable_from_trees<S: Objects + ?Sized>(
    store: &S,
    trees: &[Oid],
    seen: &BTreeSet<Oid>,
) -> Result<BTreeSet<Oid>> {
    let mut found = BTreeSet::new();
    let mut stack: Vec<Oid> = trees.to_vec();
    while let Some(tree_id) = stack.pop() {
        let skip = seen.contains(&tree_id) || !found.insert(tree_id);
        if skip {
            continue;
        }
        for entry in tree_of(store, &tree_id)?.entries {
            match entry.mode {
                crate::object::Mode::Directory => stack.push(entry.id),
                crate::object::Mode::Gitlink => {}
                _ => {
                    let skip_blob = seen.contains(&entry.id);
                    if !skip_blob {
                        found.insert(entry.id);
                    }
                }
            }
        }
    }
    Ok(found)
}

pub fn tree_at_path<S: Objects + ?Sized>(
    store: &S,
    tree: &Oid,
    path: &[u8],
) -> Result<Option<TreeEntry>> {
    let mut components = path.split(|byte| *byte == b'/').peekable();
    let mut current = *tree;
    while let Some(component) = components.next() {
        let malformed = component.is_empty();
        if malformed {
            return Ok(None);
        }
        let Some(entry) = tree_of(store, &current)?
            .entries
            .into_iter()
            .find(|e| e.name == component)
        else {
            return Ok(None);
        };
        let last = components.peek().is_none();
        if last {
            return Ok(Some(entry));
        }
        let descends = entry.mode.is_directory();
        if !descends {
            return Ok(None);
        }
        current = entry.id;
    }
    Ok(None)
}

// The merge base: the lowest common ancestor by two-color painting, ties broken by the smaller oid.
const PARENT1: u8 = 1;
const PARENT2: u8 = 2;
const STALE: u8 = 4;
const RESULT: u8 = 8;

struct Paint<'a, S: ?Sized> {
    store: &'a S,
    cap: usize,
    commits: BTreeMap<Oid, (i64, Vec<Oid>)>,
    flags: BTreeMap<Oid, u8>,
    queue: BinaryHeap<(i64, Reverse<Oid>, bool)>,
    fresh_queued: usize,
}

impl<S: Objects + ?Sized> Paint<'_, S> {
    fn info(&mut self, id: Oid) -> Result<(i64, Vec<Oid>)> {
        if let Some(known) = self.commits.get(&id) {
            return Ok(known.clone());
        }
        let over_cap = self.commits.len() >= self.cap;
        if over_cap {
            return Err(Error::CapReached);
        }
        let commit = commit_of(self.store, &id)?;
        let info = (commit.committer.time, commit.parents);
        self.commits.insert(id, info.clone());
        Ok(info)
    }

    fn mark(&mut self, id: Oid, bits: u8) -> Result<()> {
        let existing = self.flags.get(&id).copied().unwrap_or(0);
        let nothing_new = existing & bits == bits;
        if nothing_new {
            return Ok(());
        }
        let updated = existing | bits;
        self.flags.insert(id, updated);
        let (time, _) = self.info(id)?;
        let fresh = updated & STALE == 0;
        if fresh {
            self.fresh_queued += 1;
        }
        self.queue.push((time, Reverse(id), fresh));
        Ok(())
    }
}

pub fn merge_base<S: Objects + ?Sized>(
    store: &S,
    a: &Oid,
    b: &Oid,
    cap: usize,
) -> Result<Option<Oid>> {
    let trivial = a == b;
    if trivial {
        return Ok(Some(*a));
    }
    let mut paint = Paint {
        store,
        cap,
        commits: BTreeMap::new(),
        flags: BTreeMap::new(),
        queue: BinaryHeap::new(),
        fresh_queued: 0,
    };
    paint.mark(*a, PARENT1)?;
    paint.mark(*b, PARENT2)?;
    let mut results = Vec::new();
    while paint.fresh_queued > 0 {
        let Some((_, Reverse(id), was_fresh)) = paint.queue.pop() else {
            break;
        };
        if was_fresh {
            paint.fresh_queued -= 1;
        }
        let flags = paint.flags.get(&id).copied().unwrap_or(0);
        let mut propagate = flags & (PARENT1 | PARENT2 | STALE);
        let reached_from_both = propagate & (PARENT1 | PARENT2) == PARENT1 | PARENT2;
        if reached_from_both {
            let first_time = flags & RESULT == 0;
            if first_time {
                paint.flags.insert(id, flags | RESULT);
                results.push(id);
            }
            propagate |= STALE;
        }
        let (_, parents) = paint.info(id)?;
        for parent in parents {
            paint.mark(parent, propagate)?;
        }
    }
    let mut best: Vec<Oid> = Vec::new();
    for candidate in &results {
        let mut dominated = false;
        for other in &results {
            let same = other == candidate;
            if same {
                continue;
            }
            match is_ancestor(store, candidate, other, cap)? {
                Verdict::Yes => {
                    dominated = true;
                    break;
                }
                Verdict::No => {}
                Verdict::CapReached => return Err(Error::CapReached),
            }
        }
        if !dominated {
            best.push(*candidate);
        }
    }
    Ok(best.into_iter().min())
}
