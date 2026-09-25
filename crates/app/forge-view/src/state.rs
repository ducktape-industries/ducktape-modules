//! What the screen is showing and what the reader has typed. Everything the
//! programs speak is folded to rows at render time, so a snapshot carries
//! navigation and drafts, never wire records.
use std::collections::{BTreeMap, BTreeSet};

use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{Task, UniformListScrollHandle};
use serde::{Deserialize, Serialize};

use crate::api::Session;
use forge::{LineComment, Query, Reply, Revision, Side, Verdict};

#[derive(Default, Serialize, Deserialize)]
pub struct Forge {
    pub(crate) nav: Nav,
    pub(crate) session: Session,
    /// the account the seated key holds, resolved through identity
    /// (`None` while unregistered or not yet answered)
    #[serde(skip)]
    pub(crate) me: Loaded<Option<u64>>,
    pub(crate) filter: Filter,
    /// the repo-list / change-list filter box
    pub(crate) search: String,
    /// the file-tree filter box
    pub(crate) tree_search: String,
    pub(crate) reviews: BTreeMap<String, ReviewSession>,
    /// `<repo>#<n>:<path>` of every file the reader ticked off
    pub(crate) viewed: BTreeSet<String>,
    pub(crate) new_repo: Option<NewRepo>,
    pub(crate) form: Option<ChangeForm>,
    pub(crate) repo_settings: Option<SettingsForm>,
    /// the conversation composer of the open change
    pub(crate) reply: String,
    pub(crate) notice: String,
    /// the repository whose address was copied last, so its row says so
    #[serde(skip)]
    pub(crate) copied: Option<String>,
    pub(crate) layout: Layout,
    #[serde(skip)]
    /// every read on screen, keyed by the query that asked it
    pub(crate) data: BTreeMap<Query, Loaded<Reply>>,
    #[serde(skip)]
    pub(crate) names: Loaded<Names>,
    #[serde(skip)]
    pub(crate) messages: BTreeMap<String, Loaded<Vec<chat::MsgRow>>>,
    #[serde(skip)]
    pub(crate) pending: Vec<Pending>,
    #[serde(skip)]
    pub(crate) watches: Vec<Task<()>>,
    #[serde(skip)]
    pub(crate) diff_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) log_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) tree_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) next_pending: u64,
    #[serde(skip)]
    pub(crate) blob_cache: crate::ui::code::BlobCache,
}

/// Where the reader is. One flat record: every screen is a projection of it,
/// and a snapshot restores the same screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Nav {
    pub repo: Option<String>,
    pub tab: RepoTab,
    /// the picked ref, a full name such as `refs/heads/main`
    pub rev: Option<Vec<u8>>,
    /// the directories the code tree has open, by full path
    pub expanded: BTreeSet<Vec<u8>>,
    /// the tree row the keyboard is on (a file or a directory)
    pub cursor: Option<Vec<u8>>,
    /// the open file: its path and blob oid
    pub blob: Option<(Vec<u8>, String)>,
    /// a file a link named, opened once its folder's tree lands
    #[serde(skip)]
    pub goto: Option<Vec<u8>>,
    pub commit: Option<String>,
    pub change: Option<u64>,
    pub change_tab: ChangeTab,
    /// single-file mode in the change's diff
    pub diff_path: Option<Vec<u8>>,
    pub dock: Option<Dock>,
}

impl Nav {
    pub fn revision(&self, head: &[u8]) -> Revision {
        Revision::Ref(self.rev.clone().unwrap_or_else(|| head.to_vec()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RepoTab {
    /// the repository's front page: its README, rendered
    #[default]
    Readme,
    Code,
    Commits,
    Changes,
    Refs,
    Settings,
}

impl RepoTab {
    pub const ALL: [Self; 6] = [
        Self::Readme,
        Self::Code,
        Self::Commits,
        Self::Changes,
        Self::Refs,
        Self::Settings,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Readme => "README",
            Self::Code => "Code",
            Self::Commits => "Commits",
            Self::Changes => "Changes",
            Self::Refs => "Refs",
            Self::Settings => "Settings",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Readme => "readme",
            Self::Code => "code",
            Self::Commits => "commits",
            Self::Changes => "changes",
            Self::Refs => "refs",
            Self::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ChangeTab {
    #[default]
    Conversation,
    Commits,
    Files,
}

impl ChangeTab {
    pub const ALL: [Self; 3] = [Self::Conversation, Self::Commits, Self::Files];
    pub fn label(self) -> &'static str {
        match self {
            Self::Conversation => "Conversation",
            Self::Commits => "Commits",
            Self::Files => "Files",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Commits => "commits",
            Self::Files => "files",
        }
    }
}

/// The one docked panel a screen shows at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Dock {
    About,
    Overview,
    Comments,
    MergeStatus,
}

impl Dock {
    pub const CHANGE: [Self; 3] = [Self::Overview, Self::Comments, Self::MergeStatus];
    pub fn label(self) -> &'static str {
        match self {
            Self::About => "About",
            Self::Overview => "Overview",
            Self::Comments => "Comments",
            Self::MergeStatus => "Merge status",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::About => "about",
            Self::Overview => "overview",
            Self::Comments => "comments",
            Self::MergeStatus => "merge-status",
        }
    }
}

/// The change-list filters. "Needs my judgment" is its own query, not a
/// `ChangeFilter`: the program answers it from the reader's key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Filter {
    Judgment,
    #[default]
    Open,
    Merged,
    Closed,
    Authored,
    Involves,
}

impl Filter {
    pub const ALL: [Self; 6] = [
        Self::Judgment,
        Self::Open,
        Self::Merged,
        Self::Closed,
        Self::Authored,
        Self::Involves,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Judgment => "Needs my judgment",
            Self::Open => "Open",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Authored => "Authored by me",
            Self::Involves => "Involves me",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Judgment => "judgment",
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Authored => "authored",
            Self::Involves => "involves",
        }
    }
}

/// A review being written: pending comments staged against one pinned pair
/// of endpoints, published as exactly one `ReviewSubmit`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ReviewSession {
    pub commit: String,
    pub base: Option<String>,
    pub comments: Vec<PendingComment>,
    /// the anchor whose composer is open
    pub open: Option<PendingComment>,
    pub body: String,
    pub finishing: bool,
    pub error: String,
}

impl ReviewSession {
    /// One draft per anchor: re-staging the same line replaces it.
    pub fn stage(&mut self, comment: PendingComment) {
        match self
            .comments
            .iter_mut()
            .find(|staged| staged.anchors(&comment))
        {
            Some(staged) => *staged = comment,
            None => self.comments.push(comment),
        }
    }
    pub fn staged(&self, path: &[u8], new_side: bool, line: u64) -> Option<&PendingComment> {
        self.comments
            .iter()
            .find(|c| c.path == path && c.new_side == new_side && c.line == line)
    }
    pub fn line_comments(&self) -> Vec<LineComment> {
        self.comments
            .iter()
            .map(|c| LineComment {
                path: c.path.clone(),
                side: if c.new_side { Side::New } else { Side::Old },
                line: c.line,
                body: c.body.clone(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingComment {
    pub path: Vec<u8>,
    pub new_side: bool,
    pub line: u64,
    pub body: String,
}

impl PendingComment {
    pub fn anchors(&self, other: &Self) -> bool {
        self.path == other.path && self.new_side == other.new_side && self.line == other.line
    }
    pub fn anchor(&self) -> String {
        format!(
            "{}:{} ({})",
            String::from_utf8_lossy(&self.path),
            self.line,
            if self.new_side { "new" } else { "old" }
        )
    }
}

pub(crate) fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
        Verdict::Comment => "Comment",
    }
}

/// What a reviewer did, as the conversation says it after their name.
pub(crate) fn verdict_verb(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "approved",
        Verdict::RequestChanges => "requested changes",
        Verdict::Comment => "commented",
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ChangeForm {
    /// `Some(n)` edits that change instead of opening a new one
    pub edit: Option<u64>,
    pub from: Vec<u8>,
    pub into: Vec<u8>,
    pub title: String,
    pub body: String,
    pub reviewers: Vec<Vec<u8>>,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NewRepo {
    pub name: String,
    pub sha256: bool,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct SettingsForm {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
    pub grant: String,
}

/// An operation the reader issued: shown where she issued it until the
/// next query reconciles it, or refused with the reason inline.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub id: u64,
    /// the screen the row belongs to
    pub scope: String,
    pub label: String,
    pub progress: Progress,
}

/// Where an issued operation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Submitting,
    /// accepted by the node, waiting for the block that carries it
    Accepted,
    /// refused, with the program's sentence
    Refused(String),
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Layout {
    pub width: f32,
    pub height: f32,
    /// the repositories rail
    pub tree: f32,
    /// the Code tab's file tree
    pub files: f32,
    pub tree_open: bool,
    pub dock_open: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: 1180.,
            height: 760.,
            tree: 248.,
            files: 300.,
            tree_open: false,
            dock_open: false,
        }
    }
}

impl Layout {
    /// A dragged pane keeps its neighbour usable.
    pub fn clamp(&mut self) {
        self.tree = self.tree.clamp(160., 480.);
        self.files = self.files.clamp(180., 640.);
    }
    pub fn narrow(&self) -> bool {
        self.width < 880.
    }
    pub fn tree_visible(&self) -> bool {
        !self.narrow() || self.tree_open
    }
    pub fn dock_visible(&self) -> bool {
        !self.narrow() || self.dock_open
    }
}

/// The identity roster, folded to what a key or handle is called.
#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    rows: Vec<chat::AccountRow>,
}

impl Names {
    pub fn new(rows: Vec<chat::AccountRow>) -> Self {
        Self { rows }
    }
    pub fn rows(&self) -> &[chat::AccountRow] {
        &self.rows
    }
    /// What a raw forge key is called, or its short hex.
    pub fn key(&self, key: &[u8]) -> String {
        let hex = abi::hex(key);
        self.rows
            .iter()
            .find(|row| row.keys.iter().any(|held| held.eq_ignore_ascii_case(&hex)))
            .map(|row| row.name.clone())
            .unwrap_or_else(|| ducktape_view_guest::design::short_hex(&hex))
    }
    /// What a chat handle (`acct:7`, `user:<hex>`, `system`) is called.
    pub fn handle(&self, handle: &str) -> String {
        match chat::party_of_handle(handle) {
            Some(chat::Party::Account(number)) => self
                .rows
                .iter()
                .find(|row| row.number == number)
                .map(|row| row.name.clone())
                .unwrap_or_else(|| handle.to_owned()),
            Some(chat::Party::Key(key)) => self.key(&key),
            Some(chat::Party::System) => "Forge".into(),
            Some(chat::Party::Module(_)) | None => handle.to_owned(),
        }
    }
    /// The signing key of an account number: `host.props` names no key, and
    /// forge's filters and judgment are keyed by the exact key.
    pub fn key_of(&self, number: u64) -> Option<Vec<u8>> {
        let row = self.rows.iter().find(|row| row.number == number)?;
        unhex(row.keys.first()?)
    }
}

/// [`abi::unhex`], where the empty string is not a key either.
pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    abi::unhex(text).filter(|bytes| !bytes.is_empty())
}

/// The key a change's screens and drafts hang on.
pub(crate) fn change_key(repo: &str, n: u64) -> String {
    format!("{repo}#{n}")
}
