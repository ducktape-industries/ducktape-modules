//! Borsh is the module contract; the view links these types as they are.
use borsh::{BorshDeserialize, BorshSerialize};
use guest::HashKind;

pub use crate::read_contract::*;
pub use crate::review_contract::*;
pub use guest::Principal;
pub use store::{PageRequest, PageResponse};

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
    /// The person who created the repository: an account.
    pub owner: Principal,
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
    /// Lets a person push, merge and close: an account (all its keys), or
    /// a key that holds no account.
    Grant {
        repo: String,
        principal: Principal,
    },
    Revoke {
        repo: String,
        principal: Principal,
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
        reviewers: Vec<Principal>,
    },
    ChangeEdit {
        repo: String,
        n: u64,
        title: Option<String>,
        body: Option<String>,
        reviewers: Option<Vec<Principal>>,
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

impl Op {
    /// The repository every op acts on.
    pub fn repo(&self) -> &str {
        match self {
            Op::Create { repo, .. }
            | Op::Configure { repo, .. }
            | Op::Grant { repo, .. }
            | Op::Revoke { repo, .. }
            | Op::Push { repo, .. }
            | Op::Merge { repo, .. }
            | Op::ChangeOpen { repo, .. }
            | Op::ChangeEdit { repo, .. }
            | Op::ChangeClose { repo, .. }
            | Op::ReviewSubmit { repo, .. } => repo,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize)]
pub enum Service {
    ReceivePack,
    UploadPack,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    Repos {
        page: PageRequest,
    },
    /// Settings plus a page of granted writers; the owner is on the repo
    /// record.
    Repo {
        repo: String,
        page: PageRequest,
    },
    Refs {
        repo: String,
        page: PageRequest,
    },
    Advertise {
        repo: String,
        service: Service,
    },
    Upload {
        repo: String,
        request: Vec<u8>,
    },
    /// The history `from` reaches, less what `exclude` reaches (`git log
    /// exclude..from`): a change's own commits exclude its target.
    Log {
        repo: String,
        from: Revision,
        exclude: Option<Revision>,
        page: PageRequest,
    },
    Tree {
        repo: String,
        at: String,
        path: Vec<u8>,
        page: PageRequest,
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
        page: PageRequest,
    },
    Compare {
        repo: String,
        from: Revision,
        into: Revision,
    },
    Activity {
        repo: String,
    },
    Changes {
        repo: String,
        filter: ChangeFilter,
        page: PageRequest,
    },
    Change {
        repo: String,
        n: u64,
        page: PageRequest,
    },
    /// What one person owes across every repository.
    Judgment {
        principal: Principal,
        page: PageRequest,
    },
}

impl Query {
    /// The listing a page's cursor is bound to: this query with its page
    /// taken out.
    pub fn scope(&self) -> Vec<u8> {
        let mut scope = self.clone();
        if let Some(page) = scope.page_mut() {
            *page = PageRequest::default();
        }
        PageRequest::scope_of(&scope)
    }

    /// The page a listing asks for; an unpaged query has none.
    pub fn page(&self) -> Option<&PageRequest> {
        match self {
            Query::Repos { page }
            | Query::Repo { page, .. }
            | Query::Refs { page, .. }
            | Query::Log { page, .. }
            | Query::Tree { page, .. }
            | Query::Diff { page, .. }
            | Query::Changes { page, .. }
            | Query::Change { page, .. }
            | Query::Judgment { page, .. } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. } => None,
        }
    }

    /// [`Query::page`], to continue a listing with its `next` cursor.
    pub fn page_mut(&mut self) -> Option<&mut PageRequest> {
        match self {
            Query::Repos { page }
            | Query::Repo { page, .. }
            | Query::Refs { page, .. }
            | Query::Log { page, .. }
            | Query::Tree { page, .. }
            | Query::Diff { page, .. }
            | Query::Changes { page, .. }
            | Query::Change { page, .. }
            | Query::Judgment { page, .. } => Some(page),
            Query::Advertise { .. }
            | Query::Upload { .. }
            | Query::Blob { .. }
            | Query::Compare { .. }
            | Query::Activity { .. } => None,
        }
    }
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

/// One read's answer. A reply is decoded once and read in place, so its
/// widest variant (a whole change record) is not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Repos {
        height: u64,
        page: PageResponse<RepoInfo>,
    },
    Repo {
        height: u64,
        repo: RepoInfo,
        bounds: Bounds,
        writers: PageResponse<Principal>,
    },
    Refs {
        height: u64,
        page: PageResponse<RefInfo>,
    },
    Log {
        height: u64,
        tip: String,
        page: PageResponse<CommitInfo>,
    },
    Tree {
        height: u64,
        tree: String,
        page: PageResponse<TreeInfo>,
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
        page: PageResponse<FileDiff>,
    },
    Compare {
        height: u64,
        comparison: Comparison,
    },
    Activity {
        height: u64,
        last_height: u64,
    },
    Changes {
        height: u64,
        page: PageResponse<ChangeSummary>,
    },
    Change {
        height: u64,
        change: Change,
        source_head: Option<String>,
        target_head: Option<String>,
        reviews: PageResponse<Review>,
    },
    Judgment {
        height: u64,
        page: PageResponse<Judgment>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum OpReply {
    Change {
        height: u64,
        n: u64,
    },
    Review {
        height: u64,
        n: u64,
        id: u64,
    },
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
