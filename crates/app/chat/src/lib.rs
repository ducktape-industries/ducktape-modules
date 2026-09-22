//! The `chat` program: channels, messages, threads, reactions, memberships,
//! huddles. The reference program on the kernel abi.
//!
//! Writes are a [`ChatMsg`] (JSON, externally tagged), reads a
//! [`ChatViewQuery`] answered by a [`ChatViewReply`] (JSON) — the same
//! types `chat-view` links. The acting [`Party`] is the frame's origin: an
//! external key resolved through the `identity` program to its account.
//! The rules run over any [`Read`]/[`Write`] store; the `program` feature
//! adds the wasm32 program over the host (`program.rs`), which a view never
//! enables.
//!
//! Keys: `chan/<id>` → [`ChannelRow`], `seq/<id>` → head seq,
//! `msg/<ch>/<seq>` → [`MsgRow`], `root/<ch>/<!seq>` timeline roots newest
//! first, `thread/<ch>/<root>/<reply>`, `msgid/<id>`, `member/<ch>/<handle>`
//! → [`MemberRow`], `react/<ch>/<seq>/<emoji>/<handle>`, `tok/<token>/<ch>/<seq>`
//! and `tag/<label>/<!time>/<ch>/<seq>` + `tagc/<ch>/<label>/<!seq>` postings.
//! `attention/<ch>/<author>/<!last_reply>` stores a Borsh root sequence; one
//! entry per answered thread lets callers find the latest reply without scanning messages.
pub mod message;

use std::collections::BTreeSet;

use abi::{Entry, Refusal, Scan, reason};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use unicode_normalization::UnicodeNormalization;

pub use message::{AccountNumber, Block, Mark, Party, Span, parse_message};

pub const MAX_ID_BYTES: usize = 64;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_REVISIONS: u32 = 256;
pub const MAX_EMOJI_BYTES: usize = 64;
pub const MAX_REACTION_EMOJIS: usize = 64;
pub const MAX_THREAD_REPLIES: u64 = 4096;
pub const MAX_HUDDLE_MEMBERS: usize = 32;
pub const HUDDLE_NODE_KEY_BYTES: usize = 32;
/// The namespace a node key signs under to join a huddle; the message is
/// the channel id then the joining origin key.
pub const HUDDLE_JOIN_NS: &[u8] = b"ducktape/huddle-join/v1";
pub const MAX_TAGS_PER_MESSAGE: usize = 16;
pub const MAX_TAG_CHARS: usize = 64;
pub const MAX_PAGE: usize = 256;
pub const DEFAULT_PAGE: usize = 50;
/// How many postings a search reads before it reports `capped`.
pub const SEARCH_POSTING_CAP: usize = 1024;

// ── keys ────────────────────────────────────────────────────────────────────

fn chan_key(id: &str) -> String {
    format!("chan/{id}")
}
fn seq_key(id: &str) -> String {
    format!("seq/{id}")
}
fn msg_key(ch: &str, seq: u64) -> String {
    format!("msg/{ch}/{seq:016x}")
}
fn root_key(ch: &str, seq: u64) -> String {
    format!("root/{ch}/{:016x}", u64::MAX - seq)
}
fn msgid_key(id: &str) -> String {
    format!("msgid/{id}")
}
fn thread_key(ch: &str, root: u64, reply: u64) -> String {
    format!("thread/{ch}/{root:016x}/{reply:016x}")
}
fn attention_prefix(ch: &str, author: &str) -> String {
    format!("attention/{ch}/{author}/")
}
fn attention_key(ch: &str, author: &str, reply: u64) -> String {
    format!(
        "{}{last:016x}",
        attention_prefix(ch, author),
        last = u64::MAX - reply
    )
}
fn member_key(ch: &str, handle: &str) -> String {
    format!("member/{ch}/{handle}")
}
fn react_key(ch: &str, seq: u64, emoji: &str, handle: &str) -> String {
    format!("react/{ch}/{seq:016x}/{emoji}/{handle}")
}
fn tok_key(token: &str, ch: &str, seq: u64) -> String {
    format!("tok/{token}/{ch}/{seq:016x}")
}
fn tag_key(label: &str, time: u64, ch: &str, seq: u64) -> String {
    format!("tag/{label}/{:016x}/{ch}/{seq:016x}", u64::MAX - time)
}
fn tagc_key(ch: &str, label: &str, seq: u64) -> String {
    format!("tagc/{ch}/{label}/{:016x}", u64::MAX - seq)
}

// ── the store ───────────────────────────────────────────────────────────────

pub trait Read {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn scan(&self, scan: Scan) -> Vec<Entry>;
}

pub trait Write: Read {
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>);
    fn delete(&mut self, key: &[u8]);
}

// ── store helpers ───────────────────────────────────────────────────────────

fn refuse(reason: &str, sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason, sentence)
}

fn load<T: DeserializeOwned>(store: &impl Read, key: &str) -> Result<Option<T>, Refusal> {
    store
        .get(key.as_bytes())
        .map(|b| serde_json::from_slice(&b).map_err(|e| refuse(reason::CORRUPT, e.to_string())))
        .transpose()
}

fn save<T: Serialize>(store: &mut impl Write, key: String, value: &T) {
    store.set(
        key.into_bytes(),
        serde_json::to_vec(value).expect("a chat row serializes"),
    );
}

fn mark(store: &mut impl Write, key: String) {
    store.set(key.into_bytes(), Vec::new());
}

fn channel(store: &impl Read, id: &str) -> Result<ChannelRow, Refusal> {
    load(store, &chan_key(id))?.ok_or_else(|| refuse(reason::NOT_FOUND, format!("no channel {id}")))
}

fn head_seq(store: &impl Read, id: &str) -> u64 {
    load(store, &seq_key(id)).ok().flatten().unwrap_or(0)
}

fn row(store: &impl Read, ch: &str, seq: u64) -> Result<MsgRow, Refusal> {
    load(store, &msg_key(ch, seq))?
        .ok_or_else(|| refuse(reason::NOT_FOUND, format!("no message {ch}/{seq}")))
}

fn checked_id(what: &str, id: &str) -> Result<(), Refusal> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.contains('/') {
        return Err(refuse(
            reason::INVALID_INPUT,
            format!("{what} is 1..={MAX_ID_BYTES} bytes without '/'"),
        ));
    }
    Ok(())
}

fn checked_name(name: &str) -> Result<(), Refusal> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(refuse(
            reason::INVALID_INPUT,
            format!("a name is 1..={MAX_NAME_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn writable(store: &impl Read, ch: &ChannelRow, party: &Party) -> Result<(), Refusal> {
    if ch.archived {
        return Err(refuse(
            reason::WRONG_STATE,
            format!("{} is archived", ch.id),
        ));
    }
    let handle = party_handle(party);
    let allowed = ch.post_policy == PostPolicy::Open
        || ch.owner == handle
        || store.get(member_key(&ch.id, &handle).as_bytes()).is_some();
    if !allowed {
        return Err(refuse(
            reason::UNAUTHORIZED,
            format!("{handle} is not a member of {}", ch.id),
        ));
    }
    Ok(())
}

fn owned(ch: &ChannelRow, party: &Party) -> Result<(), Refusal> {
    if ch.owner != party_handle(party) {
        return Err(refuse(
            reason::UNAUTHORIZED,
            format!("only the owner of {} may", ch.id),
        ));
    }
    Ok(())
}

// ── text: flattening, search tokens, tags ───────────────────────────────────

pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        let piece = match block {
            Block::Paragraph(spans) | Block::Quote(spans) => spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            Block::Code { text, .. } => text.clone(),
            Block::Divider => continue,
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&piece);
    }
    out
}

/// NFC lowercase alphanumeric runs of two or more chars.
pub fn tokens(text: &str) -> BTreeSet<String> {
    text.nfc()
        .collect::<String>()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

/// `#tag` labels in appearance order: NFC lowercase, `[alnum_-]`, opened at
/// a word boundary (never `##`, `/#`, `&#`), outside code and links.
pub fn tags(blocks: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    for block in blocks {
        let spans = match block {
            Block::Paragraph(spans) | Block::Quote(spans) => spans,
            Block::Code { .. } | Block::Divider => continue,
        };
        for span in spans {
            if span.marks.iter().any(|m| matches!(m, Mark::Link(_))) {
                continue;
            }
            let mut prev: Option<char> = None;
            let mut rest = span.text.as_str();
            while let Some(at) = rest.find('#') {
                let before = if at == 0 {
                    prev
                } else {
                    rest[..at].chars().next_back()
                };
                let opens =
                    before.is_none_or(|p| !p.is_alphanumeric() && !matches!(p, '#' | '/' | '&'));
                let body: String = rest[at + 1..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if opens && (1..=MAX_TAG_CHARS).contains(&body.chars().count()) {
                    let label = body.nfc().collect::<String>().to_lowercase();
                    if !out.contains(&label) {
                        out.push(label);
                    }
                }
                prev = Some(body.chars().next_back().unwrap_or('#'));
                rest = &rest[at + 1 + body.len()..];
            }
        }
    }
    out.truncate(MAX_TAGS_PER_MESSAGE);
    out
}

fn index(store: &mut impl Write, row: &MsgRow, on: bool) {
    let posting = serde_json::to_vec(&(&row.channel_id, row.seq)).expect("a posting serializes");
    let mut keys: Vec<String> = tokens(&row.text)
        .iter()
        .map(|t| tok_key(t, &row.channel_id, row.seq))
        .collect();
    for label in &row.tags {
        keys.push(tag_key(label, row.time, &row.channel_id, row.seq));
        keys.push(tagc_key(&row.channel_id, label, row.seq));
    }
    for key in keys {
        if on {
            store.set(key.into_bytes(), posting.clone());
        } else {
            store.delete(key.as_bytes());
        }
    }
}

fn put_row(store: &mut impl Write, row: &MsgRow) -> Result<(), Refusal> {
    let bytes = serde_json::to_vec(row).expect("a chat row serializes");
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(refuse(
            reason::CAPACITY,
            format!("a message is at most {MAX_MESSAGE_BYTES} bytes"),
        ));
    }
    store.set(msg_key(&row.channel_id, row.seq).into_bytes(), bytes);
    Ok(())
}

mod ops;
#[cfg(feature = "program")]
mod program;
mod queries;
mod wire;
pub use ops::execute;
pub use queries::{page, query};
pub use wire::*;
#[cfg(test)]
mod tests;
