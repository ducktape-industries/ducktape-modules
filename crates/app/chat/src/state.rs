//! Everything chat keeps, declared once: each table's key and value types,
//! and the writes that keep a message and its indexes in step. Keys are
//! typed (`store::KeyCodec`: integers big-endian, strings and principals
//! NUL-terminated, so names list by name), values borsh. A tuple key scans by its leading
//! elements, which is how every "in this channel" read works.
use abi::Refusal;
use store::{Map, Reads, Set, Writes, capacity, not_found};

use crate::{ChannelRow, MAX_MESSAGE_BYTES, MemberRow, MsgRow, Principal, tokens};

type ChannelId = String;
type Seq = u64;

pub(crate) const CHANNELS: Map<ChannelId, ChannelRow> = Map::new("channel/");
/// The newest message's seq, per channel.
pub(crate) const HEADS: Map<ChannelId, Seq> = Map::new("head/");
pub(crate) const MESSAGES: Map<(ChannelId, Seq), MsgRow> = Map::new("message/");
/// Where each message id lives: ids are unique across channels.
pub(crate) const MESSAGE_IDS: Map<String, (ChannelId, Seq)> = Map::new("message-id/");
pub(crate) const MEMBERS: Map<(ChannelId, Principal), MemberRow> = Map::new("member/");
/// Who chose which emoji: `(channel, seq, emoji, principal)`.
pub(crate) const REACTIONS: Set<(ChannelId, Seq, String, Principal)> = Set::new("reaction/");

/// Timeline roots, newest first: `(channel, newest_first(seq))`.
pub(crate) const ROOTS: Set<(ChannelId, Seq)> = Set::new("root/");
/// Thread replies in post order: `(channel, root, reply)`.
pub(crate) const REPLIES: Set<(ChannelId, Seq, Seq)> = Set::new("reply/");
/// Each author's answered threads, newest answer first:
/// `(channel, root author, newest_first(last reply))` → the root's seq.
pub(crate) const ANSWERED: Map<(ChannelId, Principal, Seq), Seq> = Map::new("answered/");
/// Search postings: `(word, channel, seq)`.
pub(crate) const WORDS: Set<(String, ChannelId, Seq)> = Set::new("word/");
/// Tag postings, newest first: `(tag, newest_first(time), channel, seq)`.
pub(crate) const TAGS: Set<(String, u64, ChannelId, Seq)> = Set::new("tag/");
/// Tag postings within a channel, newest first:
/// `(channel, tag, newest_first(seq))`.
pub(crate) const CHANNEL_TAGS: Set<(ChannelId, String, Seq)> = Set::new("channel-tag/");

/// A key part that scans newest first. Its own inverse.
pub(crate) fn newest_first(n: u64) -> u64 {
    u64::MAX - n
}

pub(crate) fn channel(store: &impl Reads, id: &str) -> Result<ChannelRow, Refusal> {
    CHANNELS
        .get(store, &id.to_owned())?
        .ok_or_else(|| not_found(format!("no channel {id}")))
}

pub(crate) fn message(store: &impl Reads, channel_id: &str, seq: Seq) -> Result<MsgRow, Refusal> {
    MESSAGES
        .get(store, &(channel_id.to_owned(), seq))?
        .ok_or_else(|| not_found(format!("no message {channel_id}/{seq}")))
}

/// The rows at these addresses; one gone (never, in chat) is skipped.
pub(crate) fn messages(
    store: &impl Reads,
    at: impl IntoIterator<Item = (ChannelId, Seq)>,
) -> Result<Vec<MsgRow>, Refusal> {
    at.into_iter()
        .filter_map(|key| MESSAGES.get(store, &key).transpose())
        .collect()
}

/// Refused when the row would outgrow [`MAX_MESSAGE_BYTES`]: checked before
/// an op writes anything.
pub(crate) fn fits(row: &MsgRow) -> Result<(), Refusal> {
    if abi::encode(row).len() > MAX_MESSAGE_BYTES {
        return Err(capacity(format!(
            "a message is at most {MAX_MESSAGE_BYTES} bytes"
        )));
    }
    Ok(())
}

/// Stores `row` in place of `old` (none for a new message), moving its
/// search and tag postings with it. The caller has checked [`fits`].
pub(crate) fn replace_message(store: &mut impl Writes, old: Option<&MsgRow>, row: &MsgRow) {
    if let Some(old) = old {
        postings(store, old, false);
    }
    postings(store, row, true);
    MESSAGES.put(store, &(row.channel_id.clone(), row.seq), row);
}

/// Every search and tag posting a row makes, on or off.
fn postings(store: &mut impl Writes, row: &MsgRow, on: bool) {
    let (channel, seq) = (row.channel_id.clone(), row.seq);
    for word in tokens(&row.text) {
        toggle(store, &WORDS, &(word, channel.clone(), seq), on);
    }
    for tag in &row.tags {
        let when = newest_first(row.time);
        toggle(store, &TAGS, &(tag.clone(), when, channel.clone(), seq), on);
        let newest = newest_first(seq);
        toggle(
            store,
            &CHANNEL_TAGS,
            &(channel.clone(), tag.clone(), newest),
            on,
        );
    }
}

pub(crate) fn toggle<K: store::KeyCodec>(store: &mut impl Writes, set: &Set<K>, key: &K, on: bool) {
    if on {
        set.insert(store, key);
    } else {
        set.remove(store, key);
    }
}
