//! Immutable batched reviews, mutable changes, and the shared per-repo item counter.
use crate::read_contract::Revision;
use borsh::{BorshDeserialize, BorshSerialize};

pub const MAX_REVIEW_COMMENTS: usize = 64;
pub const MAX_REVIEWERS: usize = 64;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize)]
pub enum ChangeState {
    Open,
    Closed,
    Merged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
pub enum Side {
    Old,
    New,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LineComment {
    pub path: Vec<u8>,
    pub side: Side,
    pub line: u64,
    pub body: String,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ReviewDraft {
    pub commit_oid: String,
    pub base_oid: Option<String>,
    pub verdict: Verdict,
    pub body: String,
    pub comments: Vec<LineComment>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Review {
    pub id: u64,
    pub author: Vec<u8>,
    pub height: u64,
    pub time: u64,
    pub draft: ReviewDraft,
    /// Look up this chat root by MessageById; all line discussions are chat replies.
    pub message_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Change {
    pub n: u64,
    pub from: Revision,
    pub into: Vec<u8>,
    pub title: String,
    pub body: String,
    pub author: Vec<u8>,
    pub state: ChangeState,
    pub reviewers: Vec<Vec<u8>>,
    pub created_height: u64,
    pub updated_height: u64,
    pub created_time: u64,
    pub updated_time: u64,
    pub review_count: u64,
    pub comment_count: u64,
    pub verdicts: ReviewCounts,
    pub merge_oid: Option<String>,
    /// Who closed the change; set once, by the close op.
    pub closed_by: Option<Vec<u8>>,
    /// Who merged the change; set once, by a merge linked to it.
    pub merged_by: Option<Vec<u8>>,
    pub channel: String,
    pub system_seq: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ReviewCounts {
    pub approve: u64,
    pub request_changes: u64,
    pub comment: u64,
}
#[derive(
    Clone, Debug, Default, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize,
)]
pub struct ChangeFilter {
    pub state: Option<ChangeState>,
    pub author: Option<Vec<u8>>,
    pub involves: Option<Vec<u8>>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ChangeSummary {
    pub repo: String,
    pub n: u64,
    pub from: Revision,
    pub into: Vec<u8>,
    pub title: String,
    pub author: Vec<u8>,
    pub state: ChangeState,
    pub updated_height: u64,
    pub review_count: u64,
    pub comment_count: u64,
    pub verdicts: ReviewCounts,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ReplyAttention {
    /// None for a conversation thread authored directly in chat.
    pub review: Option<u64>,
    pub root_seq: u64,
    pub last_reply_seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Judgment {
    pub change: ChangeSummary,
    pub requested: bool,
    pub replies: Option<ReplyAttention>,
}
