//! The forge program's borsh wire, as this view speaks it.
//!
//! The definition site is `crates/app/forge/src/{contract,read_contract,
//! review_contract}.rs`. That crate is a *program*: it links `gitcore` and
//! `guest`, and its wasm32 entry exports the program ABI, so a view must not
//! link it (the Makefile says as much: "the view links `chat`, never a
//! program"). Chat solved this by keeping its wire in a separate `chat`
//! crate; forge has not been split yet, so the wire is mirrored here and
//! pinned by `tests/contract.rs`, which replays every committed fixture —
//! real bytes captured off the running program — through these types and
//! requires an exact re-encode. Drift fails that test, not a screen.
//!
//! Only borsh derives: this is a wire, never a snapshot. Order of variants
//! and fields is the wire, so nothing here may be reordered or trimmed.
use abi::HashKind;
use borsh::{BorshDeserialize, BorshSerialize};

// ---------------------------------------------------------------- read wire

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Revision {
    Ref(Vec<u8>),
    Oid(String),
}

/// Opaque to clients. Bound to the query arguments and answering height.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Cursor {
    pub height: u64,
    pub scope: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<Cursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct GitSignature {
    pub name: Vec<u8>,
    pub email: Vec<u8>,
    pub time: i64,
    pub offset_minutes: i16,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CommitInfo {
    pub oid: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: GitSignature,
    pub committer: GitSignature,
    pub message: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum EntryKind {
    File,
    Executable,
    Symlink,
    Directory,
    Gitlink,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TreeInfo {
    pub name: Vec<u8>,
    pub oid: String,
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ByteRange {
    pub offset: u64,
    pub len: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Content {
    Text,
    Binary,
    Oversize,
    Gitlink,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BlobView {
    pub oid: String,
    pub size: u64,
    pub content: Content,
    pub range: ByteRange,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    ModeChanged,
    TypeChanged,
}

/// One-based start; an empty range names the preceding line (0 at the beginning).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LineRange {
    pub start: u64,
    pub count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum LineKind {
    Context,
    Added,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
    /// Includes the original newline, if any. No patch markers or escaping.
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DiffHunk {
    pub old: LineRange,
    pub new: LineRange,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FileDiff {
    pub old_path: Option<Vec<u8>>,
    pub new_path: Option<Vec<u8>>,
    pub status: FileStatus,
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    pub old_kind: Option<EntryKind>,
    pub new_kind: Option<EntryKind>,
    pub old_size: u64,
    pub new_size: u64,
    pub content: Content,
    pub additions: u64,
    pub deletions: u64,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Mergeability {
    UpToDate,
    FastForward,
    Clean,
    Conflicts,
    Unrelated,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Comparison {
    pub from: String,
    pub into: String,
    pub base: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub mergeability: Mergeability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ConflictKind {
    Content,
    AddAdd,
    ModifyDelete,
    Mode,
    Type,
    Submodule,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Conflict {
    pub path: Vec<u8>,
    pub kind: ConflictKind,
}

// -------------------------------------------------------------- review wire

pub const MAX_REVIEW_COMMENTS: usize = 64;
pub const MAX_REVIEWERS: usize = 64;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
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
    pub channel: String,
    pub system_seq: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ReviewCounts {
    pub approve: u64,
    pub request_changes: u64,
    pub comment: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
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

// ------------------------------------------------------------- program wire

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Bounds {
    pub max_objects: u64,
    pub max_delta_depth: u64,
    pub max_object_size: u64,
    pub push_walk: u64,
    pub fetch_walk: u64,
    /// Maximum Myers edit distance for one file.
    pub merge_cost: u64,
    pub page_size: u32,
    /// Maximum commits in a history/compare walk.
    pub log_walk: u64,
    /// Maximum object reads in a tree/diff/compare query.
    pub tree_walk: u64,
    /// Aggregate object bytes read by a query (including trees and commits).
    pub diff_bytes: u64,
    /// Largest blob displayed inline. Larger blobs return a header.
    pub blob_bytes: u64,
    /// Maximum encoded change or review record.
    pub record_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Settings {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            head: b"refs/heads/main".to_vec(),
            allow_force: false,
            allow_delete: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Repo {
    pub hash: HashKind,
    pub owner: Vec<u8>,
    pub settings: Settings,
    pub refs_count: u64,
    pub last_activity: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Create {
        repo: String,
        hash: HashKind,
    },
    Configure {
        repo: String,
        settings: Settings,
    },
    Grant {
        repo: String,
        key: Vec<u8>,
    },
    Revoke {
        repo: String,
        key: Vec<u8>,
    },
    Push {
        repo: String,
        request: Vec<u8>,
    },
    /// The client publishes immutable objects first. Consensus only checks both heads.
    Merge {
        repo: String,
        into: Vec<u8>,
        from: Revision,
        expected_into: String,
        expected_from: String,
        result: String,
        change: Option<u64>,
    },
    ChangeOpen {
        repo: String,
        from: Revision,
        into: Vec<u8>,
        title: String,
        body: String,
        reviewers: Vec<Vec<u8>>,
    },
    ChangeEdit {
        repo: String,
        n: u64,
        title: Option<String>,
        body: Option<String>,
        reviewers: Option<Vec<Vec<u8>>>,
    },
    ChangeClose {
        repo: String,
        n: u64,
    },
    ReviewSubmit {
        repo: String,
        n: u64,
        review: ReviewDraft,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Service {
    ReceivePack,
    UploadPack,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    Repos {
        cursor: Option<Cursor>,
        limit: u32,
    },
    /// Settings plus a page of granted keys; the owner is on the repo record.
    Repo {
        repo: String,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Refs {
        repo: String,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Advertise {
        repo: String,
        service: Service,
    },
    Upload {
        repo: String,
        request: Vec<u8>,
    },
    Log {
        repo: String,
        from: Revision,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Tree {
        repo: String,
        at: String,
        path: Vec<u8>,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Blob {
        repo: String,
        oid: String,
        range: Option<ByteRange>,
    },
    /// None base means the empty tree, for a root commit's diff.
    Diff {
        repo: String,
        base: Option<String>,
        head: String,
        path: Option<Vec<u8>>,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Compare {
        repo: String,
        from: Revision,
        into: Revision,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Activity {
        repo: String,
    },
    Changes {
        repo: String,
        filter: ChangeFilter,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Change {
        repo: String,
        n: u64,
        cursor: Option<Cursor>,
        limit: u32,
    },
    Judgment {
        key: Vec<u8>,
        cursor: Option<Cursor>,
        limit: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RepoInfo {
    pub name: String,
    pub repo: Repo,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RefInfo {
    pub name: Vec<u8>,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Repos {
        height: u64,
        page: Page<RepoInfo>,
    },
    Repo {
        height: u64,
        repo: RepoInfo,
        bounds: Bounds,
        writers: Page<Vec<u8>>,
    },
    Refs {
        height: u64,
        page: Page<RefInfo>,
    },
    Log {
        height: u64,
        tip: String,
        page: Page<CommitInfo>,
    },
    Tree {
        height: u64,
        tree: String,
        page: Page<TreeInfo>,
    },
    Blob {
        height: u64,
        blob: BlobView,
    },
    Diff {
        height: u64,
        base: Option<String>,
        head: String,
        total_files: u64,
        page: Page<FileDiff>,
    },
    Compare {
        height: u64,
        comparison: Comparison,
        conflicts: Page<Conflict>,
    },
    Activity {
        height: u64,
        last_height: u64,
    },
    Changes {
        height: u64,
        page: Page<ChangeSummary>,
    },
    Change {
        height: u64,
        change: Change,
        source_head: Option<String>,
        target_head: Option<String>,
        reviews: Page<Review>,
    },
    Judgment {
        height: u64,
        page: Page<Judgment>,
    },
    Refused {
        height: u64,
        reason: String,
        sentence: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum OpReply {
    Change { height: u64, n: u64 },
    Review { height: u64, n: u64, id: u64 },
    Merged {
        height: u64,
        oid: String,
        change: Option<u64>,
    },
}

pub const MAX_REPO_NAME: usize = 37; // forge:<repo>:<u64> fits chat's 64-byte id.

pub fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_REPO_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with('.')
        && !name.ends_with(".git")
}
