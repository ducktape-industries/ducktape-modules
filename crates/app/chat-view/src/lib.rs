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
mod queries;
mod room;
mod state;
use queries::{around, channels, members, roots, roster, search_hits, thread};
pub use state::*;
mod ui;

use chat::{
    ChannelInfo, ChatMsg, ChatViewQuery, ChatViewReply, MemberRow, MessageHits, MsgRow, PostPolicy,
    TagPage,
};
use client::{NameDirectory, mention_token};
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::{Live as LiveChanges, Loaded, Submit, View, ViewOf, Visible};
use ducktape_view_guest::{Render, Window, export_view, wire};
use futures::StreamExt;

use api::{ChatApi, Id, Props, PropsItem, Session};
use composer::Draft;
use composer::Target;

const PAGE: usize = 64;
/// Attachments speak to the files module, which is not in this tree yet:
/// every way a file gets in is closed until it returns. The code stays.
pub(crate) const ATTACHMENTS: bool = false;
const WINDOW: usize = 256;

impl View for Chat {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180x760";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut chat = Self::default();
        chat.restored(window, cx);
        chat
    }

    fn restored(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // a send that was in flight when the snapshot was taken never came
        // back: park its body as a failed send the composer can restore.
        for draft in self.drafts.values_mut() {
            draft.retire_device_requests();
        }
        self.menu = None;
        let mut props = cx.host().subscribe::<Props>(());
        self.watches.props = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = props.next().await {
                if this
                    .update_in(cx, |chat, window, cx| {
                        cx.notify();
                        match item {
                            Ok(PropsItem::Session(next)) => chat.session_changed(*next, window, cx),
                            Ok(PropsItem::Background { background }) => {
                                let names = chat.names.ready().cloned().unwrap_or_default();
                                cx.spawn(async move |_, cx| {
                                    background::run(cx.host(), background, names).await;
                                })
                                .detach();
                            }
                            Err(refusal) => {
                                chat.notice =
                                    format!("Couldn’t read the session: {}", refusal.sentence)
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut changes = cx.host().subscribe::<LiveChanges>("chat".into());
        self.watches.changes = Some(cx.spawn(async move |this, cx| {
            while let Some(_item) = changes.next().await {
                if this
                    .update(cx, |chat, cx| {
                        cx.notify();
                        chat.refresh(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut visible = cx.host().subscribe::<Visible>(());
        self.watches.visible = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = visible.next().await {
                if this
                    .update(cx, |chat, cx| {
                        if let Ok(visible) = item {
                            cx.notify();
                            chat.visibility_changed(visible, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        if self.names.is_idle() {
            self.names = cx.load(roster(cx.host()), |chat| &mut chat.names);
        }
        if self.channels.is_idle() {
            self.channels = cx.load(channels(cx.host()), |chat| &mut chat.channels);
        }
        if let Some(room) = &self.room {
            let (id, thread) = (room.id.clone(), room.thread.as_ref().map(|t| t.root));
            self.open(id, window, cx);
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
}

impl Render for Chat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> wire::Node {
        wire::kit::set_dark(self.session.dark);
        ui::render(self, cx)
    }
}

impl Chat {
    fn session_changed(
        &mut self,
        next: Session,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.me != prev.me
            || self.session.endpoint != prev.endpoint
            || self.session.chain != prev.chain;
        if self.session.names_serial != prev.names_serial || reader_changed {
            self.names = cx.load(roster(cx.host()), |chat| &mut chat.names);
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
            self.channels = cx.load(channels(cx.host()), |chat| &mut chat.channels);
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
            self.open_at(active.clone(), land, window, cx);
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

    fn visibility_changed(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !visible {
            self.create = None;
        }
        if self.reads.visible == visible {
            return;
        }
        self.reads.visible = visible;
        self.reads.entering = visible;
        if visible && self.session.connected {
            cx.refresh(channels(cx.host()), |chat, list, _| {
                chat.channels_arrived(list)
            });
        }
    }

    /// Every state change of the chat module: re-read what is on screen,
    /// keeping the rows already there until the fresh ones land.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.refresh(channels(cx.host()), |chat, list, _| {
            chat.channels_arrived(list)
        });
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
    pub(crate) fn submit(&mut self, op: ChatMsg, cx: &mut Context<Self>) {
        self.notice.clear();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = host.ask::<Submit<ChatApi>>(op).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                match result {
                    Ok(_) => chat.refresh(cx),
                    Err(refusal) => {
                        chat.notice = format!("That didn’t go through: {}", refusal.sentence)
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn create_channel(&mut self, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = async {
                let channel_id = host.ask::<Id>("channel".into()).await?;
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
                host.ask::<Submit<ChatApi>>(op).await?;
                Ok::<_, Refusal>(channel_id)
            }
            .await;
            let _ = this.update_in(cx, |chat, window, cx| {
                cx.notify();
                match result {
                    Ok(id) => {
                        chat.create = None;
                        cx.refresh(channels(cx.host()), |chat, list, _| {
                            chat.channels_arrived(list)
                        });
                        if !voice {
                            chat.choose(id, window, cx);
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
        })
        .detach();
    }

    /// A direct message with `peer` (an account number): the derived room,
    /// created when the module has none yet.
    pub(crate) fn open_dm(&mut self, peer: &str, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = async {
                let existing = host
                    .ask::<ViewOf<ChatApi>>(ChatViewQuery::Channel {
                        channel_id: id.clone(),
                    })
                    .await?;
                if !matches!(existing, ChatViewReply::Channel(Some(_))) {
                    host.ask::<Submit<ChatApi>>(ChatMsg::CreateDmChannel {
                        counterpart: peer,
                        name,
                    })
                    .await?;
                }
                Ok::<_, Refusal>(id)
            }
            .await;
            let _ = this.update_in(cx, |chat, window, cx| {
                cx.notify();
                match result {
                    Ok(id) => {
                        cx.refresh(channels(cx.host()), |chat, list, _| {
                            chat.channels_arrived(list)
                        });
                        chat.choose(id, window, cx);
                    }
                    Err(refusal) => {
                        chat.notice =
                            format!("Couldn’t open this conversation: {}", refusal.sentence)
                    }
                }
            });
        })
        .detach();
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

export_view!(
    Chat,
    "Chat",
    "Channels, direct messages, threads, search and the live call of this workspace.",
    ["chat"]
);

#[cfg(test)]
mod tests;
