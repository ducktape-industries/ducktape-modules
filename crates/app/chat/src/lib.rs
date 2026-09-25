//! The `chat` program: channels, messages, threads, reactions, memberships,
//! huddles. The reference program on the kernel abi.
//!
//! Writes are a [`ChatMsg`] (borsh), reads a
//! [`ChatViewQuery`] answered by a [`ChatViewReply`] (borsh) — the same
//! types `chat-view` links. The acting [`Party`] is the frame's origin: an
//! external key resolved through the `identity` program to its account.
//! The rules run over any [`store::Reads`]/[`store::Writes`] store; the
//! `program` feature adds the wasm32 program over the host (`program.rs`),
//! which a view never enables.
//!
//! Keys: `chan/<id>` → [`ChannelRow`], `seq/<id>` → head seq,
//! `msg/<ch>/<seq>` → [`MsgRow`], `root/<ch>/<!seq>` timeline roots newest
//! first, `thread/<ch>/<root>/<reply>`, `msgid/<id>`, `member/<ch>/<handle>`
//! → [`MemberRow`], `react/<ch>/<seq>/<emoji>/<handle>`, `tok/<token>/<ch>/<seq>`
//! and `tag/<label>/<!time>/<ch>/<seq>` + `tagc/<ch>/<label>/<!seq>` postings.
//! `attention/<ch>/<author>/<!last_reply>` stores a Borsh root sequence; one
//! entry per answered thread lets callers find the latest reply without scanning messages.
pub mod message;
#[cfg(feature = "view")]
pub mod view;

/// The name this program runs under.
pub const PROGRAM: &str = "chat";

use std::collections::BTreeSet;

use abi::{Entry, Refusal, Scan, reason};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub use store::{Cursor, Page, PageReply};
use store::{
    Reads, Writes, already_exists, capacity, invalid, not_found, unauthorized, wrong_state,
};
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
/// The `Roots` cursor that resumes below `seq`: `Page::after` for the page
/// of roots older than the one on screen. Chat's listings are append-only,
/// so a cursor's height is not checked.
pub fn roots_below(channel_id: &str, seq: u64) -> Vec<u8> {
    let scope = roots_prefix(channel_id).into_bytes();
    abi::encode(&store::Cursor {
        height: 0,
        scope,
        after: root_key(channel_id, seq).into_bytes(),
    })
}
fn roots_prefix(ch: &str) -> String {
    format!("root/{ch}/")
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

// ── store helpers ───────────────────────────────────────────────────────────

fn load<T: DeserializeOwned>(store: &impl Reads, key: &str) -> Result<Option<T>, Refusal> {
    store
        .get(key.as_bytes())
        .map(|b| {
            serde_json::from_slice(&b).map_err(|e| Refusal::new(reason::CORRUPT, e.to_string()))
        })
        .transpose()
}

fn save<T: Serialize>(store: &mut impl Writes, key: String, value: &T) {
    store.set(
        key.into_bytes(),
        serde_json::to_vec(value).expect("a chat row serializes"),
    );
}

fn mark(store: &mut impl Writes, key: String) {
    store.set(key.into_bytes(), Vec::new());
}

fn channel(store: &impl Reads, id: &str) -> Result<ChannelRow, Refusal> {
    load(store, &chan_key(id))?.ok_or_else(|| not_found(format!("no channel {id}")))
}

fn head_seq(store: &impl Reads, id: &str) -> u64 {
    load(store, &seq_key(id)).ok().flatten().unwrap_or(0)
}

fn row(store: &impl Reads, ch: &str, seq: u64) -> Result<MsgRow, Refusal> {
    load(store, &msg_key(ch, seq))?.ok_or_else(|| not_found(format!("no message {ch}/{seq}")))
}

fn checked_id(what: &str, id: &str) -> Result<(), Refusal> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.contains('/') {
        return Err(invalid(format!(
            "{what} is 1..={MAX_ID_BYTES} bytes without '/'"
        )));
    }
    Ok(())
}

fn checked_name(name: &str) -> Result<(), Refusal> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(invalid(format!("a name is 1..={MAX_NAME_BYTES} bytes")));
    }
    Ok(())
}

fn writable(store: &impl Reads, ch: &ChannelRow, party: &Party) -> Result<(), Refusal> {
    if ch.archived {
        return Err(wrong_state(format!("{} is archived", ch.id)));
    }
    let handle = party_handle(party);
    let allowed = ch.post_policy == PostPolicy::Open
        || ch.owner == handle
        || store.get(member_key(&ch.id, &handle).as_bytes()).is_some();
    if !allowed {
        return Err(unauthorized(format!(
            "{handle} is not a member of {}",
            ch.id
        )));
    }
    Ok(())
}

fn owned(ch: &ChannelRow, party: &Party) -> Result<(), Refusal> {
    if ch.owner != party_handle(party) {
        return Err(unauthorized(format!("only the owner of {} may", ch.id)));
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

fn index(store: &mut impl Writes, row: &MsgRow, on: bool) {
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

fn put_row(store: &mut impl Writes, row: &MsgRow) -> Result<(), Refusal> {
    let bytes = serde_json::to_vec(row).expect("a chat row serializes");
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(capacity(format!(
            "a message is at most {MAX_MESSAGE_BYTES} bytes"
        )));
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
pub use queries::query;
pub use wire::*;
#[cfg(test)]
mod tests;

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &ChatMsg) -> describe::Description {
    use describe::{Value, field};
    let party = |party: &Party| match party {
        Party::Account(number) => Value::Account(*number),
        Party::Key(key) => Value::Key(key.clone()),
        Party::Module(module) => Value::Program(module.clone()),
        Party::System => Value::text("system"),
    };
    let seq = |seq: &u64| field("seq", Value::text(seq.to_string()));
    let (title, channel, fields) = match op {
        ChatMsg::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => (
            "Create channel",
            channel_id,
            vec![
                field("name", Value::text(name)),
                field(
                    "posting",
                    Value::text(match post_policy {
                        PostPolicy::Open => "open",
                        PostPolicy::MembersOnly => "members only",
                    }),
                ),
            ],
        ),
        ChatMsg::CreateVoiceChannel { channel_id, name } => (
            "Create voice channel",
            channel_id,
            vec![field("name", Value::text(name))],
        ),
        ChatMsg::CreateDmChannel { counterpart, name } => {
            return describe::Description {
                title: "Open a DM".into(),
                fields: vec![
                    field("with", Value::Account(*counterpart)),
                    field("name", Value::text(name)),
                ],
            };
        }
        ChatMsg::RenameChannel { channel_id, name } => (
            "Rename channel",
            channel_id,
            vec![field("name", Value::text(name))],
        ),
        ChatMsg::SetChannelArchived {
            channel_id,
            archived,
        } => (
            if *archived {
                "Archive channel"
            } else {
                "Unarchive channel"
            },
            channel_id,
            vec![],
        ),
        ChatMsg::PostMessage {
            channel_id,
            message_id,
            blocks,
            thread,
        } => {
            let mut fields = place(channel_id);
            fields.extend([
                field("text", Value::Text(plain_text(blocks))),
                field("message", Value::text(message_id)),
                field(
                    "thread",
                    Value::Text(thread.map_or_else(|| "—".into(), |t| t.to_string())),
                ),
            ]);
            return describe::Description {
                title: match dm_peers(channel_id) {
                    Some(_) => "Direct message".into(),
                    None => format!("Post in #{channel_id}"),
                },
                fields,
            };
        }
        ChatMsg::EditMessage {
            channel_id,
            seq: at,
            blocks,
            ..
        } => (
            "Edit message",
            channel_id,
            vec![seq(at), field("text", Value::Text(plain_text(blocks)))],
        ),
        ChatMsg::DeleteMessage {
            channel_id,
            seq: at,
        } => ("Delete message", channel_id, vec![seq(at)]),
        ChatMsg::AddReaction {
            channel_id,
            seq: at,
            emoji,
        } => (
            "React",
            channel_id,
            vec![seq(at), field("emoji", Value::text(emoji))],
        ),
        ChatMsg::RemoveReaction {
            channel_id,
            seq: at,
            emoji,
        } => (
            "Remove reaction",
            channel_id,
            vec![seq(at), field("emoji", Value::text(emoji))],
        ),
        ChatMsg::SetMembership {
            channel_id,
            party: who,
            member,
        } => (
            if *member {
                "Add member"
            } else {
                "Remove member"
            },
            channel_id,
            vec![field("party", party(who))],
        ),
        ChatMsg::JoinHuddle {
            channel_id, node, ..
        } => (
            "Join huddle",
            channel_id,
            vec![field("node", Value::Key(node.clone()))],
        ),
        ChatMsg::LeaveHuddle { channel_id } => ("Leave huddle", channel_id, vec![]),
    };
    let mut all = place(channel);
    all.extend(fields);
    describe::Description {
        title: format!("{title} · {}", room(channel)),
        fields: all,
    }
}

describe::export!(ChatMsg, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<ChatMsg>(),
        [
            "CreateChannel",
            "CreateVoiceChannel",
            "CreateDmChannel",
            "RenameChannel",
            "SetChannelArchived",
            "PostMessage",
            "EditMessage",
            "DeleteMessage",
            "AddReaction",
            "RemoveReaction",
            "SetMembership",
            "JoinHuddle",
            "LeaveHuddle",
        ]
    );
}

/// A channel as `describe` titles it: `#design`, or `DM` for a dm room,
/// whose id is no name a person picked. Its two accounts are the
/// `between` field, drawn as accounts (a name, an avatar), never numbers.
fn room(channel_id: &str) -> String {
    match dm_peers(channel_id) {
        Some(_) => "DM".into(),
        None => format!("#{channel_id}"),
    }
}

/// The fields that say where an op happened: its `channel`, and for a dm
/// room the two accounts it is `between`.
fn place(channel_id: &str) -> Vec<describe::Field> {
    use describe::{Value, field};
    let mut fields = vec![field("channel", Value::Text(room(channel_id)))];
    fields.extend(dm_peers(channel_id).map(|(a, b)| {
        field(
            "between",
            Value::List(vec![Value::Account(a), Value::Account(b)]),
        )
    }));
    fields
}
