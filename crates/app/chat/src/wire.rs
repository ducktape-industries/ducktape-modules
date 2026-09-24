use super::*;
use borsh::{BorshDeserialize, BorshSerialize};

// ── wire ────────────────────────────────────────────────────────────────────

#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PostPolicy {
    Open,
    MembersOnly,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

#[derive(
    BorshSerialize, BorshDeserialize, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq,
)]
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

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReactionSummary {
    pub emoji: String,
    pub count: u64,
    /// filled per reader from `viewer_handles`
    pub reacted_by_me: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HuddleEntry {
    pub party: String,
    /// the node key, hex
    pub node: String,
    pub joined_at: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelInfo {
    #[serde(flatten)]
    pub channel: ChannelRow,
    pub head_seq: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberRow {
    pub party: String,
    pub height: u64,
    pub time: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageHits {
    pub hits: Vec<MsgRow>,
    pub capped: bool,
}

/// `viewer_handles` are the reader's handles; they decide `reacted_by_me`.
/// Every list takes a [`Page`] and answers a [`PageReply`]: `page.after` is
/// the `next` of the previous reply, `page.limit` the rows wanted.
#[derive(BorshSerialize, BorshDeserialize, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatViewQuery {
    Channels {
        #[serde(default)]
        page: Page,
    },
    Channel {
        channel_id: String,
    },
    /// Resolve an emitted system line to the root sequence used by Thread.
    MessageById {
        message_id: String,
    },
    /// The author's most recently answered thread in this channel, if any.
    ThreadAttention {
        channel_id: String,
        author: Party,
    },
    /// one page of timeline roots, newest first
    Roots {
        channel_id: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        page: Page,
    },
    /// `page.limit` messages centred on `seq`
    MessagesAround {
        channel_id: String,
        seq: u64,
        viewer_handles: Vec<String>,
        #[serde(default)]
        page: Page,
    },
    /// the root plus one page of replies, post order
    Thread {
        channel_id: String,
        root_seq: u64,
        viewer_handles: Vec<String>,
        #[serde(default)]
        page: Page,
    },
    Members {
        channel_id: String,
        #[serde(default)]
        page: Page,
    },
    /// every token of `text`, newest first, at most `page.limit` hits
    Search {
        text: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        page: Page,
    },
    TagSearch {
        tag: String,
        viewer_handles: Vec<String>,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        page: Page,
    },
    /// the identity roster, ascending by number: the program asks identity
    /// so the view links one module
    Accounts {
        #[serde(default)]
        page: Page,
    },
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRow {
    pub number: AccountNumber,
    pub name: String,
    /// a program-controlled account: an agent, not a person
    pub program: bool,
    /// the account's keys, hex
    pub keys: Vec<String>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatViewReply {
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

/// The party a handle names: [`party_handle`] read back.
pub fn party_of_handle(handle: &str) -> Option<Party> {
    if handle == "system" {
        return Some(Party::System);
    }
    if let Some(number) = handle.strip_prefix("acct:") {
        return number.parse().ok().map(Party::Account);
    }
    if let Some(module) = handle.strip_prefix("module:") {
        return Some(Party::Module(module.into()));
    }
    let hex = handle.strip_prefix("user:").filter(|hex| !hex.is_empty())?;
    abi::unhex(hex).map(Party::Key)
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

pub use abi::hex;
