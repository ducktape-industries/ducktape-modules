//! The `chat` program: channels, messages, threads, reactions, members and
//! huddles.
//!
//! A write is an [`Op`], a read a [`Query`] answered by a [`Reply`], all
//! borsh, the same types `chat-view` links. The acting [`Party`] is the
//! frame's origin: an external key resolved by identity's `party_of` to its
//! account (a key that holds none writes nothing). The layout, in reading order:
//!
//! - `lib.rs` (here): the types on the wire and the rows they carry
//! - `state.rs`: every table and index the program keeps, declared once
//! - `rules.rs`: the checks an op passes before it writes
//! - `ops.rs`: [`execute`], one short function per op
//! - `origin.rs`: [`execute_from`], an origin resolved to its party
//!   through identity, and identity's roster
//! - `queries.rs`: [`query`], one short function per question
//! - `text.rs`: what search and tags read out of a message
//! - `description.rs`: [`describe`], an op in a person's words
//! - `program.rs` (`program` feature): the wasm32 glue over the host
//!
//! The rules run over any [`store::Reads`]/[`store::Writes`], so a native
//! test runs them over [`store::Memory`] exactly as the host does.
mod description;
pub mod message;
mod ops;
mod origin;
#[cfg(feature = "program")]
mod program;
mod queries;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod text;
#[cfg(feature = "view")]
pub mod view;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

pub use abi::hex;
pub use description::describe;
pub use identity::{AccountNumber, Frame, Party};
pub use message::{Block, Mark, Span, parse_message};
pub use ops::execute;
pub use origin::execute_from;
pub use queries::{query, roots_below};
pub use store::{Cursor, Page, PageReply};
pub use text::{plain_text, tags, tokens};

/// The name this program runs under.
pub const PROGRAM: &str = "chat";

pub const MAX_ID_BYTES: usize = 64;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_REVISIONS: u32 = 256;
pub const MAX_EMOJI_BYTES: usize = 64;
pub const MAX_REACTION_EMOJIS: usize = 64;
pub const MAX_THREAD_REPLIES: u64 = 4096;
pub const MAX_HUDDLE_MEMBERS: usize = 32;
/// The most parties a read's `viewer` names (a reader is one account).
pub const MAX_VIEWERS: usize = 8;
pub const HUDDLE_NODE_KEY_BYTES: usize = 32;
/// The namespace a node key signs under to join a huddle; the message is
/// the channel id then the joining origin key.
pub const HUDDLE_JOIN_NS: &[u8] = b"ducktape/huddle-join/v1";
pub const MAX_TAGS_PER_MESSAGE: usize = 16;
pub const MAX_TAG_CHARS: usize = 64;
/// How many postings a search reads before it reports `capped`.
pub const SEARCH_POSTING_CAP: usize = 1024;

/// A write. The enum only grows at its end (see `op_variants_only_append`).
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum Op {
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
    /// id [`dm_channel_id`]; creating it twice is a no-op. The only way a
    /// dm id opens.
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
    /// `thread` names the root this message replies to.
    PostMessage {
        channel_id: String,
        message_id: String,
        blocks: Vec<Block>,
        thread: Option<u64>,
    },
    /// `base_rev` is what the editor started from, recorded, never judged.
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

/// A read. `viewer` is the reader's parties: they decide
/// [`Reaction::reacted_by_me`]. Every list takes a [`Page`] and answers a
/// [`PageReply`] whose `next` resumes it.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum Query {
    Channels {
        page: Page,
    },
    Channel {
        channel_id: String,
    },
    /// The message an emitted id names (forge finds its own posts so).
    MessageById {
        message_id: String,
    },
    /// The author's most recently answered thread in this channel, if any.
    ThreadAttention {
        channel_id: String,
        author: Party,
    },
    /// One page of timeline roots, newest first.
    Roots {
        channel_id: String,
        viewer: Vec<Party>,
        page: Page,
    },
    /// `page.limit` messages centred on `seq`.
    MessagesAround {
        channel_id: String,
        seq: u64,
        viewer: Vec<Party>,
        page: Page,
    },
    /// The root plus one page of replies, in post order.
    Thread {
        channel_id: String,
        root_seq: u64,
        viewer: Vec<Party>,
        page: Page,
    },
    Members {
        channel_id: String,
        page: Page,
    },
    /// Every token of `text`, newest first, at most `page.limit` hits.
    Search {
        text: String,
        viewer: Vec<Party>,
        channel_id: Option<String>,
        page: Page,
    },
    TagSearch {
        tag: String,
        viewer: Vec<Party>,
        channel_id: Option<String>,
        page: Page,
    },
    /// The identity roster, ascending by number, a page at a time: the
    /// program asks identity, so a view links one program.
    Accounts {
        page: Page,
    },
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Channels(PageReply<ChannelInfo>),
    Channel(Option<ChannelInfo>),
    Message(Option<MsgRow>),
    Attention(Option<MsgRow>),
    Roots(PageReply<MsgRow>),
    Messages(Vec<MsgRow>),
    Thread {
        root: Option<MsgRow>,
        replies: PageReply<MsgRow>,
    },
    Members(PageReply<MemberRow>),
    Hits(MessageHits),
    TagHits(PageReply<MsgRow>),
    Accounts(PageReply<AccountRow>),
}

#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
pub enum PostPolicy {
    Open,
    MembersOnly,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub post_policy: PostPolicy,
    pub owner: Party,
    pub archived: bool,
    pub huddle: Vec<HuddleEntry>,
    pub voice: bool,
}

impl ChannelRow {
    pub fn members_only(&self) -> bool {
        self.post_policy == PostPolicy::MembersOnly
    }

    /// Whether the room lets `party` post, `seated` saying whether it holds
    /// a member seat: posting is open, or it owns the room or sits in it.
    /// Archiving aside; the program and the view ask this one rule.
    pub fn admits(&self, party: &Party, seated: bool) -> bool {
        !self.members_only() || self.owner == *party || seated
    }
}

/// A channel and the seq of its newest message.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    pub channel: ChannelRow,
    pub head_seq: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HuddleEntry {
    pub party: Party,
    /// the node key, hex
    pub node: String,
    pub joined_at: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub party: Party,
    pub height: u64,
    pub time: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MsgRow {
    pub channel_id: String,
    pub seq: u64,
    pub message_id: String,
    pub author: Party,
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
    pub reactions: Vec<Reaction>,
    pub tags: Vec<String>,
}

impl MsgRow {
    /// A row with nothing yet but its author: a view's pending post before
    /// the program serves it, a test's base row.
    pub fn by(author: Party) -> MsgRow {
        MsgRow {
            channel_id: String::new(),
            seq: 0,
            message_id: String::new(),
            author,
            height: 0,
            time: 0,
            blocks: Vec::new(),
            text: String::new(),
            deleted: false,
            edited: false,
            rev: 0,
            edited_at: None,
            base_rev: None,
            thread: None,
            reply_count: 0,
            last_reply_seq: None,
            reactions: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// One emoji on a message: how many parties chose it.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    pub count: u64,
    /// filled per reader from the query's `viewer`
    pub reacted_by_me: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MessageHits {
    pub hits: Vec<MsgRow>,
    pub capped: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub number: AccountNumber,
    pub name: String,
    /// a program-controlled account: an agent, not a person
    pub program: bool,
    /// the account's keys, hex
    pub keys: Vec<String>,
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

/// The program a `<program>:<name>` channel id belongs to, or `None` for a
/// channel people opened. Only that program creates one (a review thread,
/// say); a reader reaches it through its program, not the channel list.
/// The program's view opens the room at the id's own path: `forge:web:3`
/// is `duck://<chain>/forge/web/3`.
pub fn program_of(channel_id: &str) -> Option<&str> {
    channel_id.split_once(':').map(|(program, _)| program)
}

/// A program's own post in its own room: written by the module the
/// `<program>:<name>` room belongs to, as one code block in that program's
/// language (forge's `opened`, `review 7`). The code is the program's to
/// word; a reader shows it as that program's event and points to where the
/// program itself shows the room. `(program, code)`.
pub fn program_post(row: &MsgRow) -> Option<(&str, &str)> {
    let program = program_of(&row.channel_id)?;
    let own = matches!(&row.author, Party::Module(module) if module == program);
    match row.blocks.as_slice() {
        [
            Block::Code {
                lang: Some(lang),
                text,
            },
        ] if own && lang == program => Some((program, text)),
        _ => None,
    }
}

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
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

#[test]
fn a_programs_own_code_block_in_its_room_is_its_post() {
    let code = |lang: &str| Block::Code {
        lang: Some(lang.into()),
        text: "review 7".into(),
    };
    let row = |channel: &str, author: Party, blocks| MsgRow {
        channel_id: channel.into(),
        blocks,
        ..MsgRow::by(author)
    };
    let forge = || Party::Module("forge".into());
    let post = row("forge:web:3", forge(), vec![code("forge")]);
    assert_eq!(program_post(&post), Some(("forge", "review 7")));
    for other in [
        row("forge:web:3", Party::Account(1), vec![code("forge")]),
        row("forge:web:3", forge(), vec![code("rust")]),
        row("forge:web:3", forge(), vec![code("forge"), Block::Divider]),
        row("general", forge(), vec![code("forge")]),
        row("chess:1", forge(), vec![code("forge")]),
    ] {
        assert_eq!(program_post(&other), None, "{other:?}");
    }
}
