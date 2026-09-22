//! Chat: channels, direct messages, threads, search and the room's huddle, on
//! the view-guest `View` shape. State here as one struct per pane, handlers as
//! closures the frame registers, module data read through the module's wire
//! (`chat`, a local copy of the slice this view speaks) and folded to rows at
//! render time. `render` never mutates: what an event changes lands in the
//! handlers of `room`, `actions` and `compose`.
mod actions;
mod api;
mod background;
mod chat;
mod client;
mod compose;
mod composer;
mod files;
mod room;
mod ui;

use std::collections::{BTreeMap, HashMap};

use chat::{
    ChannelInfo, ChatMsg, ChatViewQuery, ChatViewReply, MemberRow, MessageHits, MsgRow, PostPolicy,
    TagPage,
};
use client::{NameDirectory, mention_token};
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{
    Cx, Live as LiveChanges, Loaded, Submit, View, ViewOf, Visible, Watching, ask,
};
use ducktape_view_guest::{export_view, wire};
use serde::{Deserialize, Serialize};

use api::{ChatApi, Id, Props, PropsItem, Session};
use composer::Draft;
use composer::Target;

const PAGE: usize = 64;
/// Attachments speak to the files module, which is not in this tree yet:
/// every way a file gets in is closed until it returns. The code stays.
pub(crate) const ATTACHMENTS: bool = false;
const WINDOW: usize = 256;

#[derive(Serialize, Deserialize, Default)]
pub struct Chat {
    pub(crate) session: Session,
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
    pub(crate) uploads: HashMap<String, wire::task::Handle>,
    #[serde(skip)]
    pub(crate) watches: Watches,
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
    props: Option<Watching>,
    changes: Option<Watching>,
    visible: Option<Watching>,
    pub(crate) drops: Option<Watching>,
}

impl View for Chat {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180,760";

    fn boot(cx: &mut Cx<Self>) -> Self {
        let mut chat = Self::default();
        chat.restored(cx);
        chat
    }

    fn restored(&mut self, cx: &mut Cx<Self>) {
        // a send that was in flight when the snapshot was taken never came
        // back: park its body as a failed send the composer can restore.
        for draft in self.drafts.values_mut() {
            draft.retire_device_requests();
        }
        self.menu = None;
        self.watches.props = Some(cx.watch::<Props>((), |chat, item, cx| match item {
            Ok(PropsItem::Session(next)) => chat.session_changed(*next, cx),
            Ok(PropsItem::Background { background }) => {
                let names = chat.names.ready().cloned().unwrap_or_default();
                cx.spawn(async move {
                    background::run(background, names).await;
                    |_: &mut Chat, _: &mut Cx<Chat>| {}
                });
            }
            Err(refusal) => {
                chat.notice = format!("Couldn’t read the session: {}", refusal.sentence)
            }
        }));
        self.watches.changes =
            Some(cx.watch::<LiveChanges>("chat".into(), |chat, _, cx| chat.refresh(cx)));
        self.watches.visible = Some(cx.watch::<Visible>((), |chat, item, cx| {
            if let Ok(visible) = item {
                chat.visibility_changed(visible, cx);
            }
        }));
        if self.names.is_idle() {
            self.names = cx.load(roster(), |chat| &mut chat.names);
        }
        if self.channels.is_idle() {
            self.channels = cx.load(channels(), |chat| &mut chat.channels);
        }
        if let Some(room) = &self.room {
            let (id, thread) = (room.id.clone(), room.thread.as_ref().map(|t| t.root));
            self.open(id, cx);
            if let Some(root) = thread {
                self.open_thread(root, cx);
            }
        }
        if !self.search.query.is_empty() {
            self.search_now(cx);
        }
        self.preview_read(cx);
        self.watch_drops(cx);
    }

    fn render(&mut self, cx: &mut Cx<Self>) -> wire::Node {
        wire::kit::set_dark(self.session.dark);
        ui::render(self, cx)
    }
}

impl Chat {
    fn session_changed(&mut self, next: Session, cx: &mut Cx<Self>) {
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.me != prev.me
            || self.session.endpoint != prev.endpoint
            || self.session.chain != prev.chain;
        if self.session.names_serial != prev.names_serial || reader_changed {
            self.names = cx.load(roster(), |chat| &mut chat.names);
        }
        if reader_changed || (prev.connected && !self.session.connected) {
            self.uploads.clear();
            for draft in self.drafts.values_mut() {
                draft.retire_device_requests();
            }
            self.reads.cursors.clear();
            self.create = None;
        }
        if !prev.connected && self.session.connected {
            self.channels = cx.load(channels(), |chat| &mut chat.channels);
            self.refresh(cx);
        }
        let active = &self.session.active_channel;
        let steered = !active.is_empty()
            && (*active != prev.active_channel || self.session.land_seq != prev.land_seq);
        let already = self
            .room
            .as_ref()
            .is_some_and(|room| room.id == *active && (self.session.land_seq == 0) != room.landed);
        if steered && !already {
            let land = u64::try_from(self.session.land_seq).unwrap_or(0);
            self.open_at(active.clone(), land, cx);
        }
        if self.session.dm_serial != prev.dm_serial && !self.session.dm_peer.is_empty() {
            let peer = self.session.dm_peer.clone();
            self.open_dm(&peer, cx);
        }
        if self.session.copy_chord_serial != prev.copy_chord_serial {
            self.copy_range(cx);
        }
        self.watch_drops(cx);
    }

    fn visibility_changed(&mut self, visible: bool, cx: &mut Cx<Self>) {
        if !visible {
            self.create = None;
        }
        if self.reads.visible == visible {
            return;
        }
        self.reads.visible = visible;
        self.reads.entering = visible;
        if visible && self.session.connected {
            cx.refresh(channels(), |chat, list, _| chat.channels_arrived(list));
        }
    }

    /// Every state change of the chat module: re-read what is on screen,
    /// keeping the rows already there until the fresh ones land.
    fn refresh(&mut self, cx: &mut Cx<Self>) {
        cx.refresh(channels(), |chat, list, _| chat.channels_arrived(list));
        self.refresh_room(cx);
    }

    pub(crate) fn viewer(&self) -> Vec<String> {
        let me = &self.session.me;
        if me.is_empty() {
            Vec::new()
        } else {
            vec![me.clone()]
        }
    }

    pub(crate) fn me_key(&self) -> Vec<u8> {
        chat::unhex(&self.session.me_key).unwrap_or_default()
    }

    /// The reader's account number, when the key holds one.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.session.me.strip_prefix("acct:")?.parse().ok()
    }

    pub(crate) fn info(&self, id: &str) -> Option<&ChannelInfo> {
        self.channels
            .ready()?
            .iter()
            .find(|info| info.channel.id == id)
    }

    pub(crate) fn room_info(&self) -> Option<&ChannelInfo> {
        self.info(&self.room.as_ref()?.id)
    }

    pub(crate) fn members(&self) -> Vec<client::ChatMember> {
        let Some(names) = self.names.ready() else {
            return Vec::new();
        };
        self.room
            .as_ref()
            .and_then(|room| room.members.ready())
            .map(|rows| {
                rows.iter()
                    .map(|row| client::ChatMember {
                        key: row.party.clone(),
                        label: names.member_label(&row.party),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Why the reader may not write here, as a reason token — "" when she
    /// may. The account comes first: with none, every write is refused.
    pub(crate) fn write_refusal(&self) -> &'static str {
        if !self.session.holds_account() {
            return "no_account";
        }
        let Some(info) = self.room_info() else {
            return "";
        };
        if info.channel.archived {
            return "channel_archived";
        }
        if crate::chat::members_only(info) {
            let me = &self.session.me;
            let seated = self
                .room
                .as_ref()
                .and_then(|room| room.members.ready())
                .is_some_and(|members| {
                    members
                        .iter()
                        .any(|m| m.party == *me || format!("user:{}", m.party) == *me)
                });
            if !seated {
                return "members_only";
            }
        }
        ""
    }

    pub(crate) fn may_write(&self) -> bool {
        self.write_refusal().is_empty()
    }

    pub(crate) fn mention_choices(&self) -> Vec<composer::MentionChoice> {
        let Some(names) = self.names.ready() else {
            return Vec::new();
        };
        client::mention_choices(names, &self.members())
            .into_iter()
            .map(|choice| composer::MentionChoice {
                token: mention_token(&choice.party),
                label: choice.label,
            })
            .collect()
    }

    /// One op to the chat module; a refusal lands in the banner.
    pub(crate) fn submit(&mut self, op: ChatMsg, cx: &mut Cx<Self>) {
        self.notice.clear();
        cx.spawn(async move {
            let result = ask::<Submit<ChatApi>>(op).await;
            move |chat: &mut Chat, cx: &mut Cx<Chat>| match result {
                Ok(_) => chat.refresh(cx),
                Err(refusal) => {
                    chat.notice = format!("That didn’t go through: {}", refusal.sentence)
                }
            }
        });
    }

    pub(crate) fn create_channel(&mut self, cx: &mut Cx<Self>) {
        let Some(create) = &mut self.create else {
            return;
        };
        if create.busy || !self.session.connected || self.session.busy {
            return;
        }
        let name = create.name.trim().to_string();
        if name.is_empty() || name.len() > 128 || name.contains('\0') {
            create.error = "Enter a channel name of at most 128 bytes".into();
            return;
        }
        create.error.clear();
        create.busy = true;
        let (voice, members_only) = (create.voice, create.members_only);
        cx.spawn(async move {
            let result = async {
                let channel_id = ask::<Id>("channel").await?;
                let op = if voice {
                    ChatMsg::CreateVoiceChannel {
                        channel_id: channel_id.clone(),
                        name,
                    }
                } else {
                    ChatMsg::CreateChannel {
                        channel_id: channel_id.clone(),
                        name,
                        post_policy: if members_only {
                            PostPolicy::MembersOnly
                        } else {
                            PostPolicy::Open
                        },
                    }
                };
                ask::<Submit<ChatApi>>(op).await?;
                Ok::<_, Refusal>(channel_id)
            }
            .await;
            move |chat: &mut Chat, cx: &mut Cx<Chat>| match result {
                Ok(id) => {
                    chat.create = None;
                    cx.refresh(channels(), |chat, list, _| chat.channels_arrived(list));
                    if !voice {
                        chat.choose(id, cx);
                    }
                }
                Err(refusal) => {
                    if let Some(create) = &mut chat.create {
                        create.busy = false;
                        create.error =
                            format!("Couldn’t create this channel: {}", refusal.sentence);
                    }
                }
            }
        });
    }

    /// A direct message with `peer` (an account number): the derived room,
    /// created when the module has none yet.
    pub(crate) fn open_dm(&mut self, peer: &str, cx: &mut Cx<Self>) {
        let Some(mine) = self.my_account() else {
            self.notice = "This key is on no account; a DM needs one".into();
            return;
        };
        let Ok(peer) = peer.trim().parse::<u64>() else {
            self.notice = "A DM peer is an account number".into();
            return;
        };
        let name = self.names.ready().map_or_else(
            || format!("acct:{peer}"),
            |names| names.member_label(&format!("acct:{peer}")),
        );
        let id = chat::dm_channel_id(mine, peer);
        self.notice.clear();
        cx.spawn(async move {
            let result = async {
                let existing = ask::<ViewOf<ChatApi>>(ChatViewQuery::Channel {
                    channel_id: id.clone(),
                })
                .await?;
                if !matches!(existing, ChatViewReply::Channel(Some(_))) {
                    ask::<Submit<ChatApi>>(ChatMsg::CreateDmChannel {
                        counterpart: peer,
                        name,
                    })
                    .await?;
                }
                Ok::<_, Refusal>(id)
            }
            .await;
            move |chat: &mut Chat, cx: &mut Cx<Chat>| match result {
                Ok(id) => {
                    cx.refresh(channels(), |chat, list, _| chat.channels_arrived(list));
                    chat.choose(id, cx);
                }
                Err(refusal) => {
                    chat.notice = format!("Couldn’t open this conversation: {}", refusal.sentence)
                }
            }
        });
    }
}

pub(crate) fn draft_key(target: &Target) -> String {
    match target {
        Target::Post {
            channel,
            thread: None,
        } => format!("draft-{channel}"),
        Target::Post {
            channel,
            thread: Some(root),
        } => format!("draft-{channel}-{root}"),
        Target::Edit { channel, seq, .. } => format!("edit-{channel}-{seq}"),
    }
}

fn wrong_reply() -> Refusal {
    malformed("the chat module answered another question".into())
}

pub(crate) async fn channels() -> Result<Vec<ChannelInfo>, Refusal> {
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let ChatViewReply::Channels {
            channels,
            has_more,
            next_after,
        } = ask::<ViewOf<ChatApi>>(ChatViewQuery::Channels {
            after,
            limit: Some(PAGE),
        })
        .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(channels);
        if !has_more || next_after.is_none() {
            return Ok(all);
        }
        after = next_after;
    }
}

/// One page of roots older than `before` (or the newest), oldest first,
/// with whether older ones remain.
pub(crate) async fn roots(
    channel_id: String,
    viewer: Vec<String>,
    before: Option<u64>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Refusal> {
    let mut all = Vec::new();
    let mut before_seq = before;
    loop {
        let ChatViewReply::Roots {
            roots,
            has_more,
            next_before_seq,
        } = ask::<ViewOf<ChatApi>>(ChatViewQuery::Roots {
            channel_id: channel_id.clone(),
            viewer_handles: viewer.clone(),
            before_seq,
            limit: Some(PAGE),
        })
        .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(roots);
        if !has_more || next_before_seq.is_none() || all.len() >= limit {
            return Ok((sorted(all), has_more && next_before_seq.is_some()));
        }
        before_seq = next_before_seq;
    }
}

/// The rows around a landing seq, oldest first.
pub(crate) async fn around(
    channel_id: String,
    seq: u64,
    viewer: Vec<String>,
) -> Result<Vec<MsgRow>, Refusal> {
    match ask::<ViewOf<ChatApi>>(ChatViewQuery::MessagesAround {
        channel_id,
        seq,
        viewer_handles: viewer,
        limit: Some(WINDOW / 2),
    })
    .await?
    {
        ChatViewReply::Messages(rows) => Ok(sorted(rows)),
        _ => Err(wrong_reply()),
    }
}

pub(crate) fn sorted(mut rows: Vec<MsgRow>) -> Vec<MsgRow> {
    rows.sort_by_key(|row| row.seq);
    rows
}

pub(crate) async fn members(channel_id: String) -> Result<Vec<MemberRow>, Refusal> {
    match ask::<ViewOf<ChatApi>>(ChatViewQuery::Members {
        channel_id,
        after: None,
        limit: Some(WINDOW),
    })
    .await?
    {
        ChatViewReply::Members { members, .. } => Ok(members),
        _ => Err(wrong_reply()),
    }
}

/// One page of a thread's replies after `after`, and how to page on.
pub(crate) async fn thread(
    channel_id: String,
    root_seq: u64,
    viewer: Vec<String>,
    after: Option<u64>,
) -> Result<(Vec<MsgRow>, bool, Option<u64>), Refusal> {
    match ask::<ViewOf<ChatApi>>(ChatViewQuery::Thread {
        channel_id,
        root_seq,
        viewer_handles: viewer,
        after_reply_seq: after,
        limit: Some(WINDOW),
    })
    .await?
    {
        ChatViewReply::Thread {
            replies,
            has_more,
            next_reply_seq,
            ..
        } => Ok((sorted(replies), has_more, next_reply_seq)),
        _ => Err(wrong_reply()),
    }
}

/// A search: `#tag` pages through the tag index, anything else is a
/// full-text search capped by the module.
pub(crate) async fn search_hits(
    text: String,
    channel_id: Option<String>,
    viewer: Vec<String>,
    after: Option<String>,
) -> Result<(Vec<MsgRow>, bool, bool, Option<String>), Refusal> {
    let query = match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => ChatViewQuery::TagSearch {
            tag: tag.to_owned(),
            viewer_handles: viewer,
            channel_id,
            after,
            limit: Some(PAGE),
        },
        _ => ChatViewQuery::Search {
            text,
            viewer_handles: viewer,
            channel_id,
            limit: Some(PAGE),
        },
    };
    match ask::<ViewOf<ChatApi>>(query).await? {
        ChatViewReply::Hits(MessageHits { hits, capped }) => Ok((hits, capped, false, None)),
        ChatViewReply::TagHits(TagPage {
            hits,
            has_more,
            next_after,
        }) => Ok((hits, false, has_more, next_after)),
        _ => Err(wrong_reply()),
    }
}

/// The identity roster, paged through chat, folded into the name directory.
async fn roster() -> Result<NameDirectory, Refusal> {
    match ask::<ViewOf<ChatApi>>(ChatViewQuery::Accounts { limit: Some(256) }).await? {
        ChatViewReply::Accounts(accounts) => Ok(NameDirectory::from_roster(accounts)),
        _ => Err(wrong_reply()),
    }
}

export_view!(
    Chat,
    "Chat",
    "Channels, direct messages, threads, search and the live call of this workspace.",
    ["chat"]
);

#[cfg(test)]
mod tests;
