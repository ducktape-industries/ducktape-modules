//! Structured hunks are built from git::diff's edit ranges, never parsed from patch text.
use crate::Sandbox;
use crate::contract::*;
use crate::git::{Mode, Oid, diff};
use crate::ops::cap;
use crate::paging::Paging;
use crate::reads::{Reading, entry_kind};
use abi::Refusal;

pub fn query<S: Sandbox>(
    r: &Reading<'_, S>,
    height: u64,
    base: &Option<String>,
    head: &str,
    path: Option<&[u8]>,
    paging: &Paging,
) -> Result<Reply, Refusal> {
    if let Some(path) = path {
        crate::changes::path(path, false)?;
    }
    let old = base.as_ref().map(|s| r.tree_id(s)).transpose()?;
    let new = r.tree_id(head)?;
    let changes = r.result(diff::tree_diff(&r.store, old.as_ref(), Some(&new)))?;
    let changes: Vec<_> = changes
        .into_iter()
        .filter(|c| path.is_none_or(|p| c.path == p))
        .collect();
    let selected = paging.slice(&changes)?;
    let items = selected
        .items
        .iter()
        .map(|c| file(r, c))
        .collect::<Result<_, _>>()?;
    Ok(Reply::Diff {
        height,
        base: base
            .as_ref()
            .map(|s| r.oid(s).map(|o| o.to_hex()))
            .transpose()?,
        head: r.oid(head)?.to_hex(),
        total_files: changes.len() as u64,
        page: Page {
            items,
            next: selected.next,
        },
    })
}

fn file<S: Sandbox>(r: &Reading<'_, S>, c: &diff::Change) -> Result<FileDiff, Refusal> {
    use diff::ChangeKind as K;
    let (old, new, status) = match c.kind {
        K::Added { mode, id } => (None, Some((mode, id)), FileStatus::Added),
        K::Deleted { mode, id } => (Some((mode, id)), None, FileStatus::Deleted),
        K::Modified {
            old_mode,
            new_mode,
            old,
            new,
        } => (
            Some((old_mode, old)),
            Some((new_mode, new)),
            if old_mode.is_file() != new_mode.is_file()
                || (old_mode != new_mode && !old_mode.is_file())
            {
                FileStatus::TypeChanged
            } else {
                FileStatus::Modified
            },
        ),
        K::ModeChanged {
            old_mode,
            new_mode,
            id,
        } => (
            Some((old_mode, id)),
            Some((new_mode, id)),
            FileStatus::ModeChanged,
        ),
    };
    let size = |item: Option<(Mode, Oid)>| -> Result<u64, Refusal> {
        match item {
            Some((Mode::Gitlink, _)) | None => Ok(0),
            Some((_, id)) => {
                let header = r.result(r.store.header(&id))?;
                if header.kind != "blob" {
                    return Err(crate::refuse::invalid("tree leaf must name a blob"));
                }
                Ok(header.len)
            }
        }
    };
    let old_size = size(old)?;
    let new_size = size(new)?;
    let mut result = FileDiff {
        old_path: old.map(|_| c.path.clone()),
        new_path: new.map(|_| c.path.clone()),
        status,
        old_oid: old.map(|(_, id)| id.to_hex()),
        new_oid: new.map(|(_, id)| id.to_hex()),
        old_kind: old.map(|(mode, _)| entry_kind(mode)),
        new_kind: new.map(|(mode, _)| entry_kind(mode)),
        old_size,
        new_size,
        content: Content::Text,
        additions: 0,
        deletions: 0,
        hunks: Vec::new(),
    };
    if [old, new]
        .into_iter()
        .flatten()
        .any(|(m, _)| m == Mode::Gitlink)
    {
        result.content = Content::Gitlink;
        return Ok(result);
    }
    if old_size > r.bounds.blob_bytes || new_size > r.bounds.blob_bytes {
        result.content = Content::Oversize;
        return Ok(result);
    }
    let read = |item: Option<(Mode, Oid)>| -> Result<Option<BlobView>, Refusal> {
        item.map(|(_, id)| r.blob(&id, None)).transpose()
    };
    let old_blob = read(old)?;
    let new_blob = read(new)?;
    if [&old_blob, &new_blob]
        .into_iter()
        .flatten()
        .any(|b| b.content == Content::Binary)
    {
        result.content = Content::Binary;
        return Ok(result);
    }
    let old = old_blob.as_ref().map_or(&[][..], |b| &b.bytes);
    let new = new_blob.as_ref().map_or(&[][..], |b| &b.bytes);
    result.hunks = r.result(hunks(old, new, cap(r.bounds.merge_cost)))?;
    for line in result.hunks.iter().flat_map(|h| &h.lines) {
        result.additions += u64::from(line.kind == LineKind::Added);
        result.deletions += u64::from(line.kind == LineKind::Deleted);
    }
    Ok(result)
}

fn hunks(old: &[u8], new: &[u8], cost: usize) -> crate::git::Result<Vec<DiffHunk>> {
    let a = diff::split_lines(old);
    let b = diff::split_lines(new);
    let edits = diff::lines(old, new, cost)?;
    let mut rows = Vec::new();
    for edit in edits {
        match edit.op {
            diff::Op::Equal => {
                for (i, j) in edit.old.zip(edit.new) {
                    rows.push((
                        i,
                        j,
                        DiffLine {
                            kind: LineKind::Context,
                            old_line: Some(i as u64 + 1),
                            new_line: Some(j as u64 + 1),
                            bytes: a[i].to_vec(),
                        },
                    ));
                }
            }
            diff::Op::Delete => {
                for i in edit.old {
                    rows.push((
                        i,
                        edit.new.start,
                        DiffLine {
                            kind: LineKind::Deleted,
                            old_line: Some(i as u64 + 1),
                            new_line: None,
                            bytes: a[i].to_vec(),
                        },
                    ));
                }
            }
            diff::Op::Insert => {
                for j in edit.new {
                    rows.push((
                        edit.old.start,
                        j,
                        DiffLine {
                            kind: LineKind::Added,
                            old_line: None,
                            new_line: Some(j as u64 + 1),
                            bytes: b[j].to_vec(),
                        },
                    ));
                }
            }
        }
    }
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, (_, _, line)) in rows.iter().enumerate() {
        if line.kind == LineKind::Context {
            continue;
        }
        let start = i.saturating_sub(3);
        let end = (i + 4).min(rows.len());
        if let Some(last) = groups.last_mut().filter(|last| start <= last.1) {
            last.1 = end;
        } else {
            groups.push((start, end));
        }
    }
    Ok(groups
        .into_iter()
        .map(|(start, end)| {
            let lines: Vec<_> = rows[start..end]
                .iter()
                .map(|(_, _, line)| line.clone())
                .collect();
            let old_count = lines.iter().filter(|l| l.old_line.is_some()).count() as u64;
            let new_count = lines.iter().filter(|l| l.new_line.is_some()).count() as u64;
            DiffHunk {
                old: LineRange {
                    start: rows[start].0 as u64 + u64::from(old_count != 0),
                    count: old_count,
                },
                new: LineRange {
                    start: rows[start].1 as u64 + u64::from(new_count != 0),
                    count: new_count,
                },
                lines,
            }
        })
        .collect())
}
