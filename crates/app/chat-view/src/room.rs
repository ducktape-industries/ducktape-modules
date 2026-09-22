//! The open room: opening, landing, paging history, the thread beside it and
//! what the reader has read.
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire;

use crate::chat::{ChannelInfo, MsgRow};
use crate::{Chat, Room, Thread, WINDOW};

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
        let link = crate::files::channel_link(&self.session.chain, &id, None);
        if !link.is_empty() {
            cx.host().open_link(&link);
        }
        self.open(id, window, cx);
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
                    let rows = crate::around(host, id, land, viewer).await?;
                    Ok(rows)
                },
                |chat| &mut room_of(chat).messages,
            )
        } else {
            let (has_older_id, viewer2) = (id.clone(), viewer.clone());
            let handle = cx.spawn(async move |this, cx| {
                let host = cx.host();
                let result = crate::roots(host, has_older_id, viewer2, None, WINDOW).await;
                let _ = this.update(cx, |chat, cx| {
                    cx.notify();
                    chat.rows_arrived(result, cx)
                });
            });
            Loaded::Loading(handle)
        };
        let members_id = self.room.as_ref().map(|r| r.id.clone()).unwrap_or_default();
        let room = room_of(self);
        room.members = cx.load(crate::members(cx.host(), members_id), |chat| {
            &mut room_of(chat).members
        });
        if land > 0 {
            self.reveal(STREAM_KEY, land, window, cx);
        }
    }

    /// The newest window landed: the rows, and whether older ones remain.
    fn rows_arrived(
        &mut self,
        result: Result<(Vec<MsgRow>, bool), Refusal>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = &mut self.room else { return };
        match result {
            Ok((rows, has_older)) => {
                room.has_older = has_older;
                room.reaches_head = true;
                room.messages = Loaded::Ready(rows);
                room.settle();
                self.load_pictures(cx);
            }
            Err(refusal) => room.messages = Loaded::Failed(refusal),
        }
    }

    /// Re-read what the room shows, keeping the rows there until fresh land.
    pub(crate) fn refresh_room(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else { return };
        let (id, viewer) = (room.id.clone(), self.viewer());
        if !room.landed {
            let shown = room
                .messages
                .ready()
                .map_or(WINDOW, |rows| rows.len().max(WINDOW));
            cx.spawn({
                let (id, viewer) = (id.clone(), viewer.clone());
                async move |this, cx| {
                    let host = cx.host();
                    let result = crate::roots(host, id, viewer, None, shown).await;
                    let _ = this.update(cx, |chat, cx| {
                        cx.notify();
                        if let (Ok((rows, has_older)), Some(room)) = (result, chat.room.as_mut()) {
                            room.has_older = has_older;
                            room.messages = Loaded::Ready(rows);
                            room.settle();
                            chat.load_pictures(cx);
                        }
                    });
                }
            })
            .detach();
        }
        cx.refresh(crate::members(cx.host(), id.clone()), |chat, members, _| {
            room_of(chat).members = Loaded::Ready(members);
        });
        if let Some(thread) = &room.thread {
            let root = thread.root;
            cx.refresh(
                crate::thread(cx.host(), id, root, viewer, None),
                move |chat, page, cx| {
                    let Some(thread) = chat.room.as_mut().and_then(|room| room.thread.as_mut())
                    else {
                        return;
                    };
                    if thread.root == root {
                        thread.replies = Loaded::Ready(page.0);
                        thread.has_more = page.1.is_some();
                        thread.next = page.1;
                        room_of(chat).settle();
                        chat.load_pictures(cx);
                    }
                },
            );
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
            let result = crate::roots(host, id, viewer, Some(below), crate::PAGE).await;
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
                        chat.load_pictures(cx);
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
        pane: crate::Pane,
        event: &ducktape_view_guest::ListScrollEvent,
        cx: &mut Context<Self>,
    ) {
        match pane {
            crate::Pane::Timeline => {
                let Some(room) = &mut self.room else { return };
                room.at_tail = event.is_following_tail || event.visible_range.end >= event.count;
                if event.visible_range.start <= 4 {
                    self.load_older(cx);
                }
            }
            crate::Pane::Thread => {
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
            let result = crate::thread(host, id, root, viewer, None).await;
            let _ = this.update(cx, |chat, cx| {
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
                        chat.load_pictures(cx);
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
            let result = crate::thread(host, id, root, viewer, after).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let Some(thread) = chat.room.as_mut().and_then(|r| r.thread.as_mut()) else {
                    return;
                };
                thread.more_loading = false;
                if let Ok((more, next)) = result
                    && thread.root == root
                {
                    thread.has_more = next.is_some();
                    thread.next = next;
                    if let Some(rows) = thread.replies.ready_mut() {
                        rows.extend(more);
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
            .is_some_and(|menu| menu.pane == crate::Pane::Thread)
        {
            self.menu = None;
        }
        if self
            .copy
            .is_some_and(|copy| copy.pane == crate::Pane::Thread)
        {
            self.copy = None;
        }
    }

    /// The channel list landed: the rooms, and what each one's head says
    /// about what the reader has read.
    pub(crate) fn channels_arrived(&mut self, channels: Vec<ChannelInfo>) {
        let reading = self
            .room
            .as_ref()
            .filter(|room| self.reads.visible && !room.landed)
            .map(|room| room.id.clone());
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
                *cursor = (*cursor).max(info.head_seq);
            }
        }
        self.channels = Loaded::Ready(channels);
    }

    pub(crate) fn unread(&self, info: &ChannelInfo) -> bool {
        self.reads
            .cursors
            .get(&info.channel.id)
            .is_some_and(|cursor| info.head_seq > *cursor)
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
