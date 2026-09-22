//! Bounded record pages; judgment joins the latest authored review with its chat root.
use crate::Sandbox;
use crate::changes::{self, involved_key, latest_key, load, review_key};
use crate::contract::*;
use crate::paging::Paging;
use crate::repo::{load_ref, load_repo, repo_hash};
use abi::Refusal;

fn summary(repo: &str, c: &Change) -> ChangeSummary {
    ChangeSummary {
        repo: repo.into(),
        n: c.n,
        from: c.from.clone(),
        into: c.into.clone(),
        title: c.title.clone(),
        author: c.author.clone(),
        state: c.state,
        updated_height: c.updated_height,
        review_count: c.review_count,
        comment_count: c.comment_count,
        verdicts: c.verdicts,
    }
}
fn heads<S: Sandbox>(
    s: &S,
    repo: &str,
    c: &Change,
) -> Result<(Option<String>, Option<String>), Refusal> {
    let record = load_repo(s, repo)?;
    let hash = repo_hash(&record);
    let source = match &c.from {
        Revision::Ref(r) => load_ref(s, repo, r, hash)?.map(|o| o.to_hex()),
        Revision::Oid(oid) => Some(oid.clone()),
    };
    Ok((
        source,
        load_ref(s, repo, &c.into, hash)?.map(|o| o.to_hex()),
    ))
}
pub fn answer<S: Sandbox>(s: &S, height: u64, q: &Query, p: &Paging) -> Result<Reply, Refusal> {
    Ok(match q {
        Query::Changes { repo, filter, .. } => {
            load_repo(s, repo)?;
            let entries = p.entries(s, &changes::prefix(repo))?;
            let mut items = Vec::new();
            for entry in entries.items {
                let c: Change = abi::decode(&entry.value)?;
                if filter.state.is_some_and(|state| c.state != state)
                    || filter.author.as_ref().is_some_and(|a| &c.author != a)
                    || filter
                        .involves
                        .as_ref()
                        .is_some_and(|a| s.get(&involved_key(a, repo, c.n)).is_none())
                {
                    continue;
                }
                items.push(summary(repo, &c));
            }
            Reply::Changes {
                height,
                page: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Change { repo, n, .. } => {
            let c = load(s, repo, *n)?;
            let (source_head, target_head) = heads(s, repo, &c)?;
            let entries = p.entries(s, &changes::reviews_prefix(repo, *n))?;
            let items = entries
                .items
                .into_iter()
                .map(|e| abi::decode(&e.value))
                .collect::<Result<_, _>>()?;
            Reply::Change {
                height,
                change: c,
                source_head,
                target_head,
                reviews: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Judgment { key, .. } => {
            if key.is_empty() {
                return Err(crate::refuse::invalid("judgment needs a key"));
            }
            let entries = p.entries(s, &changes::involved_prefix(key))?;
            let mut items = Vec::new();
            for entry in entries.items {
                let (repo, n): (String, u64) = abi::decode(&entry.value)?;
                let c = load(s, &repo, n)?;
                if c.state != ChangeState::Open {
                    continue;
                }
                let (source, _) = heads(s, &repo, &c)?;
                let latest: Option<Review> = match s.get(&latest_key(&repo, n, key)) {
                    Some(bytes) => {
                        let id: u64 = abi::decode(&bytes)?;
                        let bytes = s
                            .get(&review_key(&repo, n, id))
                            .ok_or_else(|| crate::refuse::storage("latest review missing"))?;
                        Some(abi::decode(&bytes)?)
                    }
                    None => None,
                };
                let requested = c.reviewers.contains(key)
                    && latest
                        .as_ref()
                        .is_none_or(|r| source.as_ref() != Some(&r.draft.commit_oid));
                let replies = match latest {
                    Some(review) => {
                        crate::discussion::message(s, &review.message_id)?.and_then(|root| {
                            root.last_reply_seq.map(|last_reply_seq| ReplyAttention {
                                review: review.id,
                                root_seq: root.seq,
                                last_reply_seq,
                            })
                        })
                    }
                    None => None,
                };
                if requested || replies.is_some() {
                    items.push(Judgment {
                        change: summary(&repo, &c),
                        requested,
                        replies,
                    });
                }
            }
            Reply::Judgment {
                height,
                page: Page {
                    items,
                    next: entries.next,
                },
            }
        }
        _ => return Err(crate::refuse::invalid("not a change query")),
    })
}
