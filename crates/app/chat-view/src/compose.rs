//! The composer's events for one target, run through its draft: sends,
//! edits, attachments picked, dropped or pasted, and their uploads.
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Submit;
use ducktape_view_guest::wire;
use futures::StreamExt;

use crate::api::{ChatApi, ClipboardRead, ClipboardWrite, Drops, Id, Pick, Release, SelectedFile};
use crate::chat::{ChatMsg, MsgRow};
use crate::composer::Target;
use crate::composer::{Attachment, AttachmentState, Event, Outcome, Send};
use crate::{Chat, Mode, draft_key};

impl Chat {
    /// The target a dropped file lands in: the open thread, else the room.
    fn drop_target(&self) -> Option<Target> {
        let room = self.room.as_ref()?;
        if !crate::ATTACHMENTS || !self.session.connected || !self.may_write() {
            return None;
        }
        Some(Target::Post {
            channel: room.id.clone(),
            thread: room.thread.as_ref().map(|t| t.root),
        })
    }

    /// Files dropped on the window go to the composer that accepts them.
    pub(crate) fn watch_drops(&mut self, cx: &mut Context<Self>) {
        let wants = self.drop_target().is_some();
        if wants == self.watches.drops.is_some() {
            return;
        }
        self.watches.drops = wants.then(|| {
            let mut drops = cx.host().subscribe::<Drops>(());
            cx.spawn(async move |this, cx| {
                while let Some(files) = drops.next().await {
                    if this
                        .update(cx, |chat, cx| {
                            if let Some(target) = chat.drop_target() {
                                cx.notify();
                                chat.picked(target, files.map_err(|r| r.sentence), cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
    }

    pub(crate) fn composer(
        &mut self,
        target: Target,
        event: Event<Self>,
        window: &mut ducktape_view_guest::Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        let choices = self.mention_choices();
        let key = draft_key(&target);
        let draft = self.drafts.entry(key.clone()).or_default();
        match draft.handle(event, &choices, cx) {
            Outcome::Updated => {}
            Outcome::Run(run) => run(self, window, cx),
            Outcome::Enqueue(tag) => window.dispatch(wire::WidgetCommand::EditorAction {
                target: vec![wire::ElementIdWire::Name(format!("{key}/editor").into())],
                tag,
            }),
            Outcome::Action(tag) => self.composer_action(target, &key, &tag, cx),
        }
    }

    fn composer_action(&mut self, target: Target, key: &str, tag: &str, cx: &mut Context<Self>) {
        if let Some(token) = tag.strip_prefix("remove:") {
            self.uploads.remove(token);
            self.drafts
                .entry(key.to_owned())
                .or_default()
                .attachments
                .retain(|file| file.token != token);
            let token = token.to_owned();
            cx.spawn(async move |_, cx| {
                let _ = cx.host().ask::<Release>(token).await;
            })
            .detach();
            return;
        }
        if let Some(token) = tag.strip_prefix("retry:") {
            let draft = self.drafts.entry(key.to_owned()).or_default();
            let Some(index) = draft.attachments.iter().position(|file| {
                file.token == token && matches!(file.state, AttachmentState::Failed { .. })
            }) else {
                return;
            };
            let file = draft.attachments.remove(index);
            self.picked(
                target,
                Ok(vec![SelectedFile {
                    token: file.token,
                    name: file.name,
                    bytes: file.bytes,
                }]),
                cx,
            );
            return;
        }
        match tag {
            "send" => {
                let draft = self.drafts.entry(key.to_owned()).or_default();
                if let Some(send) = draft.submitted.take() {
                    draft.in_flight.push(send.clone());
                    self.send(key.to_owned(), send, target, cx);
                }
            }
            "restore" => {
                let choices = self.mention_choices();
                let draft = self.drafts.entry(key.to_owned()).or_default();
                if let Some(send) = draft.failed_send.take() {
                    draft.seed(&send.body, &choices);
                }
            }
            "attach" if crate::ATTACHMENTS && matches!(target, Target::Post { .. }) => {
                cx.spawn(async move |this, cx| {
                    let host = cx.host();
                    let result = host.ask::<Pick>(()).await;
                    let _ = this.update(cx, |chat, cx| {
                        cx.notify();
                        chat.picked(target, result.map_err(|r| r.sentence), cx)
                    });
                })
                .detach();
            }
            "paste" => {
                let key = key.to_owned();
                cx.spawn(async move |this, cx| {
                    let host = cx.host();
                    let result = host.ask::<ClipboardRead>(()).await;
                    let _ = this.update_in(cx, |chat, window, cx| {
                        cx.notify();
                        match result {
                            Ok(clipboard) => {
                                chat.drafts.entry(key.clone()).or_default().paste =
                                    Some(clipboard.text);
                                window.dispatch(wire::WidgetCommand::EditorAction {
                                    target: vec![wire::ElementIdWire::Name(
                                        format!("{key}/editor").into(),
                                    )],
                                    tag: "paste-ready".into(),
                                });
                                if crate::ATTACHMENTS
                                    && matches!(target, Target::Post { .. })
                                    && !clipboard.files.is_empty()
                                {
                                    chat.picked(target, Ok(clipboard.files), cx);
                                }
                            }
                            Err(refusal) => {
                                chat.drafts.entry(key).or_default().note = refusal.sentence
                            }
                        }
                    });
                })
                .detach();
            }
            "copy" | "cut" => {
                let Some(text) = self
                    .drafts
                    .entry(key.to_owned())
                    .or_default()
                    .clipboard
                    .take()
                else {
                    return;
                };
                let key = key.to_owned();
                cx.spawn(async move |this, cx| {
                    let host = cx.host();
                    let result = host.ask::<ClipboardWrite>(text).await;
                    let _ = this.update(cx, |chat, cx| {
                        cx.notify();
                        if let Err(refusal) = result {
                            chat.drafts.entry(key).or_default().note = refusal.sentence;
                        }
                    });
                })
                .detach();
            }
            _ => {}
        }
    }

    /// Files the reader picked, dropped or pasted: each becomes an
    /// attachment uploading into the files module.
    pub(crate) fn picked(
        &mut self,
        target: Target,
        result: Result<Vec<SelectedFile>, String>,
        cx: &mut Context<Self>,
    ) {
        let key = draft_key(&target);
        let draft = self.drafts.entry(key.clone()).or_default();
        let files = match result {
            Ok(files) => files,
            Err(error) => {
                draft.note = error;
                return;
            }
        };
        let chain = self.session.chain.to_owned();
        for file in files {
            draft.attachments.push(Attachment {
                token: file.token.clone(),
                name: file.name.clone(),
                bytes: file.bytes,
                state: AttachmentState::Uploading,
            });
            let (draft_key, token, chain) = (key.clone(), file.token.clone(), chain.clone());
            let upload_token = token.clone();
            let handle = cx.spawn(async move |this, cx| {
                let host = cx.host();
                let result = crate::files::upload(host, file, chain).await;
                let _ = this.update(cx, |chat, cx| {
                    cx.notify();
                    chat.uploads.remove(&token);
                    let Some(draft) = chat.drafts.get_mut(&draft_key) else {
                        return;
                    };
                    if let Some(file) = draft.attachments.iter_mut().find(|f| f.token == token) {
                        file.state = match result {
                            Ok(uri) => AttachmentState::Ready { uri },
                            Err(refusal) => AttachmentState::Failed {
                                reason: refusal.sentence,
                            },
                        };
                    }
                });
            });
            self.uploads.insert(upload_token, handle);
        }
    }

    fn send(&mut self, key: String, send: Send, target: Target, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = async {
                let id = host.ask::<Id>("message".into()).await?;
                let op = crate::composer::op(id, &send, &target)?;
                let pending = pending_row(&op);
                host.ask::<Submit<ChatApi>>(op).await.map(|_| pending)
            }
            .await;
            let _ = this.update(cx, |chat, cx| {
                cx.notify();
                let draft = chat.drafts.entry(key).or_default();
                draft.complete_send(&send);
                match result {
                    Ok(pending) => {
                        let me = chat.session.account.clone();
                        if let (Some(mut row), Some(room)) = (pending, chat.room.as_mut())
                            && room.id == target.channel()
                        {
                            row.author = me;
                            room.pending.push(row);
                        }
                        if let Target::Edit { seq, .. } = target
                            && chat
                                .menu
                                .as_ref()
                                .is_some_and(|m| m.mode == Mode::Editing && m.seq == seq)
                        {
                            chat.menu = None;
                        }
                        chat.refresh(cx);
                    }
                    Err(refusal) => {
                        draft.note = refusal.sentence;
                        draft.failed(send);
                    }
                }
            });
        })
        .detach();
    }
}

/// The row a just-accepted post shows as until the index serves it: the
/// reader as author, seq 0. An edit shows nothing early.
pub(crate) fn pending_row(op: &ChatMsg) -> Option<MsgRow> {
    let ChatMsg::PostMessage {
        message_id,
        blocks,
        thread,
        ..
    } = op
    else {
        return None;
    };
    Some(MsgRow {
        message_id: message_id.clone(),
        blocks: blocks.clone(),
        thread: *thread,
        ..MsgRow::default()
    })
}
