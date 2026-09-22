//! Bounded record pages; judgment joins the latest authored review with its chat root.
use crate::changes::{self, involved_key, latest_key, load, review_key};
use crate::contract::*;
use crate::ops::storage;
use crate::repo::{load_ref, load_repo, repo_hash};
use abi::{Refusal, Scan};
use store::{Listing, Reads, capacity, invalid};

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
fn heads<S: Reads>(
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
pub fn answer<S: Reads>(s: &S, height: u64, q: &Query, p: &Listing) -> Result<Reply, Refusal> {
    Ok(match q {
        Query::Changes { repo, filter, .. } => {
            load_repo(s, repo)?;
            let entries = p.reply(s.records::<Change>(p.scan_ahead(&changes::prefix(repo)))?);
            let mut items = Vec::new();
            for c in entries.items {
                if filter.state.is_some_and(|state| c.state != state)
                    || filter.author.as_ref().is_some_and(|a| &c.author != a)
                    || filter
                        .involves
                        .as_ref()
                        .is_some_and(|a| s.get(involved_key(a, repo, c.n)).is_none())
                {
                    continue;
                }
                items.push(summary(repo, &c));
            }
            Reply::Changes {
                height,
                page: PageReply {
                    height,
                    items,
                    next: entries.next,
                },
            }
        }
        Query::Change { repo, n, .. } => {
            let c = load(s, repo, *n)?;
            let (source_head, target_head) = heads(s, repo, &c)?;
            let reviews =
                p.reply(s.records::<Review>(p.scan_ahead(&changes::reviews_prefix(repo, *n)))?);
            Reply::Change {
                height,
                change: c,
                source_head,
                target_head,
                reviews,
            }
        }
        Query::Judgment { key, .. } => {
            if key.is_empty() {
                return Err(invalid("judgment needs a key"));
            }
            // Chat participants need not have submitted a forge op, so page all changes.
            let entries = p.reply(
                s.scan(p.scan_ahead(b"c/"))
                    .into_iter()
                    .map(|e| (e.key.clone(), e)),
            );
            let mut remaining = crate::repo::load_bounds(s)?.log_walk;
            let mut items = Vec::new();
            for entry in entries.items {
                let c: Change = abi::decode(&entry.value)?;
                let rest = &entry.key[2..];
                let slash = rest
                    .iter()
                    .position(|b| *b == b'/')
                    .ok_or_else(|| storage("invalid change key"))?;
                let repo = std::str::from_utf8(&rest[..slash])
                    .map_err(|_| storage("invalid repo key"))?
                    .to_owned();
                let n = c.n;
                if c.state != ChangeState::Open {
                    continue;
                }
                let (source, _) = heads(s, &repo, &c)?;
                let latest: Option<Review> = match s.get(latest_key(&repo, n, key)) {
                    Some(bytes) => {
                        let id: u64 = abi::decode(&bytes)?;
                        let bytes = s
                            .get(review_key(&repo, n, id))
                            .ok_or_else(|| storage("latest review missing"))?;
                        Some(abi::decode(&bytes)?)
                    }
                    None => None,
                };
                let requested = c.reviewers.contains(key)
                    && latest
                        .as_ref()
                        .is_none_or(|r| source.as_ref() != Some(&r.draft.commit_oid));
                let mut replies =
                    crate::discussion::attention(s, &c.channel, key)?.and_then(|root| {
                        root.last_reply_seq.map(|last_reply_seq| ReplyAttention {
                            review: None,
                            root_seq: root.seq,
                            last_reply_seq,
                        })
                    });
                let authored = s.scan(
                    Scan::prefix(changes::authored_prefix(&repo, n, key))
                        .reverse()
                        .limit(remaining.saturating_add(1)),
                );
                if authored.len() as u64 > remaining {
                    return Err(capacity("judgment review walk exceeds Bounds.log_walk"));
                }
                remaining -= authored.len() as u64;
                for entry in authored {
                    let id: u64 = abi::decode(&entry.value)?;
                    let bytes = s
                        .get(review_key(&repo, n, id))
                        .ok_or_else(|| storage("authored review missing"))?;
                    let review: Review = abi::decode(&bytes)?;
                    if let Some(root) = crate::discussion::message(s, &review.message_id)?
                        && let Some(last_reply_seq) = root.last_reply_seq
                        && replies
                            .as_ref()
                            .is_none_or(|r| r.last_reply_seq < last_reply_seq)
                    {
                        replies = Some(ReplyAttention {
                            review: Some(id),
                            root_seq: root.seq,
                            last_reply_seq,
                        });
                    }
                }
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
                page: PageReply {
                    height,
                    items,
                    next: entries.next,
                },
            }
        }
        _ => return Err(invalid("not a change query")),
    })
}
