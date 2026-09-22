use std::collections::{BTreeMap, BTreeSet};
use ducktape_view_guest::{Loaded, Task, UniformListScrollHandle, composer::Draft};
use serde::{Deserialize, Serialize};
use crate::{api::Session, *};

#[derive(Default, Serialize, Deserialize)]
pub struct Forge {
    pub(crate) nav: Navigation,
    pub(crate) session: Session,
    pub(crate) filter: Filter,
    pub(crate) search: String,
    pub(crate) tree_search: String,
    pub(crate) reviews: BTreeMap<String, ReviewSession>,
    pub(crate) viewed: BTreeSet<String>,
    pub(crate) seen: BTreeMap<String, u64>,
    pub(crate) seen_replies: BTreeMap<String, u64>,
    pub(crate) form: Option<ChangeForm>,
    pub(crate) new_repo: Option<NewRepo>,
    pub(crate) settings: Option<SettingsForm>,
    pub(crate) drafts: BTreeMap<String, Draft>,
    pub(crate) notice: String,
    pub(crate) layout: Layout,
    #[serde(skip)]
    pub(crate) data: BTreeMap<String, Loaded<Reply>>,
    #[serde(skip)]
    pub(crate) requests: BTreeMap<String, Query>,
    #[serde(skip)]
    pub(crate) names: Loaded<Vec<Person>>,
    #[serde(skip)]
    pub(crate) messages: Loaded<Vec<chat::MsgRow>>,
    #[serde(skip)]
    pub(crate) thread: Loaded<Vec<chat::MsgRow>>,
    #[serde(skip)]
    pub(crate) pending_messages: Vec<chat::MsgRow>,
    #[serde(skip)]
    pub(crate) pending: Option<Pending>,
    #[serde(skip)]
    pub(crate) watches: Vec<Task<()>>,
    #[serde(skip)]
    pub(crate) diff_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Navigation {
    pub repo: Option<String>,
    pub tab: RepoTab,
    #[serde(with = "borsh_state")]
    pub revision: Option<Revision>,
    pub change: Option<u64>,
    pub change_tab: ChangeTab,
    pub commit: Option<String>,
    pub path: Vec<u8>,
    pub blob: Option<String>,
    pub diff_path: Option<Vec<u8>>,
    pub thread: Option<u64>,
    pub dock: Option<Dock>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RepoTab { #[default] Code, Commits, Changes, Refs, Settings }
impl RepoTab {
    pub const ALL: [Self; 5] = [Self::Code, Self::Commits, Self::Changes, Self::Refs, Self::Settings];
    pub fn label(self) -> &'static str { match self { Self::Code => "Code", Self::Commits => "Commits", Self::Changes => "Changes", Self::Refs => "Refs", Self::Settings => "Settings" } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ChangeTab { #[default] Conversation, Commits, Files }
impl ChangeTab {
    pub fn label(self) -> &'static str { match self { Self::Conversation => "Conversation", Self::Commits => "Commits", Self::Files => "Files" } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Dock { About, Overview, Comments, MergeStatus }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Filter { Judgment, #[default] Open, Merged, Closed, Authored, Involves }
impl Filter {
    pub const ALL: [Self; 6] = [Self::Judgment, Self::Open, Self::Merged, Self::Closed, Self::Authored, Self::Involves];
    pub fn label(self) -> &'static str { match self { Self::Judgment => "Needs my judgment", Self::Open => "Open", Self::Merged => "Merged", Self::Closed => "Closed", Self::Authored => "Authored by me", Self::Involves => "Involves me" } }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ReviewSession {
    #[serde(with = "borsh_state")]
    pub draft: ReviewDraft,
    #[serde(with = "borsh_state")]
    pub anchor: Option<LineComment>,
    pub finishing: bool,
    pub error: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChangeForm {
    pub edit: Option<u64>,
    #[serde(with = "borsh_state")]
    pub from: Option<Revision>,
    pub into: Vec<u8>,
    pub title: String,
    pub body: String,
    pub reviewers: Vec<Vec<u8>>,
    pub person_filter: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct NewRepo { pub name: String, pub sha256: bool }
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct SettingsForm {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
    pub person_filter: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Layout {
    pub width: f32,
    pub height: f32,
    pub tree_width: f32,
    pub tree_open: bool,
    pub dock_open: bool,
}
impl Default for Layout {
    fn default() -> Self { Self { width: 1180., height: 760., tree_width: 236., tree_open: false, dock_open: false } }
}
impl Layout {
    pub fn narrow(&self) -> bool { self.width < 850. }
    pub fn tree_visible(&self) -> bool { !self.narrow() || self.tree_open }
    pub fn dock_visible(&self) -> bool { !self.narrow() || self.dock_open }
}
#[derive(Clone, Debug)]
pub(crate) struct Person { pub number: u64, pub name: String, pub keys: Vec<Vec<u8>> }
pub(crate) struct Pending {
    pub op: Op,
    pub label: String,
    pub receipt: Option<OpReply>,
    pub accepted: bool,
    pub generation: u64,
}

/// Snapshots keep program-owned draft types in their native codec.
pub(crate) mod borsh_state {
    use borsh::{BorshDeserialize, BorshSerialize};
    use serde::{Deserialize, Serializer, Deserializer};
    pub fn serialize<T: BorshSerialize, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&borsh::to_vec(value).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, T: BorshDeserialize, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
        let bytes = Vec::<u8>::deserialize(d)?;
        borsh::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}
