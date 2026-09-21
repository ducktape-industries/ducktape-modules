// Merge base search, diff3-style line merge, and three-way tree merge that writes the result into the store.

use crate::diff::{lines, Hunk, Op};
use crate::error::{Error, Result};
use crate::object::{Kind, Mode, Object, Tree, TreeEntry};
use crate::oid::{oid_of, Oid};
use crate::store::{load_kind, Objects};
use crate::walk::{commit_of, is_ancestor, tree_of, Verdict};
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap};
use alloc::vec::Vec;
use core::cmp::Reverse;
use core::ops::Range;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Merged {
    Clean(Vec<u8>),
    Conflict,
}

struct Chunk {
    base: Range<usize>,
    side: Range<usize>,
}

fn chunks(hunks: &[Hunk]) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for hunk in hunks {
        let unchanged = hunk.op == Op::Equal;
        if unchanged {
            continue;
        }
        let adjacent = out
            .last()
            .is_some_and(|last| last.base.end == hunk.old.start && last.side.end == hunk.new.start);
        if adjacent {
            let last = out.last_mut().expect("checked above");
            last.base.end = hunk.old.end;
            last.side.end = hunk.new.end;
        } else {
            out.push(Chunk {
                base: hunk.old.clone(),
                side: hunk.new.clone(),
            });
        }
    }
    out
}

fn side_position(chunks: &[Chunk], base_position: usize, include_touching: bool) -> usize {
    let mut delta: isize = 0;
    for chunk in chunks {
        let before = if include_touching {
            chunk.base.start <= base_position
        } else {
            chunk.base.end < base_position
        };
        if !before {
            continue;
        }
        delta += chunk.side.len() as isize - chunk.base.len() as isize;
    }
    (base_position as isize + delta) as usize
}

pub fn merge_lines(base: &[u8], ours: &[u8], theirs: &[u8], max_cost: usize) -> Result<Merged> {
    let sides_agree = ours == theirs;
    if sides_agree {
        return Ok(Merged::Clean(ours.to_vec()));
    }
    let ours_unchanged = base == ours;
    if ours_unchanged {
        return Ok(Merged::Clean(theirs.to_vec()));
    }
    let theirs_unchanged = base == theirs;
    if theirs_unchanged {
        return Ok(Merged::Clean(ours.to_vec()));
    }
    let base_lines = crate::diff::split_lines(base);
    let our_lines = crate::diff::split_lines(ours);
    let their_lines = crate::diff::split_lines(theirs);
    let our_chunks = chunks(&lines(base, ours, max_cost)?);
    let their_chunks = chunks(&lines(base, theirs, max_cost)?);
    let mut out = Vec::new();
    let mut base_cursor = 0;
    let mut i = 0;
    let mut j = 0;
    while i < our_chunks.len() || j < their_chunks.len() {
        let our_start = our_chunks.get(i).map(|c| c.base.start);
        let their_start = their_chunks.get(j).map(|c| c.base.start);
        let region_start = match (our_start, their_start) {
            (Some(o), Some(t)) => o.min(t),
            (Some(o), None) => o,
            (None, Some(t)) => t,
            (None, None) => break,
        };
        let mut region_end = region_start;
        let mut used_ours = false;
        let mut used_theirs = false;
        loop {
            let mut grew = false;
            while our_chunks
                .get(i)
                .is_some_and(|c| c.base.start <= region_end)
            {
                region_end = region_end.max(our_chunks[i].base.end);
                used_ours = true;
                grew = true;
                i += 1;
            }
            while their_chunks
                .get(j)
                .is_some_and(|c| c.base.start <= region_end)
            {
                region_end = region_end.max(their_chunks[j].base.end);
                used_theirs = true;
                grew = true;
                j += 1;
            }
            if !grew {
                break;
            }
        }
        let our_text = &our_lines[side_position(&our_chunks, region_start, false)
            ..side_position(&our_chunks, region_end, true)];
        let their_text = &their_lines[side_position(&their_chunks, region_start, false)
            ..side_position(&their_chunks, region_end, true)];
        let replacement = match (used_ours, used_theirs) {
            (true, false) => our_text,
            (false, true) => their_text,
            _ => {
                let identical = our_text == their_text;
                if !identical {
                    return Ok(Merged::Conflict);
                }
                our_text
            }
        };
        for line in &base_lines[base_cursor..region_start] {
            out.extend_from_slice(line);
        }
        for line in replacement {
            out.extend_from_slice(line);
        }
        base_cursor = region_end;
    }
    for line in &base_lines[base_cursor..] {
        out.extend_from_slice(line);
    }
    Ok(Merged::Clean(out))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    Content,
    AddAdd,
    ModifyDelete,
    ModeConflict,
    TypeConflict,
    Submodule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub path: Vec<u8>,
    pub kind: ConflictKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    Clean(Oid),
    Conflicts(Vec<Conflict>),
}

struct TreeMerge<'a, S: ?Sized> {
    store: &'a S,
    max_cost: usize,
    pending: Vec<Object>,
    conflicts: Vec<Conflict>,
    path: Vec<u8>,
}

fn entries_by_name<S: Objects + ?Sized>(
    store: &S,
    tree: Option<Oid>,
) -> Result<BTreeMap<Vec<u8>, TreeEntry>> {
    let Some(id) = tree else {
        return Ok(BTreeMap::new());
    };
    Ok(tree_of(store, &id)?
        .entries
        .into_iter()
        .map(|entry| (entry.name.clone(), entry))
        .collect())
}

fn same_entry(a: Option<&TreeEntry>, b: Option<&TreeEntry>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.id == b.id && a.mode == b.mode,
        _ => false,
    }
}

impl<S: Objects + ?Sized> TreeMerge<'_, S> {
    fn conflict(&mut self, kind: ConflictKind) {
        self.conflicts.push(Conflict {
            path: self.path.clone(),
            kind,
        });
    }

    fn merge_dir(&mut self, base: Option<Oid>, ours: Oid, theirs: Oid) -> Result<Oid> {
        let base_entries = entries_by_name(self.store, base)?;
        let our_entries = entries_by_name(self.store, Some(ours))?;
        let their_entries = entries_by_name(self.store, Some(theirs))?;
        let names: BTreeSet<&Vec<u8>> = base_entries
            .keys()
            .chain(our_entries.keys())
            .chain(their_entries.keys())
            .collect();
        let mut merged = Vec::new();
        for name in names {
            let prefix_len = self.path.len();
            if !self.path.is_empty() {
                self.path.push(b'/');
            }
            self.path.extend_from_slice(name);
            let entry = self.merge_entry(
                base_entries.get(name),
                our_entries.get(name),
                their_entries.get(name),
            )?;
            self.path.truncate(prefix_len);
            if let Some(entry) = entry {
                merged.push(entry);
            }
        }
        let body = Tree { entries: merged }.serialize();
        let id = oid_of(ours.hash(), Kind::Tree, &body)?;
        let already_exists = id == ours || id == theirs || Some(id) == base;
        if !already_exists {
            self.pending.push(Object::new(Kind::Tree, body));
        }
        Ok(id)
    }

    fn merge_entry(
        &mut self,
        base: Option<&TreeEntry>,
        ours: Option<&TreeEntry>,
        theirs: Option<&TreeEntry>,
    ) -> Result<Option<TreeEntry>> {
        if same_entry(ours, theirs) {
            return Ok(ours.cloned());
        }
        if same_entry(ours, base) {
            return Ok(theirs.cloned());
        }
        if same_entry(theirs, base) {
            return Ok(ours.cloned());
        }
        let (Some(our), Some(their)) = (ours, theirs) else {
            self.conflict(ConflictKind::ModifyDelete);
            return Ok(None);
        };
        match (our.mode.is_directory(), their.mode.is_directory()) {
            (true, true) => {
                let base_dir = base.filter(|b| b.mode.is_directory()).map(|b| b.id);
                let id = self.merge_dir(base_dir, our.id, their.id)?;
                let empty = id == oid_of(our.id.hash(), Kind::Tree, &[])?;
                if empty {
                    return Ok(None);
                }
                Ok(Some(TreeEntry {
                    mode: Mode::Directory,
                    name: our.name.clone(),
                    id,
                }))
            }
            (false, false) => self.merge_leaf(base, our, their),
            _ => {
                self.conflict(ConflictKind::TypeConflict);
                Ok(None)
            }
        }
    }

    fn merge_leaf(
        &mut self,
        base: Option<&TreeEntry>,
        our: &TreeEntry,
        their: &TreeEntry,
    ) -> Result<Option<TreeEntry>> {
        let both_files = our.mode.is_file() && their.mode.is_file();
        let same_kind = both_files || our.mode == their.mode;
        if !same_kind {
            self.conflict(ConflictKind::TypeConflict);
            return Ok(None);
        }
        let base_mode = base.map(|b| b.mode);
        let modes_agree = our.mode == their.mode;
        let our_mode_kept = base_mode == Some(our.mode);
        let their_mode_kept = base_mode == Some(their.mode);
        let mode = match (modes_agree, our_mode_kept, their_mode_kept) {
            (true, _, _) => our.mode,
            (false, true, _) => their.mode,
            (false, false, true) => our.mode,
            (false, false, false) => {
                self.conflict(ConflictKind::ModeConflict);
                return Ok(None);
            }
        };
        let same_content = our.id == their.id;
        if same_content {
            return Ok(Some(TreeEntry {
                mode,
                name: our.name.clone(),
                id: our.id,
            }));
        }
        if !both_files {
            let kind = match (our.mode, base) {
                (Mode::Gitlink, _) => ConflictKind::Submodule,
                (_, None) => ConflictKind::AddAdd,
                _ => ConflictKind::Content,
            };
            self.conflict(kind);
            return Ok(None);
        }
        let Some(base_blob) = base.filter(|b| b.mode.is_file()) else {
            self.conflict(ConflictKind::AddAdd);
            return Ok(None);
        };
        let base_body = load_kind(self.store, &base_blob.id, Kind::Blob)?.body;
        let our_body = load_kind(self.store, &our.id, Kind::Blob)?.body;
        let their_body = load_kind(self.store, &their.id, Kind::Blob)?.body;
        match merge_lines(&base_body, &our_body, &their_body, self.max_cost)? {
            Merged::Clean(body) => {
                let id = oid_of(our.id.hash(), Kind::Blob, &body)?;
                self.pending.push(Object::new(Kind::Blob, body));
                Ok(Some(TreeEntry {
                    mode,
                    name: our.name.clone(),
                    id,
                }))
            }
            Merged::Conflict => {
                self.conflict(ConflictKind::Content);
                Ok(None)
            }
        }
    }
}

pub fn merge_trees<S: Objects + ?Sized>(
    store: &mut S,
    base: Option<&Oid>,
    ours: &Oid,
    theirs: &Oid,
    max_cost: usize,
) -> Result<MergeOutcome> {
    let mut merge = TreeMerge {
        store: &*store,
        max_cost,
        pending: Vec::new(),
        conflicts: Vec::new(),
        path: Vec::new(),
    };
    let id = merge.merge_dir(base.copied(), *ours, *theirs)?;
    let TreeMerge {
        pending, conflicts, ..
    } = merge;
    let conflicted = !conflicts.is_empty();
    if conflicted {
        return Ok(MergeOutcome::Conflicts(conflicts));
    }
    for object in pending {
        store.put(object.kind, &object.body)?;
    }
    Ok(MergeOutcome::Clean(id))
}
