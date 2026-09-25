// The change queries: a repository's changes, one change with its reviews, and the judgment a key owes across every repository.

use abi::{Refusal, Scan};
use store::{Listing, Reads, capacity, invalid};

use crate::contract::*;
use crate::discussion;
use crate::state::{
    AUTHORED, CHANGES, INVOLVED, REVIEWS, load_bounds, load_change, load_ref, load_repo,
    load_review, repo_hash,
};

pub fn answer(
    store: &impl Reads,
    height: u64,
    query: &Query,
    listing: &Listing,
) -> Result<Reply, Refusal> {
    Ok(match query {
        Query::Changes { repo, filter, .. } => Reply::Changes {
            height,
            page: changes(store, repo, filter, listing)?,
        },
        Query::Change { repo, n, .. } => {
            let change = load_change(store, repo, *n)?;
            let (source_head, target_head) = heads(store, repo, &change)?;
            let reviews = REVIEWS
                .page_of(store, &(repo.clone(), *n), listing)?
                .map(|(_, review)| review);
            Reply::Change {
                height,
                change,
                source_head,
                target_head,
                reviews,
            }
        }
        Query::Judgment { key, .. } => Reply::Judgment {
            height,
            page: judgment(store, key, listing)?,
        },
        _ => return Err(invalid("not a change query")),
    })
}

/// One page of a repository's changes, filtered. A filter can empty a page
/// that still has a `next`.
fn changes(
    store: &impl Reads,
    repo: &str,
    filter: &ChangeFilter,
    listing: &Listing,
) -> Result<PageReply<ChangeSummary>, Refusal> {
    load_repo(store, repo)?;
    let PageReply {
        height,
        items,
        next,
    } = CHANGES.page_of(store, &repo.to_owned(), listing)?;
    let items = items
        .into_iter()
        .filter(|((repo, n), change)| {
            filter.state.is_none_or(|state| change.state == state)
                && filter
                    .author
                    .as_ref()
                    .is_none_or(|key| &change.author == key)
                && filter
                    .involves
                    .as_ref()
                    .is_none_or(|key| INVOLVED.has(store, &(key.clone(), repo.clone(), *n)))
        })
        .map(|((repo, _), change)| summary(&repo, &change))
        .collect();
    Ok(PageReply {
        height,
        items,
        next,
    })
}

/// One page of every open change, across repositories, that waits on `key`:
/// its review is requested at the current head, or a thread it started was
/// answered. Chat participants need not have submitted a forge op, so every
/// change is paged.
fn judgment(
    store: &impl Reads,
    key: &[u8],
    listing: &Listing,
) -> Result<PageReply<Judgment>, Refusal> {
    if key.is_empty() {
        return Err(invalid("judgment needs a key"));
    }
    let mut budget = load_bounds(store)?.log_walk;
    let page = CHANGES.page_of(store, &(), listing)?;
    let mut items = Vec::new();
    for ((repo, _), change) in &page.items {
        if change.state != ChangeState::Open {
            continue;
        }
        if let Some(judgment) = judge(store, repo, change, key, &mut budget)? {
            items.push(judgment);
        }
    }
    Ok(PageReply {
        height: page.height,
        items,
        next: page.next,
    })
}

/// What `key` owes one open change, if anything.
fn judge(
    store: &impl Reads,
    repo: &str,
    change: &Change,
    key: &[u8],
    budget: &mut u64,
) -> Result<Option<Judgment>, Refusal> {
    let (source, _) = heads(store, repo, change)?;
    let authored = authored_newest_first(store, repo, change.n, key, budget)?;
    let latest = authored
        .first()
        .map(|id| load_review(store, repo, change.n, *id))
        .transpose()?;
    let requested = change.reviewers.iter().any(|reviewer| reviewer == key)
        && latest
            .as_ref()
            .is_none_or(|review| source.as_ref() != Some(&review.draft.commit_oid));
    let mut replies = discussion::attention(store, &change.channel, key)?.and_then(|root| {
        root.last_reply_seq.map(|last_reply_seq| ReplyAttention {
            review: None,
            root_seq: root.seq,
            last_reply_seq,
        })
    });
    for id in authored {
        let review = load_review(store, repo, change.n, id)?;
        if let Some(root) = discussion::message(store, &review.message_id)?
            && let Some(last_reply_seq) = root.last_reply_seq
            && replies
                .as_ref()
                .is_none_or(|newest| newest.last_reply_seq < last_reply_seq)
        {
            replies = Some(ReplyAttention {
                review: Some(id),
                root_seq: root.seq,
                last_reply_seq,
            });
        }
    }
    Ok((requested || replies.is_some()).then(|| Judgment {
        change: summary(repo, change),
        requested,
        replies,
    }))
}

/// The ids of the reviews `key` submitted on a change, newest first, each
/// one spent from the query's `Bounds.log_walk` budget.
fn authored_newest_first(
    store: &impl Reads,
    repo: &str,
    n: u64,
    key: &[u8],
    budget: &mut u64,
) -> Result<Vec<u64>, Refusal> {
    let scan: Scan = AUTHORED.prefix_of(&(repo.to_owned(), n, key.to_vec()));
    let ids: Vec<u64> = AUTHORED
        .scan(store, scan.reverse().limit(budget.saturating_add(1)))?
        .into_iter()
        .map(|(_, _, _, id)| id)
        .collect();
    if ids.len() as u64 > *budget {
        return Err(capacity("judgment review walk exceeds Bounds.log_walk"));
    }
    *budget -= ids.len() as u64;
    Ok(ids)
}

/// The change's two current endpoints; either can be gone.
fn heads(
    store: &impl Reads,
    repo: &str,
    change: &Change,
) -> Result<(Option<String>, Option<String>), Refusal> {
    let hash = repo_hash(&load_repo(store, repo)?);
    let source = match &change.from {
        Revision::Ref(name) => load_ref(store, repo, name, hash)?.map(|oid| oid.to_hex()),
        Revision::Oid(oid) => Some(oid.clone()),
    };
    let target = load_ref(store, repo, &change.into, hash)?.map(|oid| oid.to_hex());
    Ok((source, target))
}

fn summary(repo: &str, change: &Change) -> ChangeSummary {
    ChangeSummary {
        repo: repo.into(),
        n: change.n,
        from: change.from.clone(),
        into: change.into.clone(),
        title: change.title.clone(),
        author: change.author.clone(),
        state: change.state,
        updated_height: change.updated_height,
        review_count: change.review_count,
        comment_count: change.comment_count,
        verdicts: change.verdicts,
    }
}
