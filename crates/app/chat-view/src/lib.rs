//! Chat: channels, direct messages, threads, search and the room's huddle, on
//! the view-guest `View` shape. State here as one struct per pane, handlers as
//! closures the frame registers, module data read through the module's wire
//! (`chat`, a local copy of the slice this view speaks) and folded to rows at
//! render time. `render` never mutates: what an event changes lands in the
//! handlers of `room`, `actions` and `compose`.
mod actions;
mod api;
mod chat;
mod client;
mod compose;
mod composer;
mod emoji;
mod kept;
mod notices;
mod queries;
mod room;
mod state;
use queries::{around, channels, members, resolve_me, roots, roster, search_hits, thread};
pub use state::*;
mod ui;

use chat::{
    ChannelInfo, ChatMsg, ChatViewQuery, ChatViewReply, MemberRow, MessageHits, MsgRow, PostPolicy,
};
use client::{NameDirectory, mention_token};
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::{Loaded, View};
use ducktape_view_guest::{IntoElement, Render, Window, export_view};
use futures::StreamExt;

use api::{ChatApi, HostId, HostProps, HostRoute, HostVisible, RpcLive, Session, Submit};
use composer::Draft;
use composer::Target;

const PAGE: usize = 64;
const WINDOW: usize = 256;

impl View for Chat {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180,760";

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
        let mut props = cx.host().subscribe::<HostProps>(());
        self.watches.props = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = props.next().await {
                if this
                    .update_in(cx, |chat, window, cx| {
                        cx.notify();
                        match item {
                            Ok(next) => chat.session_changed(next, window, cx),
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
        let mut changes = cx.host().subscribe::<RpcLive>(::chat::PROGRAM.into());
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
        // `duck://<chain>/chat/<channel>[/<seq>]`: a link opened into this
        // view (a notice's, say) names the room, and the message to land on
        let mut routes = cx.host().subscribe::<HostRoute>(());
        self.watches.route = Some(cx.spawn(async move |this, cx| {
            while let Some(Ok(route)) = routes.next().await {
                let Some((channel, seq)) = chat::route_target(&route) else {
                    continue;
                };
                if this
                    .update_in(cx, |chat, window, cx| {
                        cx.notify();
                        chat.search_clear();
                        chat.open_at(channel, seq, window, cx);
                        chat.settle_badge(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut visible = cx.host().subscribe::<HostVisible>(());
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
        // identity is program-agnostic: a key that gains an account while
        // this view is open (Settings, then back to Chat) writes no session
        // change of its own, only an identity block. Re-resolve on it too —
        // and re-read the roster, or a name ANOTHER signer claims while this
        // room stays open never replaces the "account N" fallback their
        // messages render under (they show up, just unnamed).
        let mut identity_live = cx.host().subscribe::<RpcLive>(identity::PROGRAM.into());
        self.watches.identity = Some(cx.spawn(async move |this, cx| {
            while identity_live.next().await.is_some() {
                if this
                    .update(cx, |chat, cx| {
                        cx.notify();
                        chat.names = cx.load(roster(cx.host()), |chat| &mut chat.names);
                        chat.refresh_me(cx);
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
        if self.me.is_idle() {
            self.refresh_me(cx);
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
    }
}

impl Render for Chat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

impl Chat {
    fn session_changed(
        &mut self,
        next: Session,
        _window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.account != prev.account
            || self.session.endpoint != prev.endpoint
            || self.session.chain != prev.chain;
        if reader_changed {
            self.names = cx.load(roster(cx.host()), |chat| &mut chat.names);
            self.refresh_me(cx);
        }
        if reader_changed || (prev.connected && !self.session.connected) {
            for draft in self.drafts.values_mut() {
                draft.retire_device_requests();
            }
            self.reads.cursors.clear();
            self.reads.kept = None;
            self.create = None;
        }
        if self.session.connected && (reader_changed || !prev.connected) {
            self.load_kept(cx);
        }
        if !prev.connected && self.session.connected {
            self.channels = cx.load(channels(cx.host()), |chat| &mut chat.channels);
            self.refresh(cx);
        }
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
            cx.refresh(channels(cx.host()), |chat, list, cx| {
                chat.channels_landed(list, cx)
            });
        }
    }

    /// Every state change of the chat module: re-read what is on screen,
    /// keeping the rows already there until the fresh ones land.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.refresh(channels(cx.host()), |chat, list, cx| {
            chat.channels_landed(list, cx)
        });
        self.refresh_room(cx);
    }

    pub(crate) fn viewer(&self) -> Vec<String> {
        let me = self.my_handle();
        if me.is_empty() { Vec::new() } else { vec![me] }
    }

    /// Re-asks identity for the account the seated key holds now. Called on
    /// every key change and on identity's own live stream, so a key that
    /// gains an account while this view stays open (Settings, then back to
    /// Chat) re-enables writes without a relaunch.
    fn refresh_me(&mut self, cx: &mut Context<Self>) {
        let key = self.session.account.clone();
        self.me = cx.load(resolve_me(cx.host(), key), |chat| &mut chat.me);
    }

    /// The reader's account number, once identity has answered.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.me.ready().copied().flatten()
    }

    /// The reader's handle as chat itself would write it: `acct:<n>` once
    /// the seated key holds an account, `user:<hex>` while it is seated but
    /// holds none, "" with no key seated at all.
    pub(crate) fn my_handle(&self) -> String {
        match self.my_account() {
            Some(number) => format!("acct:{number}"),
            None if self.session.account.is_empty() => String::new(),
            None => format!("user:{}", self.session.account),
        }
    }

    /// Every write in chat is authored by an account: a key that holds none
    /// reads and nothing more.
    pub(crate) fn holds_account(&self) -> bool {
        self.my_account().is_some()
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
        if !self.holds_account() {
            return "no_account";
        }
        let Some(info) = self.room_info() else {
            return "";
        };
        if info.channel.archived {
            return "channel_archived";
        }
        if crate::chat::members_only(info) {
            let me = self.my_handle();
            let seated = self
                .room
                .as_ref()
                .and_then(|room| room.members.ready())
                .is_some_and(|members| members.iter().any(|m| m.party == me));
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
        if !self.holds_account() {
            return;
        }
        let Some(create) = &mut self.create else {
            return;
        };
        if create.busy || !self.session.connected {
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
                let channel_id = host.ask::<HostId>("channel".into()).await?;
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
                        cx.refresh(channels(cx.host()), |chat, list, cx| {
                            chat.channels_landed(list, cx)
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
    ["rpc", "op", "host", "fs", "clipboard", "notify", "store"]
);

#[cfg(test)]
mod tests;
