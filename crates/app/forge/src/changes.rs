// The change ops: open, edit, close, review and merge. Acceptance reads refs and records only; no object a node holds or lacks decides an op.

use std::collections::BTreeSet;

use abi::{Env, Refusal};
use store::{Writes, capacity, invalid, stale, unauthorized, wrong_state};

use crate::contract::*;
use crate::discussion;
use crate::ops::require_writer;
use crate::state::{
    load_bounds, load_change, load_ref, load_repo, next, next_message, next_number, parse_oid,
    repo_hash, resolve, save_change, save_review, set_ref,
};

pub fn execute(store: &mut impl Writes, env: &Env, actor: &[u8], op: Op) -> Result<(), Refusal> {
    let reply = match op {
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
            open(store, env, actor, &repo, draft)?
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
            edit(store, env, actor, &repo, n, fields)?
        }
        Op::ChangeClose { repo, n } => close(store, env, actor, &repo, n)?,
        Op::ReviewSubmit { repo, n, review } => submit_review(store, env, actor, &repo, n, review)?,
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
            merge_heads(store, env, actor, &repo, merge)?
        }
        _ => return Err(invalid("not a change operation")),
    };
    store.output(abi::encode(&reply));
    Ok(())
}

/// What `ChangeOpen` carries besides its repository.
struct Draft {
    from: Revision,
    into: Vec<u8>,
    title: String,
    body: String,
    reviewers: Vec<Vec<u8>>,
}

fn open(
    store: &mut impl Writes,
    env: &Env,
    actor: &[u8],
    repo: &str,
    mut draft: Draft,
) -> Result<OpReply, Refusal> {
    let record = load_repo(store, repo)?;
    check_title(&draft.title)?;
    check_reviewers(&draft.reviewers)?;
    check_endpoints(&draft.into, &draft.from)?;
    let hash = repo_hash(&record);
    let source = resolve(store, repo, &draft.from, hash)?;
    let target = resolve(store, repo, &Revision::Ref(draft.into.clone()), hash)?;
    if source == target {
        return Err(wrong_state("source and target already agree"));
    }
    if matches!(draft.from, Revision::Oid(_)) {
        draft.from = Revision::Oid(source.to_hex());
    }
    let n = next_number(store, repo)?;
    let change = Change {
        n,
        from: draft.from,
        into: draft.into,
        title: draft.title,
        body: draft.body,
        author: actor.to_vec(),
        state: ChangeState::Open,
        reviewers: draft.reviewers,
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
    fits(store, &change)?;
    let message = next_message(store)?;
    save_change(store, repo, &change)?;
    discussion::create(store, repo, &change);
    discussion::post(
        store,
        &change,
        message,
        format!("Opened by {}", abi::hex(actor)),
    );
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// What `ChangeEdit` changes; `None` leaves a field as it is.
struct Edit {
    title: Option<String>,
    body: Option<String>,
    reviewers: Option<Vec<Vec<u8>>>,
}

fn edit(
    store: &mut impl Writes,
    env: &Env,
    actor: &[u8],
    repo: &str,
    n: u64,
    fields: Edit,
) -> Result<OpReply, Refusal> {
    load_repo(store, repo)?;
    let mut change = load_change(store, repo, n)?;
    if change.author != actor {
        return Err(unauthorized("only the author edits a change"));
    }
    if let Some(title) = fields.title {
        check_title(&title)?;
        change.title = title;
    }
    if let Some(body) = fields.body {
        change.body = body;
    }
    if let Some(reviewers) = fields.reviewers {
        check_reviewers(&reviewers)?;
        change.reviewers = reviewers;
    }
    touched(&mut change, env);
    fits(store, &change)?;
    save_change(store, repo, &change)?;
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// Closing is terminal: the author or a writer ends an open change.
fn close(
    store: &mut impl Writes,
    env: &Env,
    actor: &[u8],
    repo: &str,
    n: u64,
) -> Result<OpReply, Refusal> {
    let record = load_repo(store, repo)?;
    let mut change = load_change(store, repo, n)?;
    if change.author != actor {
        require_writer(store, repo, &record, actor)?;
    }
    require_open(&change)?;
    change.state = ChangeState::Closed;
    touched(&mut change, env);
    change.system_seq = next(change.system_seq)?;
    let message = next_message(store)?;
    save_change(store, repo, &change)?;
    discussion::post(
        store,
        &change,
        message,
        format!("Closed by {}", abi::hex(actor)),
    );
    Ok(OpReply::Change {
        height: env.height,
        n,
    })
}

/// One immutable review: a verdict, a body and its line comments, pinned
/// at the commits it read. Reviews stay appendable after a change ends.
fn submit_review(
    store: &mut impl Writes,
    env: &Env,
    actor: &[u8],
    repo: &str,
    n: u64,
    mut draft: ReviewDraft,
) -> Result<OpReply, Refusal> {
    let record = load_repo(store, repo)?;
    let mut change = load_change(store, repo, n)?;
    let hash = repo_hash(&record);
    draft.commit_oid = parse_oid(hash, &draft.commit_oid)?.to_hex();
    draft.base_oid = draft
        .base_oid
        .map(|hex| parse_oid(hash, &hex).map(|oid| oid.to_hex()))
        .transpose()?;
    check_comments(&draft)?;
    let id = next(change.review_count)?;
    let mut review = Review {
        id,
        author: actor.to_vec(),
        height: env.height,
        time: env.time,
        draft,
        message_id: "forge:0000000000000000".into(),
    };
    fits(store, &review)?;
    change.review_count = id;
    change.comment_count = change
        .comment_count
        .checked_add(review.draft.comments.len() as u64)
        .ok_or_else(|| capacity("comment counter exhausted"))?;
    count_verdict(&mut change.verdicts, review.draft.verdict)?;
    touched(&mut change, env);
    change.system_seq = next(change.system_seq)?;
    review.message_id = next_message(store)?;
    save_review(store, repo, n, &review);
    save_change(store, repo, &change)?;
    discussion::post(
        store,
        &change,
        review.message_id.clone(),
        format!(
            "Review {id} submitted by {}: {:?}; {} line comment{}",
            abi::hex(actor),
            review.draft.verdict,
            review.draft.comments.len(),
            if review.draft.comments.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
    );
    Ok(OpReply::Review {
        height: env.height,
        n,
        id,
    })
}

/// What `Merge` carries besides its repository.
struct MergeRequest {
    into: Vec<u8>,
    from: Revision,
    expected_into: String,
    expected_from: String,
    result: String,
    change: Option<u64>,
}

/// A compare-and-swap of both heads: the client built and published the
/// result; forge only checks that neither endpoint moved since.
fn merge_heads(
    store: &mut impl Writes,
    env: &Env,
    actor: &[u8],
    repo: &str,
    merge: MergeRequest,
) -> Result<OpReply, Refusal> {
    let record = load_repo(store, repo)?;
    require_writer(store, repo, &record, actor)?;
    check_endpoints(&merge.into, &merge.from)?;
    let hash = repo_hash(&record);
    let expected_into = parse_oid(hash, &merge.expected_into)?;
    let expected_from = parse_oid(hash, &merge.expected_from)?;
    let result = parse_oid(hash, &merge.result)?;
    let heads_moved = load_ref(store, repo, &merge.into, hash)? != Some(expected_into)
        || resolve(store, repo, &merge.from, hash)? != expected_from;
    if heads_moved {
        return Err(stale("source or target head moved; recompute the merge"));
    }
    if result == expected_into || expected_from == expected_into {
        return Err(wrong_state("merge does not advance the target"));
    }
    if let Some(n) = merge.change {
        let mut change = load_change(store, repo, n)?;
        require_open(&change)?;
        let same_source = match (&change.from, &merge.from) {
            (Revision::Oid(a), Revision::Oid(b)) => parse_oid(hash, a)? == parse_oid(hash, b)?,
            (a, b) => a == b,
        };
        if change.into != merge.into || !same_source {
            return Err(invalid("merge endpoints differ from the change"));
        }
        change.state = ChangeState::Merged;
        change.merge_oid = Some(result.to_hex());
        touched(&mut change, env);
        change.system_seq = next(change.system_seq)?;
        fits(store, &change)?;
        let message = next_message(store)?;
        save_change(store, repo, &change)?;
        discussion::post(
            store,
            &change,
            message,
            format!("Merged by {} as {result}", abi::hex(actor)),
        );
    }
    set_ref(store, repo, &merge.into, &result);
    Ok(OpReply::Merged {
        height: env.height,
        oid: result.to_hex(),
        change: merge.change,
    })
}

fn touched(change: &mut Change, env: &Env) {
    change.updated_height = env.height;
    change.updated_time = env.time;
}

fn count_verdict(counts: &mut ReviewCounts, verdict: Verdict) -> Result<(), Refusal> {
    let count = match verdict {
        Verdict::Approve => &mut counts.approve,
        Verdict::RequestChanges => &mut counts.request_changes,
        Verdict::Comment => &mut counts.comment,
    };
    *count = next(*count)?;
    Ok(())
}

fn require_open(change: &Change) -> Result<(), Refusal> {
    if change.state != ChangeState::Open {
        return Err(wrong_state("change is not open"));
    }
    Ok(())
}

/// A record over `Bounds.record_bytes` is refused whole.
fn fits(store: &impl store::Reads, record: &impl borsh::BorshSerialize) -> Result<(), Refusal> {
    let bound = load_bounds(store)?.record_bytes;
    if abi::encode(record).len() as u64 > bound {
        return Err(capacity("record exceeds Bounds.record_bytes"));
    }
    Ok(())
}

fn check_title(title: &str) -> Result<(), Refusal> {
    if title.trim().is_empty() || title.len() > MAX_TITLE_BYTES {
        return Err(invalid(
            "title must be nonblank and at most MAX_TITLE_BYTES",
        ));
    }
    Ok(())
}

fn check_reviewers(keys: &[Vec<u8>]) -> Result<(), Refusal> {
    if keys.len() > MAX_REVIEWERS {
        return Err(capacity("too many requested reviewers"));
    }
    let mut seen = BTreeSet::new();
    let well_named = keys
        .iter()
        .all(|key| !key.is_empty() && key.len() <= MAX_KEY_BYTES && seen.insert(key));
    if !well_named {
        return Err(invalid("reviewer keys must be nonempty and distinct"));
    }
    Ok(())
}

/// Both ends of a change or a merge are branches under `refs/heads/`.
fn check_endpoints(into: &[u8], from: &Revision) -> Result<(), Refusal> {
    check_branch(into)?;
    if let Revision::Ref(name) = from {
        check_branch(name)?;
    }
    Ok(())
}

fn check_branch(name: &[u8]) -> Result<(), Refusal> {
    if !name.starts_with(b"refs/heads/") || !gitcore::server::valid_ref_name(name) {
        return Err(invalid(
            "change endpoints must name branches under refs/heads/",
        ));
    }
    Ok(())
}

fn check_comments(draft: &ReviewDraft) -> Result<(), Refusal> {
    if draft.comments.len() > MAX_REVIEW_COMMENTS {
        return Err(capacity("too many comments; use MAX_REVIEW_COMMENTS"));
    }
    let mut anchors = BTreeSet::new();
    for comment in &draft.comments {
        check_path(&comment.path, false)?;
        let anchored = comment.line > 0
            && !comment.body.trim().is_empty()
            && (comment.side == Side::New || draft.base_oid.is_some());
        if !anchored {
            return Err(invalid(
                "comments need a positive line, nonblank body, and an old-side base",
            ));
        }
        if !anchors.insert((&comment.path, comment.side, comment.line)) {
            return Err(invalid("duplicate line anchor in one review"));
        }
    }
    let says_nothing = draft.verdict == Verdict::Comment
        && draft.body.trim().is_empty()
        && draft.comments.is_empty();
    if says_nothing {
        return Err(invalid("a comment review needs text or line comments"));
    }
    Ok(())
}

/// A relative git path with no empty, `.` or `..` component; `root` admits
/// the empty path (a tree's root).
pub fn check_path(path: &[u8], root: bool) -> Result<(), Refusal> {
    if root && path.is_empty() {
        return Ok(());
    }
    let well_formed = path.len() <= MAX_PATH_BYTES
        && !path.contains(&0)
        && path
            .split(|b| *b == b'/')
            .all(|part| !part.is_empty() && part != b"." && part != b"..");
    if !well_formed {
        return Err(invalid(
            "path must be a relative Git path without empty, dot or dot-dot components",
        ));
    }
    Ok(())
}
