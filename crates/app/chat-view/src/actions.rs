//! What a press does: the message menus, reactions, edits and deletes, the
//! channel's details, copying, links, the attachment preview, pictures, the
//! search and the live-run poll.
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire;

use crate::api::{Copy, copy};
use crate::chat::{ChatMsg, party_of};
use crate::client::{ChatMessage, NameDirectory, chat_message};
use crate::{Chat, CopyRange, Hits, Menu, Mode, Pane, Preview};

impl Chat {
    pub(crate) fn room_id(&self) -> String {
        self.room
            .as_ref()
            .map(|room| room.id.clone())
            .unwrap_or_default()
    }

    // ---------- menus ----------

    /// A press on a message's body: chosen (its actions stay open), or with
    /// shift held the copy range grows to it.
    pub(crate) fn press_message(&mut self, pane: Pane, seq: u64) {
        if seq == 0 {
            return;
        }
        if !self.session.shift_held {
            self.menu = Some(Menu {
                pane,
                seq,
                rev: 0,
                mode: Mode::Toolbar,
                at: self.layout.press,
            });
            return;
        }
        let anchor = match self.copy {
            Some(range) if range.pane == pane => range.anchor,
            _ => seq,
        };
        self.copy = Some(CopyRange {
            pane,
            anchor,
            head: seq,
        });
    }

    pub(crate) fn open_menu(
        &mut self,
        pane: Pane,
        seq: u64,
        rev: u32,
        mode: Mode,
        window: &mut ducktape_view_guest::Window,
        _cx: &mut Context<Self>,
    ) {
        if seq == 0 {
            return;
        }
        if mode == Mode::Reactions && self.room_info().is_some_and(|i| i.channel.archived) {
            self.notice = "This channel is archived — reactions are closed. Unarchive it from Channel details to react here again.".into();
            return;
        }
        if mode == Mode::Editing {
            let Some(body) = self.edit_body(pane, seq) else {
                return;
            };
            let target = crate::composer::Target::Edit {
                channel: self.room_id(),
                seq,
                base_rev: rev,
            };
            let choices = self.mention_choices();
            self.drafts
                .entry(crate::draft_key(&target))
                .or_default()
                .seed(&body, &choices);
        }
        self.menu = Some(Menu {
            pane,
            seq,
            rev,
            mode,
            at: self.layout.press,
        });
        if mode != Mode::Editing {
            window.dispatch(wire::WidgetCommand::Focus {
                target: crate::ui::menu::focus_key(pane, mode),
            });
        }
    }

    pub(crate) fn close_menu(&mut self) {
        self.menu = None;
    }

    fn edit_body(&self, pane: Pane, seq: u64) -> Option<String> {
        let names = self.names.ready().cloned().unwrap_or_default();
        let row = self.rows(pane).into_iter().find(|row| row.seq == seq)?;
        let message = chat_message(row, &names);
        (!message.edit_body.is_empty()).then_some(message.edit_body)
    }

    /// The rows one pane shows, fetched and pending, as the index served them.
    pub(crate) fn rows(&self, pane: Pane) -> Vec<crate::chat::MsgRow> {
        let Some(room) = &self.room else {
            return Vec::new();
        };
        match pane {
            Pane::Timeline => room
                .messages
                .ready()
                .into_iter()
                .flatten()
                .chain(&room.pending)
                .filter(|row| row.thread.is_none())
                .cloned()
                .collect(),
            Pane::Thread => {
                let Some(thread) = &room.thread else {
                    return Vec::new();
                };
                let root = room
                    .messages
                    .ready()
                    .and_then(|rows| rows.iter().find(|row| row.seq == thread.root));
                root.into_iter()
                    .chain(thread.replies.ready().into_iter().flatten())
                    .chain(
                        room.pending
                            .iter()
                            .filter(|row| row.thread == Some(thread.root)),
                    )
                    .cloned()
                    .collect()
            }
        }
    }

    /// Rows to messages, named by the directory the reader has.
    pub(crate) fn messages(&self, pane: Pane) -> Vec<ChatMessage> {
        let empty = NameDirectory::empty();
        let names = self.names.ready().unwrap_or(&empty);
        let mut messages: Vec<ChatMessage> = self
            .rows(pane)
            .into_iter()
            .map(|row| chat_message(row, names))
            .collect();
        crate::client::mark_message_groups(&mut messages);
        messages
    }

    // ---------- writes ----------

    pub(crate) fn react(&mut self, seq: u64, emoji: String, add: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() || seq == 0 {
            return;
        }
        if self.room_info().is_some_and(|i| i.channel.archived) {
            self.notice = "This channel is archived — reactions are closed. Unarchive it from Channel details to react here again.".into();
            return;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.mode == Mode::Reactions)
        {
            self.menu = None;
        }
        let op = if add {
            ChatMsg::AddReaction {
                channel_id,
                seq,
                emoji,
            }
        } else {
            ChatMsg::RemoveReaction {
                channel_id,
                seq,
                emoji,
            }
        };
        self.submit(op, cx);
    }

    pub(crate) fn delete_armed(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else { return };
        if menu.mode != Mode::Delete || self.session.busy {
            return;
        }
        let channel_id = self.room_id();
        self.submit(
            ChatMsg::DeleteMessage {
                channel_id,
                seq: menu.seq,
            },
            cx,
        );
    }

    pub(crate) fn rename(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let name = details.name_draft.trim().to_owned();
        if name.is_empty() || self.session.busy {
            return;
        }
        let channel_id = self.room_id();
        self.submit(ChatMsg::RenameChannel { channel_id, name }, cx);
    }

    pub(crate) fn set_archived(&mut self, archived: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() || self.session.busy {
            return;
        }
        self.submit(
            ChatMsg::SetChannelArchived {
                channel_id,
                archived,
            },
            cx,
        );
    }

    pub(crate) fn set_member(&mut self, text: &str, member: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() || self.session.busy {
            return;
        }
        let Some(party) = party_of(text) else {
            self.notice = "A member is an account number or a key hex".into();
            return;
        };
        if let Some(details) = &mut self.details {
            details.member_draft.clear();
        }
        self.submit(
            ChatMsg::SetMembership {
                channel_id,
                party,
                member,
            },
            cx,
        );
    }

    pub(crate) fn toggle_details(&mut self) {
        match self.details.take() {
            Some(_) => {}
            None => {
                let name = self
                    .room_info()
                    .map(|info| info.channel.name.clone())
                    .unwrap_or_default();
                self.details = Some(crate::Details {
                    name_draft: name,
                    member_draft: String::new(),
                });
            }
        }
    }

    // ---------- copying and links ----------

    pub(crate) fn copy_text(&self, text: String, label: &str, cx: &mut Context<Self>) {
        if !text.is_empty() {
            cx.host().notify::<Copy>(copy(&text, label));
        }
    }

    /// The copy range as one run of text, to the clipboard.
    pub(crate) fn copy_range(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self.copy else { return };
        let lines: Vec<String> = self
            .messages(range.pane)
            .into_iter()
            .filter(|m| range.holds(range.pane, m.seq))
            .map(|m| format!("{}: {}", m.author, m.body))
            .collect();
        if lines.is_empty() {
            return;
        }
        let label = match lines.len() {
            1 => "1 message selected".to_owned(),
            n => format!("{n} messages selected"),
        };
        self.copy_text(lines.join("\n"), &label, cx);
    }

    pub(crate) fn copy_count(&self) -> usize {
        let Some(range) = self.copy else { return 0 };
        self.rows(range.pane)
            .iter()
            .filter(|row| range.holds(range.pane, row.seq))
            .count()
    }

    pub(crate) fn message_link(&self, seq: u64) -> String {
        crate::files::channel_link(&self.session.chain, &self.room_id(), Some(seq))
    }

    pub(crate) fn open_link(&mut self, link: String, cx: &mut Context<Self>) {
        self.preview = None;
        self.create = None;
        let url = crate::files::pressed_link(link, &self.session.chain);
        if !url.is_empty() {
            cx.host().open_link(&url);
        }
    }

    // ---------- attachments ----------

    pub(crate) fn open_preview(&mut self, link: String, cx: &mut Context<Self>) {
        if !crate::ATTACHMENTS {
            return self.open_link(link, cx);
        }
        self.preview = Some(Preview {
            link,
            read: Loaded::Idle,
        });
        self.preview_read(cx);
    }

    /// A preview that is not a decoded picture reads its file.
    pub(crate) fn preview_read(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = &self.preview else { return };
        let picture = matches!(self.pictures.get(&preview.link), Some(&(w, h)) if w > 0 && h > 0);
        if picture || !preview.read.is_idle() {
            return;
        }
        let path = crate::files::attachment_file_path(&preview.link);
        let load = cx.load(crate::files::read_preview(cx.host(), path), |chat| {
            &mut chat.preview.get_or_insert_default().read
        });
        if let Some(preview) = &mut self.preview {
            preview.read = load;
        }
    }

    /// Every picture attachment on screen the host has not been asked for
    /// yet: one decode each, answered into `pictures`.
    pub(crate) fn load_pictures(&mut self, cx: &mut Context<Self>) {
        if !crate::ATTACHMENTS {
            return;
        }
        let links: Vec<String> = [Pane::Timeline, Pane::Thread]
            .into_iter()
            .flat_map(|pane| self.messages(pane))
            .flat_map(|m| m.blocks)
            .filter(|b| b.kind == "attachment" && crate::files::is_picture(&b.text))
            .map(|b| b.link)
            .filter(|link| !self.pictures.contains_key(link))
            .collect();
        for link in links {
            self.pictures.insert(link.clone(), (-1, -1));
            let path = crate::files::attachment_file_path(&link);
            cx.spawn(async move |this, cx| {
                let host = cx.host();
                let drawn = crate::files::picture_load(host, path).await;
                let _ = this.update(cx, |chat, cx| {
                    cx.notify();
                    chat.pictures.insert(link, drawn);
                    chat.preview_read(cx);
                });
            })
            .detach();
        }
    }

    // ---------- search ----------

    pub(crate) fn search_submit(&mut self, cx: &mut Context<Self>) {
        let query = self.search.draft.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.search.query = query;
        self.search_now(cx);
    }

    pub(crate) fn search_now(&mut self, cx: &mut Context<Self>) {
        let (text, viewer) = (self.search.query.clone(), self.viewer());
        let host = cx.host();
        self.search.hits = cx.load(
            async move {
                let (rows, capped, has_more, next_after) =
                    crate::search_hits(host, text, None, viewer, None).await?;
                Ok(Hits {
                    rows,
                    capped,
                    has_more,
                    next_after,
                })
            },
            |chat| &mut chat.search.hits,
        );
    }

    pub(crate) fn search_more(&mut self, cx: &mut Context<Self>) {
        let Some(after) = self.search.hits.ready().and_then(|h| h.next_after.clone()) else {
            return;
        };
        if self.search.more_loading {
            return;
        }
        self.search.more_loading = true;
        let (text, viewer) = (self.search.query.clone(), self.viewer());
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = crate::search_hits(host, text, None, viewer, Some(after)).await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                chat.search.more_loading = false;
                let Ok((rows, _, has_more, next_after)) = result else {
                    return;
                };
                if let Some(hits) = chat.search.hits.ready_mut() {
                    for row in rows {
                        if !hits
                            .rows
                            .iter()
                            .any(|h| h.channel_id == row.channel_id && h.seq == row.seq)
                        {
                            hits.rows.push(row);
                        }
                    }
                    hits.has_more = has_more;
                    hits.next_after = next_after;
                }
            });
        })
        .detach();
    }

    pub(crate) fn search_clear(&mut self) {
        self.search = crate::Search::default();
    }

    /// A hit opens its room around the message.
    pub(crate) fn open_hit(
        &mut self,
        channel_id: String,
        seq: u64,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        self.search_clear();
        self.create = None;
        self.open_at(channel_id, seq, window, cx);
    }
}
