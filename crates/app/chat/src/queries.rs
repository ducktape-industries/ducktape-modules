use super::*;

// ── query ───────────────────────────────────────────────────────────────────

pub fn page(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE)
}

/// `limit + 1` entries under `scan`, split into the page and `has_more`.
fn paged(store: &impl Read, scan: Scan, limit: usize) -> (Vec<Entry>, bool) {
    let mut entries = store.scan(scan.limit(limit as u64 + 1));
    let more = entries.len() > limit;
    entries.truncate(limit);
    (entries, more)
}

fn rows_at(
    store: &impl Read,
    keys: impl IntoIterator<Item = String>,
) -> Result<Vec<MsgRow>, Refusal> {
    keys.into_iter()
        .filter_map(|k| load(store, &k).transpose())
        .collect()
}

/// A posting's row, by the `(channel, seq)` it names.
fn posted(store: &impl Read, entries: &[Entry]) -> Result<Vec<MsgRow>, Refusal> {
    let keys = entries
        .iter()
        .filter_map(|e| serde_json::from_slice::<(String, u64)>(&e.value).ok())
        .map(|(ch, seq)| msg_key(&ch, seq));
    rows_at(store, keys)
}

fn hydrate(store: &impl Read, rows: &mut [MsgRow], viewer: &[String]) {
    for row in rows {
        for r in &mut row.reactions {
            r.reacted_by_me = viewer.iter().any(|h| {
                store
                    .get(react_key(&row.channel_id, row.seq, &r.emoji, h).as_bytes())
                    .is_some()
            });
        }
    }
}

fn key_tail(entry: &Entry) -> String {
    let key = String::from_utf8_lossy(&entry.key);
    key.rsplit('/').next().unwrap_or_default().to_string()
}

pub fn query(store: &impl Read, q: ChatViewQuery) -> Result<ChatViewReply, Refusal> {
    Ok(match q {
        ChatViewQuery::Accounts { .. } => {
            return Err(refuse(
                reason::UNSUPPORTED,
                "accounts are identity's, asked by the program",
            ));
        }
        ChatViewQuery::Channels { after, limit } => {
            let mut scan = Scan::prefix(b"chan/");
            if let Some(after) = after {
                scan = scan.after(chan_key(&after));
            }
            let (entries, has_more) = paged(store, scan, page(limit));
            let channels: Vec<ChannelInfo> = entries
                .iter()
                .filter_map(|e| serde_json::from_slice::<ChannelRow>(&e.value).ok())
                .map(|channel| ChannelInfo {
                    head_seq: head_seq(store, &channel.id),
                    channel,
                })
                .collect();
            let next_after = has_more
                .then(|| channels.last().map(|c| c.channel.id.clone()))
                .flatten();
            ChatViewReply::Channels {
                channels,
                has_more,
                next_after,
            }
        }
        ChatViewQuery::MessageById { message_id } => {
            let address: Option<(String, u64)> = load(store, &msgid_key(&message_id))?;
            ChatViewReply::Message(match address {
                Some((ch, seq)) => Some(row(store, &ch, seq)?),
                None => None,
            })
        }
        ChatViewQuery::Channel { channel_id } => ChatViewReply::Channel(
            load::<ChannelRow>(store, &chan_key(&channel_id))?.map(|channel| ChannelInfo {
                head_seq: head_seq(store, &channel_id),
                channel,
            }),
        ),
        ChatViewQuery::Roots {
            channel_id,
            viewer_handles,
            before_seq,
            limit,
        } => {
            let mut scan = Scan::prefix(format!("root/{channel_id}/"));
            if let Some(before) = before_seq {
                scan.lo = root_key(&channel_id, before.saturating_sub(1)).into_bytes();
            }
            let (entries, has_more) = paged(store, scan, page(limit));
            let seqs: Vec<u64> = entries
                .iter()
                .rev()
                .filter_map(|e| u64::from_str_radix(&key_tail(e), 16).ok())
                .map(|r| u64::MAX - r)
                .collect();
            let mut roots = rows_at(store, seqs.iter().map(|s| msg_key(&channel_id, *s)))?;
            hydrate(store, &mut roots, &viewer_handles);
            ChatViewReply::Roots {
                next_before_seq: has_more.then(|| seqs.first().copied()).flatten(),
                roots,
                has_more,
            }
        }
        ChatViewQuery::MessagesAround {
            channel_id,
            seq,
            viewer_handles,
            limit,
        } => {
            let half = (page(limit) / 2) as u64;
            let lo = msg_key(&channel_id, seq.saturating_sub(half));
            let hi = msg_key(&channel_id, seq.saturating_add(half + 1));
            let entries = store.scan(Scan::range(lo, Some(hi.into_bytes())));
            let mut rows: Vec<MsgRow> = entries
                .iter()
                .filter_map(|e| serde_json::from_slice(&e.value).ok())
                .collect();
            hydrate(store, &mut rows, &viewer_handles);
            ChatViewReply::Messages(rows)
        }
        ChatViewQuery::Thread {
            channel_id,
            root_seq,
            viewer_handles,
            after_reply_seq,
            limit,
        } => {
            let mut root = load::<MsgRow>(store, &msg_key(&channel_id, root_seq))?;
            let mut scan = Scan::prefix(format!("thread/{channel_id}/{root_seq:016x}/"));
            if let Some(after) = after_reply_seq {
                scan = scan.after(thread_key(&channel_id, root_seq, after));
            }
            let (entries, has_more) = paged(store, scan, page(limit));
            let seqs: Vec<u64> = entries
                .iter()
                .filter_map(|e| u64::from_str_radix(&key_tail(e), 16).ok())
                .collect();
            let mut replies = rows_at(store, seqs.iter().map(|s| msg_key(&channel_id, *s)))?;
            hydrate(store, &mut replies, &viewer_handles);
            if let Some(root) = root.as_mut() {
                hydrate(store, std::slice::from_mut(root), &viewer_handles);
            }
            ChatViewReply::Thread {
                root,
                next_reply_seq: has_more.then(|| seqs.last().copied()).flatten(),
                replies,
                has_more,
            }
        }
        ChatViewQuery::Members {
            channel_id,
            after,
            limit,
        } => {
            let mut scan = Scan::prefix(member_key(&channel_id, ""));
            if let Some(after) = after {
                scan = scan.after(member_key(&channel_id, &after));
            }
            let (entries, has_more) = paged(store, scan, page(limit));
            let members: Vec<MemberRow> = entries
                .iter()
                .filter_map(|e| serde_json::from_slice(&e.value).ok())
                .collect();
            ChatViewReply::Members {
                next_after: has_more
                    .then(|| members.last().map(|m| m.party.clone()))
                    .flatten(),
                members,
                has_more,
            }
        }
        ChatViewQuery::Search {
            text,
            viewer_handles,
            channel_id,
            limit,
        } => {
            let wanted = tokens(&text);
            let Some(first) = wanted.iter().next() else {
                return Err(refuse(reason::INVALID_INPUT, "nothing to search for"));
            };
            // ponytail: one posting list scanned, the rest filtered on the row;
            // intersect postings if search volume ever matters.
            let prefix = match &channel_id {
                Some(ch) => tok_key(first, ch, 0).replace("0000000000000000", ""),
                None => format!("tok/{first}/"),
            };
            let entries = store.scan(Scan::prefix(prefix).limit(SEARCH_POSTING_CAP as u64 + 1));
            let capped = entries.len() > SEARCH_POSTING_CAP;
            let mut hits: Vec<MsgRow> = posted(store, &entries)?
                .into_iter()
                .filter(|row| wanted.is_subset(&tokens(&row.text)))
                .collect();
            hits.sort_by(|a, b| b.time.cmp(&a.time).then(b.seq.cmp(&a.seq)));
            let limit = page(limit);
            let capped = capped || hits.len() > limit;
            hits.truncate(limit);
            hydrate(store, &mut hits, &viewer_handles);
            ChatViewReply::Hits(MessageHits { hits, capped })
        }
        ChatViewQuery::TagSearch {
            tag,
            viewer_handles,
            channel_id,
            after,
            limit,
        } => {
            let label = tag
                .trim_start_matches('#')
                .nfc()
                .collect::<String>()
                .to_lowercase();
            let prefix = match &channel_id {
                Some(ch) => format!("tagc/{ch}/{label}/"),
                None => format!("tag/{label}/"),
            };
            let mut scan = Scan::prefix(&prefix);
            if let Some(after) = after {
                scan = scan.after(after);
            }
            let (entries, has_more) = paged(store, scan, page(limit));
            let mut hits = posted(store, &entries)?;
            hydrate(store, &mut hits, &viewer_handles);
            ChatViewReply::TagHits(TagPage {
                hits,
                has_more,
                next_after: has_more
                    .then(|| {
                        entries
                            .last()
                            .map(|e| String::from_utf8_lossy(&e.key).into_owned())
                    })
                    .flatten(),
            })
        }
    })
}
