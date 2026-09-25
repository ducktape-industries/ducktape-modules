//! What a press does: the message menus, the writes (reactions, deletes,
//! the channel's details, new channels), copying and links.
use chat::{MsgRow, Op, Party, PostPolicy};
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::wire;

use crate::api::{ChatApi, ClipboardWrite, HostId, Submit};
use crate::composer::Target;
use crate::message::{ChatMessage, chat_message, mark_message_groups};
use crate::{Chat, Menu, Mode, Pane, links};
use chat::view::Names;

/// A refusal the archive gives every reaction, said before it is asked.
const ARCHIVED_REACTIONS: &str = "This channel is archived — reactions are closed. Unarchive it from Channel details to react here again.";

impl Chat {
    // ---------- menus ----------

    /// A press on a message's body: chosen, its actions stay open.
    /// A control on a message card took `event`: the card's own select
    /// handler, handed the same click after it, stands down.
    /// A key press reaches only the focused control, so only a pointer's
    /// click is claimed.
    pub(crate) fn claim(&mut self, event: &ducktape_view_guest::ClickEvent) {
        use ducktape_view_guest::ClickEvent;
        self.claimed = match event {
            ClickEvent::Keyboard(_) => None,
            ClickEvent::Mouse(_) | ClickEvent::Touch(_) => {
                let at = event.position();
                Some((at.x.into(), at.y.into()))
            }
        };
    }

    /// Whether a control on the card already took the click at `at`.
    pub(crate) fn was_claimed(&mut self, at: (f32, f32)) -> bool {
        self.claimed.take() == Some(at)
    }

    pub(crate) fn press_message(&mut self, pane: Pane, seq: u64) {
        if seq == 0 {
            return;
        }
        self.menu = Some(Menu {
            pane,
            seq,
            rev: 0,
            mode: Mode::Toolbar,
            at: self.layout.press,
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
            self.notice = ARCHIVED_REACTIONS.into();
            return;
        }
        if mode == Mode::Editing {
            let Some(body) = self.edit_body(pane, seq) else {
                return;
            };
            let target = Target::Edit {
                channel: self.room_id(),
                seq,
                base_rev: rev,
            };
            let choices = self.mention_choices();
            self.drafts
                .entry(target.key())
                .or_default()
                .seed(&body, &choices);
        }
        if mode == Mode::Reactions {
            self.picker = Default::default();
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
                target: vec![wire::ElementIdWire::Name(
                    crate::ui::menu::focus_key(pane, mode).into(),
                )],
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
    pub(crate) fn rows(&self, pane: Pane) -> Vec<MsgRow> {
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
        let empty = Names::empty();
        let names = self.names.ready().unwrap_or(&empty);
        let mut messages: Vec<ChatMessage> = self
            .rows(pane)
            .into_iter()
            .map(|row| chat_message(row, names))
            .collect();
        let boundary = (pane == Pane::Timeline).then_some(self.reads.boundary);
        mark_message_groups(&mut messages, boundary);
        messages
    }

    /// Whether the reader wrote the message at `seq`: only its author
    /// edits it, and the chat module refuses anyone else.
    pub(crate) fn wrote(&self, pane: Pane, seq: u64) -> bool {
        let Some(me) = self.me() else { return false };
        self.rows(pane)
            .iter()
            .any(|row| row.seq == seq && row.author == me)
    }

    /// Whether the reader may delete the message at `seq`: its author, or
    /// the channel's owner.
    pub(crate) fn may_delete(&self, pane: Pane, seq: u64) -> bool {
        self.wrote(pane, seq)
            || self
                .room_info()
                .is_some_and(|info| Some(&info.channel.owner) == self.me().as_ref())
    }

    // ---------- writes ----------

    pub(crate) fn react(&mut self, seq: u64, emoji: String, add: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() || seq == 0 {
            return;
        }
        if self.room_info().is_some_and(|i| i.channel.archived) {
            self.notice = ARCHIVED_REACTIONS.into();
            return;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.mode == Mode::Reactions)
        {
            self.menu = None;
        }
        if add {
            crate::emoji::remember(&mut self.recent_emoji, &emoji);
            self.save_emoji(cx);
        }
        let op = if add {
            Op::AddReaction {
                channel_id,
                seq,
                emoji,
            }
        } else {
            Op::RemoveReaction {
                channel_id,
                seq,
                emoji,
            }
        };
        self.submit(op, cx);
    }

    pub(crate) fn delete_armed(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else { return };
        if menu.mode != Mode::Delete {
            return;
        }
        let channel_id = self.room_id();
        self.submit(
            Op::DeleteMessage {
                channel_id,
                seq: menu.seq,
            },
            cx,
        );
    }

    pub(crate) fn rename(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let name = details.name_draft.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let channel_id = self.room_id();
        self.submit(Op::RenameChannel { channel_id, name }, cx);
    }

    pub(crate) fn set_archived(&mut self, archived: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        self.submit(
            Op::SetChannelArchived {
                channel_id,
                archived,
            },
            cx,
        );
    }

    /// The member typed in the details pane joins the room.
    pub(crate) fn add_member(&mut self, cx: &mut Context<Self>) {
        let typed = self
            .details
            .as_ref()
            .map(|details| details.member_draft.as_str());
        let Some(party) = typed.and_then(Party::parse) else {
            self.notice = "A member is an account number".into();
            return;
        };
        if let Some(details) = &mut self.details {
            details.member_draft.clear();
        }
        self.set_member(party, true, cx);
    }

    pub(crate) fn set_member(&mut self, party: Party, member: bool, cx: &mut Context<Self>) {
        let channel_id = self.room_id();
        if channel_id.is_empty() {
            return;
        }
        self.submit(
            Op::SetMembership {
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

    pub(crate) fn copy_text(&mut self, text: String, label: &str, cx: &mut Context<Self>) {
        if !text.is_empty() {
            cx.host().notify::<ClipboardWrite>(text);
            self.confirmation = format!("Copied {label}");
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
        let label = ducktape_view_guest::design::plural(lines.len() as u64, "message", "messages");
        self.copy_text(lines.join("\n"), &label, cx);
    }

    pub(crate) fn copy_count(&self) -> usize {
        let Some(range) = self.copy else { return 0 };
        self.rows(range.pane)
            .iter()
            .filter(|row| range.holds(range.pane, row.seq))
            .count()
    }

    /// None before the session names a chain: Copy link is not offered.
    pub(crate) fn message_link(&self, seq: u64) -> Option<String> {
        links::channel_link(&self.session.chain, &self.room_id(), Some(seq))
    }

    pub(crate) fn open_link(&mut self, link: String, cx: &mut Context<Self>) {
        self.create = None;
        match links::pressed_link(link, &self.session.chain) {
            Some(url) => cx.host().open_link(&url),
            None => cx.host().log("no link to open: the session names no chain"),
        }
    }

    // ---------- submitting ----------

    /// One op to the chat program; a refusal lands in the banner.
    pub(crate) fn submit(&mut self, op: Op, cx: &mut Context<Self>) {
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

    /// The create dialog's channel: the host names it, chat opens it.
    pub(crate) fn create_channel(&mut self, cx: &mut Context<Self>) {
        let ready = self.holds_account() && self.session.connected;
        let Some(create) = self.create.as_mut().filter(|create| ready && !create.busy) else {
            return;
        };
        let name = create.name.trim().to_string();
        if name.is_empty() || name.len() > chat::MAX_NAME_BYTES || name.contains('\0') {
            create.error = format!(
                "Enter a channel name of at most {} bytes",
                chat::MAX_NAME_BYTES
            );
            return;
        }
        create.error.clear();
        create.busy = true;
        let (voice, members_only) = (create.voice, create.members_only);
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let created = async {
                let channel_id = host.ask::<HostId>("channel".into()).await?;
                let op = new_channel(channel_id.clone(), name, voice, members_only);
                host.ask::<Submit<ChatApi>>(op).await?;
                Ok::<_, Refusal>(channel_id)
            };
            let result = created.await;
            let _ = this.update_in(cx, |chat, window, cx| {
                cx.notify();
                match result {
                    Ok(id) => {
                        chat.create = None;
                        chat.reread_channels(cx);
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

fn new_channel(channel_id: String, name: String, voice: bool, members_only: bool) -> Op {
    if voice {
        return Op::CreateVoiceChannel { channel_id, name };
    }
    let post_policy = match members_only {
        true => PostPolicy::MembersOnly,
        false => PostPolicy::Open,
    };
    Op::CreateChannel {
        channel_id,
        name,
        post_policy,
    }
}
