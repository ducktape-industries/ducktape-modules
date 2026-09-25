//! [`query`]: one function per [`Query`], each a read of the tables in
//! `state.rs`. Chat's listings only grow, so a page cursor from any height
//! resumes where it left off.
use abi::{Refusal, Scan};
use store::{Page, PageReply, Reads, capacity, invalid};

use crate::state::{
    ANSWERED, CHANNEL_TAGS, CHANNELS, HEADS, MEMBERS, MESSAGE_IDS, MESSAGES, REACTIONS, REPLIES,
    ROOTS, TAGS, WORDS, message, messages, newest_first,
};
use crate::text::tag_label;
use crate::{
    ChannelInfo, ChannelRow, MAX_VIEWERS, MessageHits, MsgRow, Principal, Query, Reply,
    SEARCH_POSTING_CAP, tokens,
};

pub fn query(store: &impl Reads, height: u64, query: Query) -> Result<Reply, Refusal> {
    let (mut reply, viewer) = match query {
        Query::Channels { page } => (Reply::Channels(channels(store, &page, height)?), vec![]),
        Query::Channel { channel_id } => (Reply::Channel(channel(store, &channel_id)?), vec![]),
        Query::MessageById { message_id } => (Reply::Message(by_id(store, &message_id)?), vec![]),
        Query::ThreadAttention { channel_id, author } => (
            Reply::Attention(attention(store, &channel_id, author)?),
            vec![],
        ),
        Query::Roots {
            channel_id,
            viewer,
            page,
        } => (
            Reply::Roots(roots(store, channel_id, &page, height)?),
            viewer,
        ),
        Query::MessagesAround {
            channel_id,
            seq,
            viewer,
            page,
        } => (
            Reply::Messages(around(store, channel_id, seq, &page)?),
            viewer,
        ),
        Query::Thread {
            channel_id,
            root_seq,
            viewer,
            page,
        } => (thread(store, channel_id, root_seq, &page, height)?, viewer),
        Query::Members { channel_id, page } => {
            let members = MEMBERS.range_of(store, &channel_id, &page, height)?;
            (Reply::Members(members.map(|(_, member)| member)), vec![])
        }
        Query::Search {
            text,
            viewer,
            channel_id,
            page,
        } => (
            Reply::Hits(search(store, &text, channel_id, &page)?),
            viewer,
        ),
        Query::TagSearch {
            tag,
            viewer,
            channel_id,
            page,
        } => (
            Reply::TagHits(tagged(store, &tag, channel_id, &page, height)?),
            viewer,
        ),
        Query::Accounts { page } => (
            Reply::Accounts(crate::origin::accounts(store, page)?),
            vec![],
        ),
    };
    if viewer.len() > MAX_VIEWERS {
        return Err(capacity(format!(
            "a read names at most {MAX_VIEWERS} viewers, not {}",
            viewer.len()
        )));
    }
    for row in rows_in(&mut reply) {
        mark_reacted(store, &viewer, row);
    }
    Ok(reply)
}

/// The `Roots` cursor that resumes below `seq`: `Page::after` for the page
/// of roots older than the one on screen.
pub fn roots_below(channel_id: &str, seq: u64) -> Vec<u8> {
    let channel_id = channel_id.to_owned();
    abi::encode(&store::Cursor {
        height: 0,
        scope: ROOTS.key(&channel_id),
        after: ROOTS.key(&(channel_id.clone(), newest_first(seq))),
    })
}

fn info(store: &impl Reads, channel: ChannelRow) -> Result<ChannelInfo, Refusal> {
    let head_seq = HEADS.get(store, &channel.id)?.unwrap_or(0);
    Ok(ChannelInfo { channel, head_seq })
}

fn channels(
    store: &impl Reads,
    page: &Page,
    height: u64,
) -> Result<PageReply<ChannelInfo>, Refusal> {
    CHANNELS
        .range(store, page, height)?
        .try_map(|(_, channel)| info(store, channel))
}

fn channel(store: &impl Reads, id: &String) -> Result<Option<ChannelInfo>, Refusal> {
    CHANNELS
        .get(store, id)?
        .map(|channel| info(store, channel))
        .transpose()
}

fn by_id(store: &impl Reads, message_id: &String) -> Result<Option<MsgRow>, Refusal> {
    MESSAGE_IDS
        .get(store, message_id)?
        .map(|(channel_id, seq)| message(store, &channel_id, seq))
        .transpose()
}

/// The root of the author's most recently answered thread.
fn attention(
    store: &impl Reads,
    channel_id: &str,
    author: Principal,
) -> Result<Option<MsgRow>, Refusal> {
    let channel_id = channel_id.to_owned();
    let newest = ANSWERED.prefix_of(&(channel_id.clone(), author)).limit(1);
    ANSWERED
        .scan(store, newest)?
        .first()
        .map(|(_, root)| message(store, &channel_id, *root))
        .transpose()
}

fn roots(
    store: &impl Reads,
    channel_id: String,
    page: &Page,
    height: u64,
) -> Result<PageReply<MsgRow>, Refusal> {
    let keys = ROOTS.range_of(store, &channel_id, page, height)?;
    rows_at(
        store,
        keys.map(|(channel_id, newest)| (channel_id, newest_first(newest))),
    )
}

fn thread(
    store: &impl Reads,
    channel_id: String,
    root: u64,
    page: &Page,
    height: u64,
) -> Result<Reply, Refusal> {
    let keys = REPLIES.range_of(store, &(channel_id.clone(), root), page, height)?;
    Ok(Reply::Thread {
        root: MESSAGES.get(store, &(channel_id, root))?,
        replies: rows_at(
            store,
            keys.map(|(channel_id, _, reply)| (channel_id, reply)),
        )?,
    })
}

/// Up to `page.limit` messages, half before `seq` and half from it.
fn around(
    store: &impl Reads,
    channel_id: String,
    seq: u64,
    page: &Page,
) -> Result<Vec<MsgRow>, Refusal> {
    let half = page.limit() / 2;
    let lo = MESSAGES.key(&(channel_id.clone(), seq.saturating_sub(half)));
    let hi = MESSAGES.key(&(channel_id, seq.saturating_add(half + 1)));
    let rows = MESSAGES.scan(store, Scan::range(lo, Some(hi)))?;
    Ok(rows.into_iter().map(|(_, row)| row).collect())
}

/// The messages holding every word of `text`, newest first. The first
/// word's postings are read (at most [`SEARCH_POSTING_CAP`]) and the rest
/// checked on each row.
fn search(
    store: &impl Reads,
    text: &str,
    channel_id: Option<String>,
    page: &Page,
) -> Result<MessageHits, Refusal> {
    let wanted = tokens(text);
    let Some(first) = wanted.first().cloned() else {
        return Err(invalid("nothing to search for"));
    };
    // ponytail: one posting list scanned, the rest filtered on the row;
    // intersect postings if search volume ever matters.
    let postings = match channel_id {
        Some(channel_id) => WORDS.prefix_of(&(first, channel_id)),
        None => WORDS.prefix_of(&first),
    };
    let postings = WORDS.scan(store, postings.limit(SEARCH_POSTING_CAP as u64 + 1))?;
    let capped = postings.len() > SEARCH_POSTING_CAP;
    let at = postings
        .into_iter()
        .map(|(_, channel_id, seq)| (channel_id, seq));
    let mut hits: Vec<MsgRow> = messages(store, at)?
        .into_iter()
        .filter(|row| wanted.is_subset(&tokens(&row.text)))
        .collect();
    hits.sort_by(|a, b| b.time.cmp(&a.time).then(b.seq.cmp(&a.seq)));
    let limit = page.limit() as usize;
    let capped = capped || hits.len() > limit;
    hits.truncate(limit);
    Ok(MessageHits { hits, capped })
}

/// One page of the messages tagged `tag`, newest first.
fn tagged(
    store: &impl Reads,
    tag: &str,
    channel_id: Option<String>,
    page: &Page,
    height: u64,
) -> Result<PageReply<MsgRow>, Refusal> {
    let label = tag_label(tag);
    let keys = match channel_id {
        Some(channel_id) => CHANNEL_TAGS
            .range_of(store, &(channel_id, label), page, height)?
            .map(|(channel_id, _, newest)| (channel_id, newest_first(newest))),
        None => TAGS
            .range_of(store, &label, page, height)?
            .map(|(_, _, channel_id, seq)| (channel_id, seq)),
    };
    rows_at(store, keys)
}

/// A page of message addresses as the page of rows they name.
fn rows_at(
    store: &impl Reads,
    keys: PageReply<(String, u64)>,
) -> Result<PageReply<MsgRow>, Refusal> {
    Ok(PageReply {
        height: keys.height,
        items: messages(store, keys.items)?,
        next: keys.next,
    })
}

/// Every message row a reply carries.
fn rows_in(reply: &mut Reply) -> Vec<&mut MsgRow> {
    match reply {
        Reply::Roots(page) | Reply::TagHits(page) => page.items.iter_mut().collect(),
        Reply::Messages(rows) | Reply::Hits(MessageHits { hits: rows, .. }) => {
            rows.iter_mut().collect()
        }
        Reply::Thread { root, replies } => root.iter_mut().chain(&mut replies.items).collect(),
        Reply::Message(row) | Reply::Attention(row) => row.iter_mut().collect(),
        Reply::Channels(_) | Reply::Channel(_) | Reply::Members(_) | Reply::Accounts(_) => vec![],
    }
}

/// Each reaction on `row` learns whether one of `viewer` chose it.
fn mark_reacted(store: &impl Reads, viewer: &[Principal], row: &mut MsgRow) {
    for reaction in &mut row.reactions {
        reaction.reacted_by_me = viewer.iter().any(|principal| {
            let key = (
                row.channel_id.clone(),
                row.seq,
                reaction.emoji.clone(),
                principal.clone(),
            );
            REACTIONS.has(store, &key)
        });
    }
}
