//! Consensus changes use refs and records only. No object possession influences acceptance.
use crate::contract::*;
use crate::discussion;
use crate::ops::require_writer;
use crate::repo::{load_bounds, load_ref, load_repo, parse_oid, repo_hash, resolve, set_ref};
use abi::{Env, Refusal};
use std::collections::BTreeSet;
use store::{Reads, Writes, capacity, invalid, not_found, stale, unauthorized, wrong_state};

pub fn prefix(repo: &str) -> Vec<u8> {
    format!("c/{repo}/").into_bytes()
}
pub fn key(repo: &str, n: u64) -> Vec<u8> {
    [prefix(repo), n.to_be_bytes().to_vec()].concat()
}
pub fn reviews_prefix(repo: &str, n: u64) -> Vec<u8> {
    format!("v/{repo}/{n:016x}/").into_bytes()
}
pub fn review_key(repo: &str, n: u64, id: u64) -> Vec<u8> {
    [reviews_prefix(repo, n), id.to_be_bytes().to_vec()].concat()
}
pub fn involved_prefix(actor: &[u8]) -> Vec<u8> {
    format!("i/{}/", abi::hex(actor)).into_bytes()
}
pub fn involved_key(actor: &[u8], repo: &str, n: u64) -> Vec<u8> {
    [
        involved_prefix(actor),
        format!("{repo}/{n:016x}").into_bytes(),
    ]
    .concat()
}
pub fn authored_prefix(repo: &str, n: u64, actor: &[u8]) -> Vec<u8> {
    format!("review-author/{repo}/{n:016x}/{}/", abi::hex(actor)).into_bytes()
}
pub fn latest_key(repo: &str, n: u64, actor: &[u8]) -> Vec<u8> {
    format!("l/{repo}/{n:016x}/{}", abi::hex(actor)).into_bytes()
}
pub fn load<S: Reads>(s: &S, repo: &str, n: u64) -> Result<Change, Refusal> {
    let bytes = s
        .get(key(repo, n))
        .ok_or_else(|| not_found(format!("no change {repo}#{n}")))?;
    abi::decode(&bytes)
}
pub fn save<S: Writes>(s: &mut S, repo: &str, change: &Change) {
    s.set(key(repo, change.n), abi::encode(change));
    for actor in std::iter::once(&change.author).chain(&change.reviewers) {
        involve(s, actor, repo, change.n);
    }
}
fn involve<S: Writes>(s: &mut S, actor: &[u8], repo: &str, n: u64) {
    s.set(involved_key(actor, repo, n), abi::encode(&(repo, n)));
}
fn fits(value: &impl borsh::BorshSerialize, bound: u64) -> Result<(), Refusal> {
    if abi::encode(value).len() as u64 > bound {
        return Err(capacity("record exceeds Bounds.record_bytes"));
    }
    Ok(())
}
fn title(text: &str) -> Result<(), Refusal> {
    if text.trim().is_empty() || text.len() > MAX_TITLE_BYTES {
        return Err(invalid(
            "title must be nonblank and at most MAX_TITLE_BYTES",
        ));
    }
    Ok(())
}
fn reviewers(keys: &[Vec<u8>]) -> Result<(), Refusal> {
    if keys.len() > MAX_REVIEWERS {
        return Err(capacity("too many requested reviewers"));
    }
    let mut seen = BTreeSet::new();
    if keys.iter().any(|k| k.is_empty() || !seen.insert(k)) {
        return Err(invalid("reviewer keys must be nonempty and distinct"));
    }
    Ok(())
}
pub fn path(path: &[u8], root: bool) -> Result<(), Refusal> {
    if root && path.is_empty() {
        return Ok(());
    }
    if path.len() > MAX_PATH_BYTES
        || path.contains(&0)
        || path
            .split(|b| *b == b'/')
            .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Err(invalid(
            "path must be a relative Git path without empty, dot or dot-dot components",
        ));
    }
    Ok(())
}
fn branch(name: &[u8]) -> Result<(), Refusal> {
    if !name.starts_with(b"refs/heads/") || !gitcore::server::valid_ref_name(name) {
        return Err(invalid(
            "change endpoints must name branches under refs/heads/",
        ));
    }
    Ok(())
}
fn author(change: &Change, actor: &[u8]) -> Result<(), Refusal> {
    if change.author != actor {
        return Err(unauthorized("only the author edits a change"));
    }
    Ok(())
}
fn open(change: &Change) -> Result<(), Refusal> {
    if change.state != ChangeState::Open {
        return Err(wrong_state("change is not open"));
    }
    Ok(())
}
fn changed(change: &mut Change, env: &Env) {
    change.updated_height = env.height;
    change.updated_time = env.time;
}
fn next(n: u64) -> Result<u64, Refusal> {
    n.checked_add(1)
        .ok_or_else(|| Refusal::new(abi::reason::EXHAUSTED, "counter exhausted"))
}

pub fn execute<S: Writes>(s: &mut S, env: &Env, actor: &[u8], op: Op) -> Result<(), Refusal> {
    let bounds = load_bounds(s)?;
    let reply = match op {
        Op::ChangeOpen {
            repo,
            mut from,
            into,
            title: text,
            body,
            reviewers: requested,
        } => {
            let record = load_repo(s, &repo)?;
            title(&text)?;
            reviewers(&requested)?;
            branch(&into)?;
            if let Revision::Ref(r) = &from {
                branch(r)?;
            }
            let hash = repo_hash(&record);
            let source = resolve(s, &repo, &from, hash)?;
            let target = resolve(s, &repo, &Revision::Ref(into.clone()), hash)?;
            if source == target {
                return Err(wrong_state("source and target already agree"));
            }
            if matches!(from, Revision::Oid(_)) {
                from = Revision::Oid(source.to_hex());
            }
            let counter = format!("item-number/{repo}").into_bytes();
            let n = next(
                s.get(&counter)
                    .map(|b| abi::decode(&b))
                    .transpose()?
                    .unwrap_or(0),
            )?;
            let change = Change {
                n,
                from,
                into,
                title: text,
                body,
                author: actor.to_vec(),
                state: ChangeState::Open,
                reviewers: requested,
                created_height: env.height,
                updated_height: env.height,
                created_time: env.time,
                updated_time: env.time,
                review_count: 0,
                comment_count: 0,
                verdicts: ReviewCounts::default(),
                merge_oid: None,
                channel: format!("forge:{repo}:{n}"),
                system_seq: 1,
            };
            fits(&change, bounds.record_bytes)?;
            let message = discussion::message_id(s)?;
            s.set(counter, abi::encode(&n));
            save(s, &repo, &change);
            discussion::create(s, &repo, &change);
            discussion::post(
                s,
                &change,
                message,
                format!("Opened by {}", abi::hex(actor)),
            );
            OpReply::Change {
                height: env.height,
                n,
            }
        }
        Op::ChangeEdit {
            repo,
            n,
            title: text,
            body,
            reviewers: requested,
        } => {
            load_repo(s, &repo)?;
            let mut change = load(s, &repo, n)?;
            author(&change, actor)?;
            if let Some(text) = text {
                title(&text)?;
                change.title = text;
            }
            if let Some(body) = body {
                change.body = body;
            }
            if let Some(keys) = requested {
                reviewers(&keys)?;
                change.reviewers = keys;
            }
            changed(&mut change, env);
            fits(&change, bounds.record_bytes)?;
            save(s, &repo, &change);
            OpReply::Change {
                height: env.height,
                n,
            }
        }
        Op::ChangeClose { repo, n } => {
            let record = load_repo(s, &repo)?;
            let mut change = load(s, &repo, n)?;
            if change.author != actor {
                require_writer(s, &repo, &record, actor)?;
            }
            open(&change)?;
            change.state = ChangeState::Closed;
            changed(&mut change, env);
            change.system_seq = next(change.system_seq)?;
            let message = discussion::message_id(s)?;
            save(s, &repo, &change);
            discussion::post(
                s,
                &change,
                message,
                format!("Closed by {}", abi::hex(actor)),
            );
            OpReply::Change {
                height: env.height,
                n,
            }
        }
        Op::ReviewSubmit {
            repo,
            n,
            mut review,
        } => {
            let record = load_repo(s, &repo)?;
            let mut change = load(s, &repo, n)?;
            let hash = repo_hash(&record);
            review.commit_oid = parse_oid(hash, &review.commit_oid)?.to_hex();
            review.base_oid = review
                .base_oid
                .map(|h| parse_oid(hash, &h).map(|o| o.to_hex()))
                .transpose()?;
            if review.comments.len() > MAX_REVIEW_COMMENTS {
                return Err(capacity("too many comments; use MAX_REVIEW_COMMENTS"));
            }
            let mut anchors = BTreeSet::new();
            for comment in &review.comments {
                path(&comment.path, false)?;
                if comment.line == 0
                    || comment.body.trim().is_empty()
                    || (comment.side == Side::Old && review.base_oid.is_none())
                {
                    return Err(invalid(
                        "comments need a positive line, nonblank body, and an old-side base",
                    ));
                }
                if !anchors.insert((&comment.path, comment.side, comment.line)) {
                    return Err(invalid("duplicate line anchor in one review"));
                }
            }
            if review.verdict == Verdict::Comment
                && review.body.trim().is_empty()
                && review.comments.is_empty()
            {
                return Err(invalid("a comment review needs text or line comments"));
            }
            let id = next(change.review_count)?;
            let mut row = Review {
                id,
                author: actor.to_vec(),
                height: env.height,
                time: env.time,
                draft: review,
                message_id: "forge:0000000000000000".into(),
            };
            fits(&row, bounds.record_bytes)?;
            change.review_count = id;
            change.comment_count = change
                .comment_count
                .checked_add(row.draft.comments.len() as u64)
                .ok_or_else(|| capacity("comment counter exhausted"))?;
            match row.draft.verdict {
                Verdict::Approve => change.verdicts.approve = next(change.verdicts.approve)?,
                Verdict::RequestChanges => {
                    change.verdicts.request_changes = next(change.verdicts.request_changes)?
                }
                Verdict::Comment => change.verdicts.comment = next(change.verdicts.comment)?,
            }
            changed(&mut change, env);
            change.system_seq = next(change.system_seq)?;
            row.message_id = discussion::message_id(s)?;
            s.set(review_key(&repo, n, id), abi::encode(&row));
            let author_index =
                [authored_prefix(&repo, n, actor), id.to_be_bytes().to_vec()].concat();
            s.set(author_index, abi::encode(&id));
            s.set(latest_key(&repo, n, actor), abi::encode(&id));
            involve(s, actor, &repo, n);
            save(s, &repo, &change);
            discussion::post(
                s,
                &change,
                row.message_id.clone(),
                format!(
                    "Review {id} submitted by {}: {:?}; {} line comments",
                    abi::hex(actor),
                    row.draft.verdict,
                    row.draft.comments.len()
                ),
            );
            OpReply::Review {
                height: env.height,
                n,
                id,
            }
        }
        Op::Merge {
            repo,
            into,
            from,
            expected_into,
            expected_from,
            result,
            change,
        } => {
            let record = load_repo(s, &repo)?;
            require_writer(s, &repo, &record, actor)?;
            branch(&into)?;
            if let Revision::Ref(r) = &from {
                branch(r)?;
            }
            let hash = repo_hash(&record);
            let expected_into = parse_oid(hash, &expected_into)?;
            let expected_from = parse_oid(hash, &expected_from)?;
            let result = parse_oid(hash, &result)?;
            if load_ref(s, &repo, &into, hash)? != Some(expected_into)
                || resolve(s, &repo, &from, hash)? != expected_from
            {
                return Err(stale("source or target head moved; recompute the merge"));
            }
            if result == expected_into || expected_from == expected_into {
                return Err(wrong_state("merge does not advance the target"));
            }
            let mut item = change.map(|n| load(s, &repo, n)).transpose()?;
            if let Some(item) = &mut item {
                open(item)?;
                let same_source = match (&item.from, &from) {
                    (Revision::Oid(a), Revision::Oid(b)) => {
                        parse_oid(hash, a)? == parse_oid(hash, b)?
                    }
                    (a, b) => a == b,
                };
                if item.into != into || !same_source {
                    return Err(invalid("merge endpoints differ from the change"));
                }
                item.state = ChangeState::Merged;
                item.merge_oid = Some(result.to_hex());
                changed(item, env);
                item.system_seq = next(item.system_seq)?;
                fits(item, bounds.record_bytes)?;
                let message = discussion::message_id(s)?;
                save(s, &repo, item);
                discussion::post(
                    s,
                    item,
                    message,
                    format!("Merged by {} as {result}", abi::hex(actor)),
                );
            }
            set_ref(s, &repo, &into, &result);
            OpReply::Merged {
                height: env.height,
                oid: result.to_hex(),
                change,
            }
        }
        _ => return Err(invalid("not a change operation")),
    };
    s.output(abi::encode(&reply));
    Ok(())
}
