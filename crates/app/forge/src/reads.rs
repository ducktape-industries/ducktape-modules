//! Object reads over the existing loose-object store; no pack parsing and no persistent writes.
use crate::Sandbox;
use crate::contract::*;
use crate::ops::{cap, refusal_of};
use crate::paging::Paging;
use crate::refuse::{invalid, not_found};
use crate::repo::{load_repo, parse_oid, repo_hash, resolve};
use crate::store::Store;
use abi::Refusal;
use gitcore::{Commit, Hash, Kind, Mode, Objects, Oid, Signature, Tag, Tree};
use std::collections::BTreeSet;

pub struct Reading<'a, S: Sandbox> {
    pub store: Store<'a, S>,
    pub hash: Hash,
    pub bounds: &'a Bounds,
}
impl<S: Sandbox> Reading<'_, S> {
    pub fn result<T>(&self, r: gitcore::Result<T>) -> Result<T, Refusal> {
        r.map_err(|e| refusal_of(&self.store, e))
    }
    pub fn oid(&self, s: &str) -> Result<Oid, Refusal> {
        parse_oid(self.hash, s)
    }
    pub fn commit_id(&self, mut id: Oid) -> Result<Oid, Refusal> {
        loop {
            let object = self
                .result(self.store.get(&id))?
                .ok_or_else(|| crate::refuse::object_not_held(id))?;
            match object.kind {
                Kind::Tag => id = self.result(Tag::parse(&object.body, self.hash))?.object,
                Kind::Commit => return Ok(id),
                _ => return Err(invalid("expected a commit or a tag pointing to a commit")),
            }
        }
    }
    pub fn commit(&self, id: &Oid) -> Result<Commit, Refusal> {
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| crate::refuse::object_not_held(id))?;
        if object.kind != Kind::Commit {
            return Err(invalid("expected a commit object"));
        }
        self.result(Commit::parse(&object.body, self.hash))
    }
    pub fn tree(&self, id: &Oid) -> Result<Tree, Refusal> {
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| crate::refuse::object_not_held(id))?;
        if object.kind != Kind::Tree {
            return Err(invalid("expected a tree object"));
        }
        self.result(Tree::parse(&object.body, self.hash))
    }
    pub fn tree_id(&self, at: &str) -> Result<Oid, Refusal> {
        let id = self.oid(at)?;
        let object = self
            .result(self.store.get(&id))?
            .ok_or_else(|| crate::refuse::object_not_held(id))?;
        match object.kind {
            Kind::Tree => Ok(id),
            Kind::Commit => Ok(self.result(Commit::parse(&object.body, self.hash))?.tree),
            _ => Err(invalid("tree endpoint must be a commit or tree")),
        }
    }
    pub fn blob(&self, id: &Oid, range: Option<ByteRange>) -> Result<BlobView, Refusal> {
        let header = self.result(self.store.header(id))?;
        if header.kind != "blob" {
            return Err(invalid("expected a blob object"));
        }
        let requested = range.unwrap_or(ByteRange {
            offset: 0,
            len: header.len.min(self.bounds.blob_bytes),
        });
        if requested.offset > header.len
            || requested.len > self.bounds.blob_bytes
            || requested.offset.checked_add(requested.len).is_none()
        {
            return Err(invalid("invalid blob byte range"));
        }
        let mut blob = BlobView {
            oid: id.to_hex(),
            size: header.len,
            content: Content::Oversize,
            range: ByteRange {
                offset: requested.offset,
                len: 0,
            },
            bytes: Vec::new(),
        };
        if header.len > self.bounds.blob_bytes {
            return Ok(blob);
        }
        let object = self
            .result(self.store.get(id))?
            .ok_or_else(|| crate::refuse::object_not_held(id))?;
        if object.body.contains(&0) || std::str::from_utf8(&object.body).is_err() {
            blob.content = Content::Binary;
            return Ok(blob);
        }
        blob.content = Content::Text;
        let end = requested
            .offset
            .saturating_add(requested.len)
            .min(header.len);
        blob.bytes = object.body[requested.offset as usize..end as usize].to_vec();
        blob.range.len = blob.bytes.len() as u64;
        Ok(blob)
    }
}
fn signature(sig: Signature) -> GitSignature {
    GitSignature {
        name: sig.name,
        email: sig.email,
        time: sig.time,
        offset_minutes: sig.offset_minutes,
    }
}
pub fn entry_kind(mode: Mode) -> EntryKind {
    match mode {
        Mode::Regular => EntryKind::File,
        Mode::Executable => EntryKind::Executable,
        Mode::Symlink => EntryKind::Symlink,
        Mode::Directory => EntryKind::Directory,
        Mode::Gitlink => EntryKind::Gitlink,
    }
}
pub fn answer<S: Sandbox>(
    s: &S,
    height: u64,
    q: &Query,
    bounds: &Bounds,
    page: Option<&Paging>,
) -> Result<Reply, Refusal> {
    let name = match q {
        Query::Log { repo, .. }
        | Query::Tree { repo, .. }
        | Query::Blob { repo, .. }
        | Query::Diff { repo, .. }
        | Query::Compare { repo, .. } => repo,
        _ => return Err(invalid("not an object query")),
    };
    let repo = load_repo(s, name)?;
    let hash = repo_hash(&repo);
    let reads = match q {
        Query::Log { .. } => bounds.log_walk.saturating_mul(2).saturating_add(1),
        Query::Compare { .. } => bounds
            .log_walk
            .saturating_mul(8)
            .saturating_add(bounds.tree_walk),
        _ => bounds.tree_walk,
    };
    let mut r = Reading {
        store: Store::querying(s, hash, bounds, reads),
        hash,
        bounds,
    };
    let page = || page.expect("this query has pagination");
    Ok(match q {
        Query::Log { from, .. } => {
            let tip = r.commit_id(resolve(s, name, from, hash)?)?;
            // ponytail: repeat the complete walk up to log_walk; index history if larger repos need it.
            let ids = r.result(gitcore::walk::commits(
                &r.store,
                &[tip],
                &[],
                cap(bounds.log_walk),
            ))?;
            let selected = page().slice(&ids)?;
            let items = selected
                .items
                .into_iter()
                .map(|id| {
                    let c = r.commit(&id)?;
                    Ok(CommitInfo {
                        oid: id.to_hex(),
                        tree: c.tree.to_hex(),
                        parents: c.parents.iter().map(Oid::to_hex).collect(),
                        author: signature(c.author),
                        committer: signature(c.committer),
                        message: c.message,
                    })
                })
                .collect::<Result<_, Refusal>>()?;
            Reply::Log {
                height,
                tip: tip.to_hex(),
                page: Page {
                    items,
                    next: selected.next,
                },
            }
        }
        Query::Tree { at, path, .. } => {
            crate::changes::path(path, true)?;
            let root = r.tree_id(at)?;
            let tree = if path.is_empty() {
                root
            } else {
                let entry = r
                    .result(gitcore::walk::tree_at_path(&r.store, &root, path))?
                    .ok_or_else(|| not_found("no entry at this path"))?;
                if entry.mode != Mode::Directory {
                    return Err(invalid("tree path names a directory"));
                }
                entry.id
            };
            let entries = r.tree(&tree)?.entries;
            let selected = page().slice(&entries)?;
            Reply::Tree {
                height,
                tree: tree.to_hex(),
                page: Page {
                    next: selected.next,
                    items: selected
                        .items
                        .into_iter()
                        .map(|e| TreeInfo {
                            name: e.name,
                            oid: e.id.to_hex(),
                            kind: entry_kind(e.mode),
                        })
                        .collect(),
                },
            }
        }
        Query::Blob { oid, range, .. } => Reply::Blob {
            height,
            blob: r.blob(&r.oid(oid)?, *range)?,
        },
        Query::Diff {
            base, head, path, ..
        } => crate::diffs::query(&r, height, base, head, path.as_deref(), page())?,
        Query::Compare { from, into, .. } => {
            let from = r.commit_id(resolve(s, name, from, hash)?)?;
            let into = r.commit_id(resolve(s, name, into, hash)?)?;
            compare(&mut r, height, from, into, page())?
        }
        _ => unreachable!(),
    })
}
fn compare<S: Sandbox>(
    r: &mut Reading<'_, S>,
    height: u64,
    from: Oid,
    into: Oid,
    p: &Paging,
) -> Result<Reply, Refusal> {
    let source: BTreeSet<_> = r
        .result(gitcore::walk::commits(
            &r.store,
            &[from],
            &[],
            cap(r.bounds.log_walk),
        ))?
        .into_iter()
        .collect();
    let target: BTreeSet<_> = r
        .result(gitcore::walk::commits(
            &r.store,
            &[into],
            &[],
            cap(r.bounds.log_walk),
        ))?
        .into_iter()
        .collect();
    let ahead = source.difference(&target).count() as u64;
    let behind = target.difference(&source).count() as u64;
    let base = r.result(gitcore::merge::merge_base(
        &r.store,
        &from,
        &into,
        cap(r.bounds.log_walk),
    ))?;
    let mut conflicts = Vec::new();
    let mergeability = match base {
        None => Mergeability::Unrelated,
        Some(b) if b == from => Mergeability::UpToDate,
        Some(b) if b == into => Mergeability::FastForward,
        Some(base) => {
            let base = r.commit(&base)?.tree;
            let ours = r.commit(&into)?.tree;
            let theirs = r.commit(&from)?.tree;
            let result = gitcore::merge::merge_trees(
                &mut r.store,
                Some(&base),
                &ours,
                &theirs,
                cap(r.bounds.merge_cost),
            );
            match r.result(result)? {
                gitcore::merge::MergeOutcome::Clean(_) => Mergeability::Clean,
                gitcore::merge::MergeOutcome::Conflicts(items) => {
                    use gitcore::merge::ConflictKind as K;
                    conflicts = items
                        .into_iter()
                        .map(|c| Conflict {
                            path: c.path,
                            kind: match c.kind {
                                K::Content => ConflictKind::Content,
                                K::AddAdd => ConflictKind::AddAdd,
                                K::ModifyDelete => ConflictKind::ModifyDelete,
                                K::ModeConflict => ConflictKind::Mode,
                                K::TypeConflict => ConflictKind::Type,
                                K::Submodule => ConflictKind::Submodule,
                            },
                        })
                        .collect();
                    Mergeability::Conflicts
                }
            }
        }
    };
    Ok(Reply::Compare {
        height,
        comparison: Comparison {
            from: from.to_hex(),
            into: into.to_hex(),
            base: base.map(|o| o.to_hex()),
            ahead,
            behind,
            mergeability,
        },
        conflicts: p.slice(&conflicts)?,
    })
}
