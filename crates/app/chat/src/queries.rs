use super::*;

// ── query ───────────────────────────────────────────────────────────────────

/// One page of the entries under `prefix`, the raw key beside each value
/// so `Page::reply` can resume from it.
fn paged(store: &impl Reads, prefix: impl AsRef<[u8]>, page: &Page) -> Vec<(Vec<u8>, Vec<u8>)> {
    store
        .scan(page.scan_ahead(prefix.as_ref()))
        .into_iter()
        .map(|e| (e.key, e.value))
        .collect()
}

/// One page of the keys under `prefix`.
fn keys_page(
    store: &impl Reads,
    prefix: impl AsRef<[u8]>,
    page: &Page,
    height: u64,
) -> PageReply<Vec<u8>> {
    page.reply(
        height,
        paged(store, prefix, page)
            .into_iter()
            .map(|(key, _)| (key.clone(), key)),
    )
}

/// The rows the postings under `prefix` name, one page.
fn posted_page(
    store: &impl Reads,
    prefix: impl AsRef<[u8]>,
    page: &Page,
    height: u64,
) -> Result<PageReply<MsgRow>, Refusal> {
    page.reply(height, paged(store, prefix, page))
        .try_map(|posting| {
            let (ch, seq): (String, u64) = serde_json::from_slice(&posting)
                .map_err(|e| Refusal::new(reason::CORRUPT, e.to_string()))?;
            row(store, &ch, seq)
        })
}

fn rows_at(
    store: &impl Reads,
    keys: impl IntoIterator<Item = String>,
) -> Result<Vec<MsgRow>, Refusal> {
    keys.into_iter()
        .filter_map(|k| load(store, &k).transpose())
        .collect()
}

/// A posting's row, by the `(channel, seq)` it names.
fn posted(store: &impl Reads, entries: &[Entry]) -> Result<Vec<MsgRow>, Refusal> {
    let keys = entries
        .iter()
        .filter_map(|e| serde_json::from_slice::<(String, u64)>(&e.value).ok())
        .map(|(ch, seq)| msg_key(&ch, seq));
    rows_at(store, keys)
}

fn hydrate(store: &impl Reads, rows: &mut [MsgRow], viewer: &[String]) {
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

fn key_tail(key: &[u8]) -> &str {
    let key = std::str::from_utf8(key).unwrap_or_default();
    key.rsplit('/').next().unwrap_or_default()
}

pub fn query(store: &impl Reads, height: u64, q: ChatViewQuery) -> Result<ChatViewReply, Refusal> {
    Ok(match q {
        ChatViewQuery::Accounts { .. } => {
            return Err(Refusal::new(
                reason::UNSUPPORTED,
                "accounts are identity's, asked by the program",
            ));
        }
        ChatViewQuery::Channels { page } => ChatViewReply::Channels(
            page.reply(height, paged(store, b"chan/", &page))
                .try_map(|value| {
                    let channel: ChannelRow = serde_json::from_slice(&value)
                        .map_err(|e| Refusal::new(reason::CORRUPT, e.to_string()))?;
                    Ok(ChannelInfo {
                        head_seq: head_seq(store, &channel.id),
                        channel,
                    })
                })?,
        ),
        ChatViewQuery::ThreadAttention { channel_id, author } => {
            let entries = store
                .scan(Scan::prefix(attention_prefix(&channel_id, &party_handle(&author))).limit(1));
            let root = entries
                .first()
                .map(|e| {
                    abi::decode::<u64>(&e.value)
                        .map_err(|e| Refusal::new(reason::CORRUPT, e.to_string()))
                })
                .transpose()?;
            ChatViewReply::Attention(root.map(|seq| row(store, &channel_id, seq)).transpose()?)
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
            page,
        } => {
            let keyed = keys_page(store, format!("root/{channel_id}/"), &page, height);
            let seqs = keyed
                .items
                .iter()
                .filter_map(|key| u64::from_str_radix(key_tail(key), 16).ok())
                .map(|r| u64::MAX - r);
            let mut roots = rows_at(store, seqs.map(|s| msg_key(&channel_id, s)))?;
            hydrate(store, &mut roots, &viewer_handles);
            ChatViewReply::Roots(PageReply {
                height,
                items: roots,
                next: keyed.next,
            })
        }
        ChatViewQuery::MessagesAround {
            channel_id,
            seq,
            viewer_handles,
            page,
        } => {
            let half = page.limit() / 2;
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
            page,
        } => {
            let mut root = load::<MsgRow>(store, &msg_key(&channel_id, root_seq))?;
            let prefix = format!("thread/{channel_id}/{root_seq:016x}/");
            let keyed = keys_page(store, prefix, &page, height);
            let seqs = keyed
                .items
                .iter()
                .filter_map(|key| u64::from_str_radix(key_tail(key), 16).ok());
            let mut replies = rows_at(store, seqs.map(|s| msg_key(&channel_id, s)))?;
            hydrate(store, &mut replies, &viewer_handles);
            if let Some(root) = root.as_mut() {
                hydrate(store, std::slice::from_mut(root), &viewer_handles);
            }
            ChatViewReply::Thread {
                root,
                replies: PageReply {
                    height,
                    items: replies,
                    next: keyed.next,
                },
            }
        }
        ChatViewQuery::Members { channel_id, page } => ChatViewReply::Members(
            page.reply(height, paged(store, member_key(&channel_id, ""), &page))
                .try_map(|value| {
                    serde_json::from_slice(&value)
                        .map_err(|e| Refusal::new(reason::CORRUPT, e.to_string()))
                })?,
        ),
        ChatViewQuery::Search {
            text,
            viewer_handles,
            channel_id,
            page,
        } => {
            let wanted = tokens(&text);
            let Some(first) = wanted.iter().next() else {
                return Err(invalid("nothing to search for"));
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
            let limit = page.limit() as usize;
            let capped = capped || hits.len() > limit;
            hits.truncate(limit);
            hydrate(store, &mut hits, &viewer_handles);
            ChatViewReply::Hits(MessageHits { hits, capped })
        }
        ChatViewQuery::TagSearch {
            tag,
            viewer_handles,
            channel_id,
            page,
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
            let mut hits = posted_page(store, &prefix, &page, height)?;
            hydrate(store, &mut hits.items, &viewer_handles);
            ChatViewReply::TagHits(hits)
        }
    })
}
