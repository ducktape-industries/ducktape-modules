//! The `chat` program: channels, messages, threads, reactions, memberships,
//! huddles. The reference program on the kernel abi.
//!
//! Writes are a [`ChatMsg`] (JSON, externally tagged), reads a
//! [`ChatViewQuery`] answered by a [`ChatViewReply`] (JSON) — the same
//! types `chat-view` links. The acting [`Party`] is the frame's origin: an
//! external key resolved through the `identity` program to its account.
//!
//! Keys: `chan/<id>` → [`ChannelRow`], `seq/<id>` → head seq,
//! `msg/<ch>/<seq>` → [`MsgRow`], `root/<ch>/<!seq>` timeline roots newest
//! first, `thread/<ch>/<root>/<reply>`, `msgid/<id>`, `member/<ch>/<handle>`
//! → [`MemberRow`], `react/<ch>/<seq>/<emoji>/<handle>`, `tok/<token>/<ch>/<seq>`
//! and `tag/<label>/<!time>/<ch>/<seq>` + `tagc/<ch>/<label>/<!seq>` postings.
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

// ── wire ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PostPolicy {
    Open,
    MembersOnly,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatMsg {
    CreateChannel {
        channel_id: String,
        name: String,
        post_policy: PostPolicy,
    },
    CreateVoiceChannel {
        channel_id: String,
        name: String,
    },
    /// A members-only room between the actor's account and `counterpart`,
    /// id `dm-<lower>-<higher>`; creating it twice is a no-op.
    CreateDmChannel {
        counterpart: AccountNumber,
        name: String,
    },
    RenameChannel {
        channel_id: String,
        name: String,
    },
    SetChannelArchived {
        channel_id: String,
        archived: bool,
    },
    PostMessage {
        channel_id: String,
        message_id: String,
        blocks: Vec<Block>,
        thread: Option<u64>,
    },
    EditMessage {
        channel_id: String,
        seq: u64,
        blocks: Vec<Block>,
        base_rev: Option<u32>,
    },
    DeleteMessage {
        channel_id: String,
        seq: u64,
    },
    AddReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    RemoveReaction {
        channel_id: String,
        seq: u64,
        emoji: String,
    },
    SetMembership {
        channel_id: String,
        party: Party,
        member: bool,
    },
    /// `node_proof` is `node`'s signature over [`HUDDLE_JOIN_NS`] + channel
    /// id + the origin key (verified by the program, not the rules).
    JoinHuddle {
        channel_id: String,
        node: Vec<u8>,
        node_proof: Vec<u8>,
    },
    LeaveHuddle {
        channel_id: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MsgRow {
    pub channel_id: String,
    pub seq: u64,
    pub message_id: String,
    /// the author's handle: `acct:<n>`, `user:<hex>`, `module:<id>`, `system`
    pub author: String,
    pub height: u64,
    pub time: u64,
    pub blocks: Vec<Block>,
    /// the flattened text search indexes; a tombstone's is empty
    pub text: String,
    pub deleted: bool,
    pub edited: bool,
    pub rev: u32,
    pub edited_at: Option<u64>,
    /// what the last edit claimed to be based on, recorded, never judged
    pub base_rev: Option<u32>,
    /// `Some(root_seq)` marks a thread reply
    pub thread: Option<u64>,
    pub reply_count: u64,
    pub last_reply_seq: Option<u64>,
    pub reactions: Vec<ReactionSummary>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReactionSummary {
    pub emoji: String,
    pub count: u64,
    /// filled per reader from `viewer_handles`
    pub reacted_by_me: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub post_policy: PostPolicy,
    pub owner: String,
    pub archived: bool,
    pub huddle: Vec<HuddleEntry>,
    pub voice: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HuddleEntry {
    pub party: String,
    /// the node key, hex
    pub node: String,
    pub joined_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelInfo {
    #[serde(flatten)]
    pub channel: ChannelRow,
    pub head_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberRow {
    pub party: String,
    pub height: u64,
    pub time: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageHits {
    pub hits: Vec<MsgRow>,
    pub capped: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagPage {
    pub hits: Vec<MsgRow>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after: Option<String>,
}

/// `viewer_handles` are the reader's handles; they decide `reacted_by_me`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatViewQuery {
    Channels {
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    Channel {
        channel_id: String,
    },
    /// one page of timeline roots older than `before_seq`, oldest first
    Roots {
        channel_id: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        before_seq: Option<u64>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// `limit` messages centred on `seq`
    MessagesAround {
        channel_id: String,
        seq: u64,
        viewer_handles: Vec<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// the root plus one page of replies after `after_reply_seq`, post order
    Thread {
        channel_id: String,
        root_seq: u64,
        viewer_handles: Vec<String>,
        #[serde(default)]
        after_reply_seq: Option<u64>,
        #[serde(default)]
        limit: Option<usize>,
    },
    Members {
        channel_id: String,
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// every token of `text`, newest first
    Search {
        text: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    TagSearch {
        tag: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// the identity roster, ascending by number: the program asks identity
    /// so the view links one module
    Accounts {
        #[serde(default)]
        limit: Option<usize>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRow {
    pub number: AccountNumber,
    pub name: String,
    /// a program-controlled account: an agent, not a person
    pub program: bool,
    /// the account's keys, hex
    pub keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatViewReply {
    Channels {
        channels: Vec<ChannelInfo>,
        has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_after: Option<String>,
    },
    Channel(Option<ChannelInfo>),
    Roots {
        roots: Vec<MsgRow>,
        has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_before_seq: Option<u64>,
    },
    Messages(Vec<MsgRow>),
    Thread {
        root: Option<MsgRow>,
        replies: Vec<MsgRow>,
        has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_reply_seq: Option<u64>,
    },
    Members {
        members: Vec<MemberRow>,
        has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_after: Option<String>,
    },
    Hits(MessageHits),
    TagHits(TagPage),
    Accounts(Vec<AccountRow>),
}

/// The frame a write runs in: who acts, when.
#[derive(Clone, Debug)]
pub struct Frame {
    pub party: Party,
    pub height: u64,
    pub time: u64,
}

pub fn party_handle(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("acct:{account}"),
        Party::Key(key) => format!("user:{}", hex(key)),
        Party::Module(module) => format!("module:{module}"),
        Party::System => "system".to_string(),
    }
}

/// The room two accounts share: `dm-<lower>-<higher>`.
pub fn dm_channel_id(a: AccountNumber, b: AccountNumber) -> String {
    format!("dm-{}-{}", a.min(b), a.max(b))
}

/// The two accounts of a dm room id, or `None` for any other channel.
pub fn dm_peers(channel_id: &str) -> Option<(AccountNumber, AccountNumber)> {
    let (a, b) = channel_id.strip_prefix("dm-")?.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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

// ── execute ─────────────────────────────────────────────────────────────────

pub fn execute(store: &mut impl Write, frame: &Frame, msg: ChatMsg) -> Result<(), Refusal> {
    let actor = party_handle(&frame.party);
    match msg {
        ChatMsg::CreateChannel {
            channel_id,
            name,
            post_policy,
        } => create_channel(store, frame, channel_id, name, post_policy, false),
        ChatMsg::CreateVoiceChannel { channel_id, name } => {
            create_channel(store, frame, channel_id, name, PostPolicy::Open, true)
        }
        ChatMsg::CreateDmChannel { counterpart, name } => {
            let Party::Account(me) = frame.party else {
                return Err(refuse(reason::UNAUTHORIZED, "only an account opens a dm"));
            };
            if me == counterpart {
                return Err(refuse(reason::INVALID_INPUT, "a dm needs two accounts"));
            }
            let id = dm_channel_id(me, counterpart);
            if load::<ChannelRow>(store, &chan_key(&id))?.is_some() {
                return Ok(());
            }
            create_channel(
                store,
                frame,
                id.clone(),
                name,
                PostPolicy::MembersOnly,
                false,
            )?;
            for party in [Party::Account(me), Party::Account(counterpart)] {
                set_member(store, frame, &id, &party, true);
            }
            Ok(())
        }
        ChatMsg::RenameChannel { channel_id, name } => {
            checked_name(&name)?;
            let mut ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            ch.name = name;
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::SetChannelArchived {
            channel_id,
            archived,
        } => {
            let mut ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            ch.archived = archived;
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::PostMessage {
            channel_id,
            message_id,
            blocks,
            thread,
        } => {
            checked_id("message_id", &message_id)?;
            let ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            if store.get(msgid_key(&message_id).as_bytes()).is_some() {
                return Err(refuse(
                    reason::ALREADY_EXISTS,
                    format!("message {message_id} exists"),
                ));
            }
            let seq = head_seq(store, &channel_id) + 1;
            if let Some(root_seq) = thread {
                let mut root = row(store, &channel_id, root_seq)?;
                if root.thread.is_some() {
                    return Err(refuse(
                        reason::INVALID_INPUT,
                        "a reply cannot be a thread root",
                    ));
                }
                if root.reply_count >= MAX_THREAD_REPLIES {
                    return Err(refuse(reason::CAPACITY, "this thread is full"));
                }
                root.reply_count += 1;
                root.last_reply_seq = Some(seq);
                put_row(store, &root)?;
                mark(store, thread_key(&channel_id, root_seq, seq));
            } else {
                mark(store, root_key(&channel_id, seq));
            }
            let text = plain_text(&blocks);
            let row = MsgRow {
                channel_id: channel_id.clone(),
                seq,
                message_id: message_id.clone(),
                author: actor,
                height: frame.height,
                time: frame.time,
                tags: tags(&blocks),
                blocks,
                text,
                thread,
                ..MsgRow::default()
            };
            put_row(store, &row)?;
            index(store, &row, true);
            save(store, msgid_key(&message_id), &(&channel_id, seq));
            save(store, seq_key(&channel_id), &seq);
            Ok(())
        }
        ChatMsg::EditMessage {
            channel_id,
            seq,
            blocks,
            base_rev,
        } => {
            let ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            let mut row = row(store, &channel_id, seq)?;
            if row.author != actor {
                return Err(refuse(reason::UNAUTHORIZED, "only the author edits"));
            }
            if row.deleted {
                return Err(refuse(reason::WRONG_STATE, "the message is deleted"));
            }
            if row.rev >= MAX_REVISIONS {
                return Err(refuse(
                    reason::CAPACITY,
                    "the message has no revisions left",
                ));
            }
            index(store, &row, false);
            row.text = plain_text(&blocks);
            row.tags = tags(&blocks);
            row.blocks = blocks;
            row.rev += 1;
            row.edited = true;
            row.edited_at = Some(frame.time);
            row.base_rev = base_rev;
            put_row(store, &row)?;
            index(store, &row, true);
            Ok(())
        }
        ChatMsg::DeleteMessage { channel_id, seq } => {
            let ch = channel(store, &channel_id)?;
            let mut row = row(store, &channel_id, seq)?;
            if row.author != actor && ch.owner != actor {
                return Err(refuse(
                    reason::UNAUTHORIZED,
                    "only the author or the owner deletes",
                ));
            }
            if row.deleted {
                return Ok(());
            }
            index(store, &row, false);
            for entry in store.scan(Scan::prefix(react_key(&channel_id, seq, "", ""))) {
                store.delete(&entry.key);
            }
            row = MsgRow {
                blocks: Vec::new(),
                text: String::new(),
                tags: Vec::new(),
                reactions: Vec::new(),
                deleted: true,
                ..row
            };
            put_row(store, &row)
        }
        ChatMsg::AddReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, true),
        ChatMsg::RemoveReaction {
            channel_id,
            seq,
            emoji,
        } => react(store, frame, &channel_id, seq, &emoji, false),
        ChatMsg::SetMembership {
            channel_id,
            party,
            member,
        } => {
            let ch = channel(store, &channel_id)?;
            owned(&ch, &frame.party)?;
            set_member(store, frame, &channel_id, &party, member);
            Ok(())
        }
        ChatMsg::JoinHuddle {
            channel_id, node, ..
        } => {
            if !frame.party.is_person() {
                return Err(refuse(reason::UNAUTHORIZED, "only people join a huddle"));
            }
            if node.len() != HUDDLE_NODE_KEY_BYTES {
                return Err(refuse(
                    reason::INVALID_INPUT,
                    format!("a node key is {HUDDLE_NODE_KEY_BYTES} bytes"),
                ));
            }
            let mut ch = channel(store, &channel_id)?;
            writable(store, &ch, &frame.party)?;
            let entry = HuddleEntry {
                party: actor.clone(),
                node: hex(&node),
                joined_at: frame.time,
            };
            match ch.huddle.iter().position(|e| e.party == actor) {
                Some(seat) => ch.huddle[seat] = entry,
                None if ch.huddle.len() >= MAX_HUDDLE_MEMBERS => {
                    return Err(refuse(reason::CAPACITY, "the huddle is full"));
                }
                None => ch.huddle.push(entry),
            }
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
        ChatMsg::LeaveHuddle { channel_id } => {
            let mut ch = channel(store, &channel_id)?;
            ch.huddle.retain(|e| e.party != actor);
            save(store, chan_key(&channel_id), &ch);
            Ok(())
        }
    }
}

fn create_channel(
    store: &mut impl Write,
    frame: &Frame,
    id: String,
    name: String,
    post_policy: PostPolicy,
    voice: bool,
) -> Result<(), Refusal> {
    checked_id("channel_id", &id)?;
    checked_name(&name)?;
    if load::<ChannelRow>(store, &chan_key(&id))?.is_some() {
        return Err(refuse(
            reason::ALREADY_EXISTS,
            format!("channel {id} exists"),
        ));
    }
    let ch = ChannelRow {
        id: id.clone(),
        name,
        created_at: frame.time,
        post_policy,
        owner: party_handle(&frame.party),
        archived: false,
        huddle: Vec::new(),
        voice,
    };
    save(store, chan_key(&id), &ch);
    Ok(())
}

fn set_member(store: &mut impl Write, frame: &Frame, ch: &str, party: &Party, member: bool) {
    let key = member_key(ch, &party_handle(party));
    if member {
        let row = MemberRow {
            party: party_handle(party),
            height: frame.height,
            time: frame.time,
        };
        save(store, key, &row);
    } else {
        store.delete(key.as_bytes());
    }
}

fn react(
    store: &mut impl Write,
    frame: &Frame,
    channel_id: &str,
    seq: u64,
    emoji: &str,
    on: bool,
) -> Result<(), Refusal> {
    if emoji.is_empty() || emoji.len() > MAX_EMOJI_BYTES || emoji.contains('/') {
        return Err(refuse(reason::INVALID_INPUT, "not an emoji"));
    }
    let ch = channel(store, channel_id)?;
    writable(store, &ch, &frame.party)?;
    let mut row = row(store, channel_id, seq)?;
    if row.deleted {
        return Err(refuse(reason::WRONG_STATE, "the message is deleted"));
    }
    let key = react_key(channel_id, seq, emoji, &party_handle(&frame.party));
    if store.get(key.as_bytes()).is_some() == on {
        return Ok(());
    }
    let at = row.reactions.iter().position(|r| r.emoji == emoji);
    match (on, at) {
        (true, Some(i)) => row.reactions[i].count += 1,
        (true, None) => {
            if row.reactions.len() >= MAX_REACTION_EMOJIS {
                return Err(refuse(reason::CAPACITY, "no room for another emoji"));
            }
            row.reactions.push(ReactionSummary {
                emoji: emoji.to_string(),
                count: 1,
                reacted_by_me: false,
            });
            row.reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
        }
        (false, Some(i)) => {
            row.reactions[i].count -= 1;
            if row.reactions[i].count == 0 {
                row.reactions.remove(i);
            }
        }
        (false, None) => return Ok(()),
    }
    if on {
        mark(store, key);
    } else {
        store.delete(key.as_bytes());
    }
    put_row(store, &row)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Memory(BTreeMap<Vec<u8>, Vec<u8>>);

    impl Read for Memory {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.0.get(key).cloned()
        }
        fn scan(&self, scan: Scan) -> Vec<Entry> {
            let mut hits: Vec<Entry> = self
                .0
                .iter()
                .filter(|(key, _)| scan.admits(key))
                .map(|(key, value)| Entry {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect();
            if scan.reverse {
                hits.reverse();
            }
            if let Some(limit) = scan.limit {
                hits.truncate(limit as usize);
            }
            hits
        }
    }

    impl Write for Memory {
        fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
            self.0.insert(key, value);
        }
        fn delete(&mut self, key: &[u8]) {
            self.0.remove(key);
        }
    }

    fn frame(party: Party) -> Frame {
        Frame {
            party,
            height: 1,
            time: 1000,
        }
    }

    fn post(
        store: &mut Memory,
        who: u64,
        ch: &str,
        id: &str,
        text: &str,
        thread: Option<u64>,
    ) -> Result<(), Refusal> {
        execute(
            store,
            &frame(Party::Account(who)),
            ChatMsg::PostMessage {
                channel_id: ch.into(),
                message_id: id.into(),
                blocks: parse_message(text),
                thread,
            },
        )
    }

    #[test]
    fn a_channel_takes_posts_threads_reactions_and_answers_the_view() {
        let mut store = Memory::default();
        let ada = frame(Party::Account(1));
        execute(
            &mut store,
            &ada,
            ChatMsg::CreateChannel {
                channel_id: "general".into(),
                name: "General".into(),
                post_policy: PostPolicy::MembersOnly,
            },
        )
        .unwrap();
        assert_eq!(
            post(&mut store, 2, "general", "m1", "hi", None)
                .unwrap_err()
                .reason,
            reason::UNAUTHORIZED
        );
        execute(
            &mut store,
            &ada,
            ChatMsg::SetMembership {
                channel_id: "general".into(),
                party: Party::Account(2),
                member: true,
            },
        )
        .unwrap();
        post(&mut store, 2, "general", "m1", "hello #World", None).unwrap();
        post(&mut store, 1, "general", "m2", "hello back", Some(1)).unwrap();
        post(&mut store, 1, "general", "m3", "another root", None).unwrap();
        assert_eq!(
            post(&mut store, 1, "general", "m3", "dup", None)
                .unwrap_err()
                .reason,
            reason::ALREADY_EXISTS
        );
        execute(
            &mut store,
            &ada,
            ChatMsg::AddReaction {
                channel_id: "general".into(),
                seq: 1,
                emoji: "👍".into(),
            },
        )
        .unwrap();

        let ChatViewReply::Roots {
            roots, has_more, ..
        } = query(
            &store,
            ChatViewQuery::Roots {
                channel_id: "general".into(),
                viewer_handles: vec!["acct:1".into()],
                before_seq: None,
                limit: Some(10),
            },
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(!has_more);
        assert_eq!(roots.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(roots[0].reply_count, 1);
        assert_eq!(roots[0].tags, vec!["world"]);
        assert!(roots[0].reactions[0].reacted_by_me);

        let ChatViewReply::Thread { root, replies, .. } = query(
            &store,
            ChatViewQuery::Thread {
                channel_id: "general".into(),
                root_seq: 1,
                viewer_handles: vec![],
                after_reply_seq: None,
                limit: None,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!((root.unwrap().seq, replies[0].seq), (1, 2));

        let ChatViewReply::Hits(hits) = query(
            &store,
            ChatViewQuery::Search {
                text: "hello".into(),
                viewer_handles: vec![],
                channel_id: None,
                limit: None,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            hits.hits.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![2, 1]
        );

        let ChatViewReply::TagHits(tags) = query(
            &store,
            ChatViewQuery::TagSearch {
                tag: "#World".into(),
                viewer_handles: vec![],
                channel_id: Some("general".into()),
                after: None,
                limit: None,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(tags.hits[0].seq, 1);

        execute(
            &mut store,
            &frame(Party::Account(2)),
            ChatMsg::DeleteMessage {
                channel_id: "general".into(),
                seq: 1,
            },
        )
        .unwrap();
        let ChatViewReply::Hits(hits) = query(
            &store,
            ChatViewQuery::Search {
                text: "hello".into(),
                viewer_handles: vec![],
                channel_id: Some("general".into()),
                limit: None,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(hits.hits.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![2]);
    }
}
