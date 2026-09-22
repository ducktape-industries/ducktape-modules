//! State stored by the root view and its panes.
use super::{ChannelInfo, Draft, Loaded, MemberRow, MsgRow, NameDirectory, Session, files};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

#[derive(Serialize, Deserialize, Default)]
pub struct Chat {
    pub(crate) session: Session,
    /// the account the seated key holds, resolved through identity
    /// (`None` while unregistered or not yet answered)
    pub(crate) me: Loaded<Option<u64>>,
    #[serde(skip)]
    pub(crate) names: Loaded<NameDirectory>,
    pub(crate) channels: Loaded<Vec<ChannelInfo>>,
    pub(crate) room: Option<Room>,
    pub(crate) drafts: BTreeMap<String, Draft>,
    /// the banner over the room: the last refusal, until the reader moves on
    pub(crate) notice: String,
    pub(crate) search: Search,
    pub(crate) create: Option<ChannelCreate>,
    pub(crate) details: Option<Details>,
    pub(crate) layout: Layout,
    pub(crate) reads: Reads,
    pub(crate) menu: Option<Menu>,
    pub(crate) copy: Option<CopyRange>,
    pub(crate) preview: Option<Preview>,
    /// the pictures the host decoded for this view, by link: the drawn
    /// size, or (0, 0) for one that stays a file card
    #[serde(skip)]
    pub(crate) pictures: BTreeMap<String, (i64, i64)>,
    #[serde(skip)]
    pub(crate) uploads: HashMap<String, ducktape_view_guest::Task<()>>,
    #[serde(skip)]
    pub(crate) watches: Watches,
    #[serde(skip)]
    pub(crate) timeline_list: RefCell<Option<ducktape_view_guest::ListState>>,
    #[serde(skip)]
    pub(crate) thread_list: RefCell<Option<ducktape_view_guest::ListState>>,
    #[serde(skip)]
    pub(crate) timeline_rows: RefCell<Vec<String>>,
    #[serde(skip)]
    pub(crate) thread_rows: RefCell<Vec<String>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Room {
    pub(crate) id: String,
    pub(crate) messages: Loaded<Vec<MsgRow>>,
    pub(crate) members: Loaded<Vec<MemberRow>>,
    pub(crate) thread: Option<Thread>,
    /// sends the module accepted that the index has not shown yet; drawn
    /// after the fetched rows and dropped once a fetched row carries the id
    #[serde(skip)]
    pub(crate) pending: Vec<MsgRow>,
    pub(crate) has_older: bool,
    #[serde(skip)]
    pub(crate) older_loading: bool,
    /// opened around a landing seq: the window may not reach the head
    pub(crate) landed: bool,
    pub(crate) reaches_head: bool,
    pub(crate) at_tail: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Thread {
    pub(crate) root: u64,
    pub(crate) replies: Loaded<Vec<MsgRow>>,
    pub(crate) has_more: bool,
    pub(crate) next: Option<u64>,
    #[serde(skip)]
    pub(crate) more_loading: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Search {
    pub(crate) draft: String,
    /// the query the hits answer; "" while no search stands
    pub(crate) query: String,
    pub(crate) hits: Loaded<Hits>,
    #[serde(skip)]
    pub(crate) more_loading: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Hits {
    pub(crate) rows: Vec<MsgRow>,
    pub(crate) capped: bool,
    pub(crate) has_more: bool,
    pub(crate) next_after: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ChannelCreate {
    pub(crate) name: String,
    pub(crate) voice: bool,
    pub(crate) members_only: bool,
    pub(crate) error: String,
    #[serde(skip)]
    pub(crate) busy: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Details {
    pub(crate) name_draft: String,
    pub(crate) member_draft: String,
}

#[derive(Serialize, Deserialize)]
pub struct Layout {
    pub(crate) viewport: (f32, f32),
    pub(crate) sidebar: f32,
    pub(crate) details: f32,
    pub(crate) thread: f32,
    /// where the pointer last pressed: a menu opens there
    pub(crate) press: (f32, f32),
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            viewport: (1280., 800.),
            sidebar: 236.,
            details: 320.,
            thread: 330.,
            press: (0., 0.),
        }
    }
}

impl Layout {
    pub(crate) fn clamp(&mut self) {
        let (w, _) = self.viewport;
        self.sidebar = self.sidebar.clamp(180., (w * 0.5).clamp(180., 420.));
        let side = w - self.sidebar - 20. - 320.;
        self.details = self.details.clamp(260., side.clamp(260., 520.));
        self.thread = self.thread.clamp(280., side.clamp(280., 640.));
    }
}

/// What the reader has read: per room, the head seq when she last had it on
/// screen. `boundary` is the cursor the open room was entered with — the
/// unread divider's row is the first message past it.
#[derive(Serialize, Deserialize, Default)]
pub struct Reads {
    pub(crate) cursors: BTreeMap<String, u64>,
    #[serde(skip)]
    pub(crate) visible: bool,
    #[serde(skip)]
    pub(crate) entering: bool,
    pub(crate) boundary: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Timeline,
    Thread,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// chosen: the floating actions stay open
    Toolbar,
    More,
    Reactions,
    Editing,
    Delete,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Menu {
    pub(crate) pane: Pane,
    pub(crate) seq: u64,
    pub(crate) rev: u32,
    pub(crate) mode: Mode,
    pub(crate) at: (f32, f32),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct CopyRange {
    pub(crate) pane: Pane,
    pub(crate) anchor: u64,
    pub(crate) head: u64,
}

impl CopyRange {
    pub(crate) fn holds(&self, pane: Pane, seq: u64) -> bool {
        self.pane == pane && seq >= self.anchor.min(self.head) && seq <= self.anchor.max(self.head)
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Preview {
    pub(crate) link: String,
    #[serde(skip)]
    pub(crate) read: Loaded<files::Preview>,
}

#[derive(Default)]
pub struct Watches {
    pub(crate) props: Option<ducktape_view_guest::Task<()>>,
    pub(crate) changes: Option<ducktape_view_guest::Task<()>>,
    pub(crate) identity: Option<ducktape_view_guest::Task<()>>,
    pub(crate) visible: Option<ducktape_view_guest::Task<()>>,
    pub(crate) drops: Option<ducktape_view_guest::Task<()>>,
}
