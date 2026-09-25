//! The open room: opening, landing, paging history, the thread beside it and
//! what the reader has read.
use chat::{ChannelInfo, MsgRow, Party};
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire;

use crate::composer::Target;
use crate::queries;
use crate::{Chat, PAGE, Pane, Room, Thread, WINDOW, links};

pub(crate) const STREAM_KEY: &str = "chat/room/stream";

impl Chat {
    /// A room the reader chose: opened here, and named to the host so its
    /// tray, links and notices follow.
    pub(crate) fn choose(
        &mut self,
        id: String,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        self.create = None;
        self.search_clear();
        match links::channel_link(&self.session.chain, &id, None) {
            Some(link) => cx.host().open_link(&link),
            None => cx.host().log("no room link: the session names no chain"),
        }
        self.open(id, window, cx);
        // read to the head this view knows now, not at the next list: a
        // reader who opens a room and quits has read it
        if let Some(list) = self.channels.ready().cloned() {
            self.channels_arrived(list, cx);
            self.save_reads(cx);
        }
        self.settle_badge(cx);
    }

    pub(crate) fn open(
        &mut self,
        id: String,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_at(id, 0, window, cx);
    }

    /// The room at its live tail (`land` 0) or around a landing seq.
    pub(crate) fn open_at(
        &mut self,
        id: String,
        land: u64,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        let viewer = self.viewer();
        let same = self.room.as_ref().is_some_and(|room| room.id == id);
        if !same {
            self.reads.boundary = 0;
            self.reads.entering = self.reads.visible;
            self.menu = None;
            self.copy = None;
            self.details = None;
        }
        let room = self.room.get_or_insert_default();
        if !same {
            *room = Room {
                id: id.clone(),
                at_tail: true,
                reaches_head: true,
                ..Room::default()
            };
        }
        room.landed = land > 0;
        room.at_tail = land == 0;
        room.messages = if land > 0 {
            let host = cx.host();
            cx.load(
                async move {
                    let rows = queries::around(host, id, land, viewer).await?;
                    Ok(rows)
                },
                |chat| &mut room_of(chat).messages,
            )
        } else {
            let (has_older_id, viewer2) = (id.clone(), viewer.clone());
            let handle = cx.spawn(async move |this, cx| {
                let host = cx.host();
                let result = queries::roots(host, has_older_id, viewer2, None, WINDOW).await;
                let _ = this.update(cx, |chat, cx| {
                    cx.notify();
                    chat.rows_arrived(result)
                });
            });
            Loaded::Loading(handle)
        };
        let members_id = self.room.as_ref().map(|r| r.id.clone()).unwrap_or_default();
        let room = room_of(self);
        room.members = cx.load(queries::members(cx.host(), members_id), |chat| {
            &mut room_of(chat).members
        });
        if land > 0 {
            self.reveal(STREAM_KEY, land, window, cx);
        }
    }

    /// The newest window landed: the rows, and whether older ones remain.
    fn rows_arrived(&mut self, result: Result<(Vec<MsgRow>, bool), Refusal>) {
        let Some(room) = &mut self.room else { return };
        match result {
            Ok((rows, has_older)) => {
                room.has_older = has_older;
                room.reaches_head = true;
                room.messages = Loaded::Ready(rows);
                room.settle();
            }
            Err(refusal) => room.messages = Loaded::Failed(refusal),
        }
    }

    /// Every change of the chat program: re-read the channel list and
    /// what the open room shows.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reread_channels(cx);
        self.refresh_room(cx);
    }

    pub(crate) fn reread_channels(&mut self, cx: &mut Context<Self>) {
        let list = queries::channels(cx.host());
        cx.refresh(list, |chat, (rooms, more), cx| {
            chat.channels_more = more;
            chat.channels_landed(rooms, cx);
        });
    }

    /// Re-read what the room shows, keeping the rows there until fresh land.
    fn refresh_room(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let (id, viewer) = (room.id.clone(), self.viewer());
        if !room.landed {
            let shown = room
                .messages
                .ready()
                .map_or(WINDOW, |rows| rows.len().max(WINDOW));
            let rows = queries::roots(cx.host(), id.clone(), viewer.clone(), None, shown);
            cx.refresh(rows, |chat, (rows, has_older), _| {
                if let Some(room) = chat.room.as_mut() {
                    room.has_older = has_older;
                    room.messages = Loaded::Ready(rows);
                    room.settle();
                }
            });
        }
        let roster = queries::members(cx.host(), id.clone());
        cx.refresh(roster, |chat, members, _| {
            room_of(chat).members = Loaded::Ready(members);
        });
        if let Some(root) = room.thread.as_ref().map(|thread| thread.root) {
            let replies = queries::thread(cx.host(), id, root, viewer, None);
            cx.refresh(replies, move |chat, (replies, next), _| {
                let Some(thread) = chat.room.as_mut().and_then(|room| room.thread.as_mut()) else {
                    return;
                };
                if thread.root == root {
                    thread.replies = Loaded::Ready(replies);
                    thread.has_more = next.is_some();
                    thread.next = next;
                    room_of(chat).settle();
                }
            });
        }
    }

    /// One older page before the oldest row on screen.
    pub(crate) fn load_older(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let Some(oldest) = room
            .messages
            .ready()
            .and_then(|rows| rows.first())
            .map(|r| r.seq)
        else {
            return;
        };
        if !room.has_older || room.older_loading || room.landed {
            return;
        }
        room.older_loading = true;
        let id = room.id.clone();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let below = chat::roots_below(&id, oldest);
            let result = queries::roots(host, id, viewer, Some(below), PAGE).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let Some(room) = &mut chat.room else { return };
                room.older_loading = false;
                match result {
                    Ok((older, has_older)) => {
                        room.has_older = has_older;
                        if let Some(rows) = room.messages.ready_mut() {
                            let mut all = older;
                            all.append(rows);
                            *rows = all;
                        }
                    }
                    Err(refusal) => {
                        chat.notice = format!("Couldn’t read this room: {}", refusal.sentence)
                    }
                }
            });
        })
        .detach();
    }

    /// Settled native list geometry drives paging and tail state. Wheel deltas
    /// are intentionally not used: remeasurement and programmatic scrolling
    /// can move the viewport without one.
    pub(crate) fn list_scrolled(
        &mut self,
        pane: Pane,
        event: &ducktape_view_guest::ListScrollEvent,
        cx: &mut Context<Self>,
    ) {
        match pane {
            Pane::Timeline => {
                let Some(room) = &mut self.room else { return };
                room.at_tail = event.is_following_tail || event.visible_range.end >= event.count;
                if event.visible_range.start <= 4 {
                    self.load_older(cx);
                }
            }
            Pane::Thread => {
                if event.visible_range.end.saturating_add(4) >= event.count {
                    self.load_more_replies(cx);
                }
            }
        }
    }

    pub(crate) fn open_thread(&mut self, root: u64, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        self.details = None;
        self.menu = None;
        self.copy = None;
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let thread = room.thread.get_or_insert_default();
        *thread = Thread {
            root,
            ..Thread::default()
        };
        let handle = cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = queries::thread(host, id.clone(), root, viewer, None).await;
            let _ = this.update_in(cx, |chat, window, cx| {
                cx.notify();
                let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                    return;
                };
                if thread.root != root {
                    return;
                }
                match result {
                    Ok((replies, next)) => {
                        thread.replies = Loaded::Ready(replies);
                        thread.has_more = next.is_some();
                        thread.next = next;
                        room_of(chat).settle();
                        // the reply field takes the keys once it can be typed in
                        let key = Target::Post {
                            channel: id,
                            thread: Some(root),
                        }
                        .key();
                        window.focus(ducktape_view_guest::ElementId::Name(
                            format!("{key}/editor").into(),
                        ));
                    }
                    Err(refusal) => thread.replies = Loaded::Failed(refusal),
                }
            });
        });
        let Some(thread) = self.room.as_mut().and_then(|r| r.thread.as_mut()) else {
            return;
        };
        thread.replies = Loaded::Loading(handle);
    }

    pub(crate) fn load_more_replies(&mut self, cx: &mut Context<Self>) {
        let viewer = self.viewer();
        let Some(room) = &mut self.room else { return };
        let id = room.id.clone();
        let Some(thread) = &mut room.thread else {
            return;
        };
        if !thread.has_more || thread.more_loading {
            return;
        }
        thread.more_loading = true;
        let (root, after) = (thread.root, thread.next.clone());
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = queries::thread(host, id, root, viewer, after).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                    return;
                };
                thread.more_loading = false;
                match result {
                    Ok(_) if thread.root != root => {}
                    Ok((more, next)) => {
                        thread.has_more = next.is_some();
                        thread.next = next;
                        if let Some(rows) = thread.replies.ready_mut() {
                            rows.extend(more);
                        }
                    }
                    Err(refusal) => {
                        chat.notice = format!("Couldn’t read this thread: {}", refusal.sentence)
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn close_thread(&mut self) {
        if let Some(room) = &mut self.room {
            room.thread = None;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.pane == Pane::Thread)
        {
            self.menu = None;
        }
        if self.copy.is_some_and(|copy| copy.pane == Pane::Thread) {
            self.copy = None;
        }
    }

    /// The channel list landed: the rooms, and what each one's head says
    /// about what the reader has read. The room on screen is read to its
    /// head, and the host's notices for it with it.
    pub(crate) fn channels_arrived(&mut self, channels: Vec<ChannelInfo>, cx: &mut Context<Self>) {
        let reading = self
            .room
            .as_ref()
            .filter(|room| self.reads.visible && !room.landed)
            .map(|room| room.id.clone());
        let mut read = None;
        for info in &channels {
            let cursor = self
                .reads
                .cursors
                .entry(info.channel.id.clone())
                .or_insert(info.head_seq);
            if reading.as_deref() == Some(info.channel.id.as_str()) {
                if self.reads.entering && info.head_seq > *cursor {
                    self.reads.boundary = *cursor;
                }
                self.reads.entering = false;
                if info.head_seq > *cursor {
                    *cursor = info.head_seq;
                    read = Some(info.channel.id.clone());
                }
            }
        }
        // a room gone from the list takes its cursor with it: the kept map
        // is written whole, and must not grow with every room ever seen. A
        // list cut at its page budget has not seen them all: it keeps them.
        let listed: std::collections::HashSet<&str> = channels
            .iter()
            .map(|info| info.channel.id.as_str())
            .collect();
        if !self.channels_more {
            self.reads
                .cursors
                .retain(|room, _| listed.contains(room.as_str()));
        }
        self.channels = Loaded::Ready(channels);
        if let Some(room) = read {
            self.read_notices(&room, cx);
        }
    }

    pub(crate) fn unread(&self, info: &ChannelInfo) -> bool {
        self.reads
            .cursors
            .get(&info.channel.id)
            .is_some_and(|cursor| info.head_seq > *cursor)
    }

    pub(crate) fn room_id(&self) -> String {
        self.room
            .as_ref()
            .map(|room| room.id.clone())
            .unwrap_or_default()
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

    /// The open room's members, as the roster names them.
    pub(crate) fn roster(&self) -> Vec<(Party, String)> {
        let (Some(names), Some(members)) = (
            self.names.ready(),
            self.room.as_ref().and_then(|room| room.members.ready()),
        ) else {
            return Vec::new();
        };
        members
            .iter()
            .map(|row| (row.party.clone(), names.member(&row.party)))
            .collect()
    }

    /// Scroll a stream to the row with `seq`.
    pub(crate) fn reveal(
        &mut self,
        target: &str,
        seq: u64,
        window: &mut ducktape_view_guest::Window,
        _cx: &mut Context<Self>,
    ) {
        window.dispatch(wire::WidgetCommand::ScrollToKey {
            target: vec![wire::ElementIdWire::Name(target.to_owned().into())],
            key: wire::ListKey::from(seq as i64).virtual_key(),
        });
    }
}

impl Room {
    /// Fresh rows landed: a pending send the index now serves leaves.
    pub(crate) fn settle(&mut self) {
        let shown = |id: &str, rows: Option<&Vec<MsgRow>>| {
            rows.is_some_and(|rows| rows.iter().any(|row| row.message_id == id))
        };
        let replies = self.thread.as_ref().and_then(|t| t.replies.ready());
        let messages = self.messages.ready();
        self.pending
            .retain(|p| !shown(&p.message_id, messages) && !shown(&p.message_id, replies));
    }
}

pub(crate) fn room_of(chat: &mut Chat) -> &mut Room {
    chat.room.get_or_insert_default()
}
