//! Native GPUI message lists. The room and thread use native variable-height lists;
//! callbacks still call the root view's existing message operations.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    ClickEvent, Context, FollowMode, ListAlignment, ListSizingBehavior, ListState, ParentElement,
    Styled, Theme, div, list as gpui_list, px,
};

use crate::ui::room::selection_bar;
use crate::ui::{message, quiet};
use crate::{Chat, Loaded, Pane};

pub fn list(chat: &Chat, pane: Pane, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let messages = chat.messages(pane);
    let mut content = div()
        .id(match pane {
            Pane::Timeline => "chat-timeline",
            Pane::Thread => "chat-thread-messages",
        })
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
    if messages.is_empty() {
        let loading = match pane {
            Pane::Timeline => chat
                .room
                .as_ref()
                .is_some_and(|room| matches!(room.messages, Loaded::Loading(_))),
            Pane::Thread => chat
                .room
                .as_ref()
                .and_then(|room| room.thread.as_ref())
                .is_some_and(|thread| matches!(thread.replies, Loaded::Loading(_))),
        };
        if loading {
            return content.child(quiet("Loading messages…", theme).p_4());
        }
        let failed = match pane {
            Pane::Timeline => chat
                .room
                .as_ref()
                .and_then(|room| room.messages.failed())
                .map(|error| error.sentence.clone()),
            Pane::Thread => chat
                .room
                .as_ref()
                .and_then(|room| room.thread.as_ref())
                .and_then(|thread| thread.replies.failed())
                .map(|error| error.sentence.clone()),
        };
        if let Some(error) = failed {
            return content.child(quiet(error, theme).p_4());
        }
        if pane == Pane::Thread {
            return content.child(no_replies(theme));
        }
        if let Some(room) = &chat.room {
            let name = chat
                .info(&room.id)
                .map_or_else(|| room.id.clone(), |info| info.channel.name.clone());
            return content.child(intro(
                &name,
                super::sidebar::dm_peer(chat)
                    .as_ref()
                    .map(|peer| peer.0.as_str()),
                theme,
            ));
        }
    }
    if let Some(room) = &chat.room
        && matches!(pane, Pane::Timeline)
        && room.has_older
        && !room.landed
    {
        let older = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.load_older(cx)
        });
        let label = if room.older_loading {
            "Loading older messages…"
        } else {
            "Load older messages"
        };
        let control = if room.older_loading {
            div()
                .id("chat-load-older-button")
                .text_color(theme.muted)
                .child(label)
                .into_any_element()
        } else {
            super::button("chat-load-older-button", label, theme, older).into_any_element()
        };
        content = content.child(
            div()
                .id("chat-load-older")
                .flex()
                .justify_center()
                .p_2()
                .child(control),
        );
    }
    if !messages.is_empty() {
        let lead = matches!(pane, Pane::Timeline)
            && chat
                .room
                .as_ref()
                .is_some_and(|room| !room.has_older && !room.landed);
        let lead_text = lead.then(|| {
            let room = chat.room.as_ref().expect("timeline room");
            (
                chat.info(&room.id)
                    .map_or_else(|| room.id.clone(), |info| info.channel.name.clone()),
                super::sidebar::dm_peer(chat).map(|peer| peer.0),
            )
        });
        // a thread that is only its root says so under it
        let bare = pane == Pane::Thread
            && messages.len() == 1
            && chat
                .room
                .as_ref()
                .and_then(|room| room.thread.as_ref())
                .is_some_and(|thread| thread.replies.ready().is_some_and(Vec::is_empty));
        let keys = lead
            .then_some("intro".to_owned())
            .into_iter()
            .chain(messages.iter().map(|message| message.id.clone()))
            .chain(bare.then_some("no-replies".to_owned()))
            .collect::<Vec<_>>();
        let state = list_state(chat, pane, &keys);
        state.set_follow_mode(if pane == Pane::Timeline {
            FollowMode::Tail
        } else {
            FollowMode::Normal
        });
        let observed_pane = pane;
        state.set_scroll_handler(cx.listener(move |chat, event, _window, cx| {
            chat.list_scrolled(observed_pane, event, cx);
        }));
        let pane_for_items = pane;
        let list_theme = *theme;
        let list_messages = messages.clone();
        let unread_seq = (pane == Pane::Timeline && chat.reads.boundary > 0)
            .then(|| {
                list_messages
                    .iter()
                    .find(|message| !message.pending && message.seq > chat.reads.boundary)
                    .map(|message| message.seq)
            })
            .flatten();
        let list = gpui_list(
            state,
            cx.processor(move |chat, index: usize, window, cx| {
                if lead && index == 0 {
                    let (name, dm) = lead_text.clone().expect("lead row");
                    return intro(&name, dm.as_deref(), &list_theme).into_any_element();
                }
                let message_index = index - usize::from(lead);
                let Some(message) = list_messages.get(message_index).cloned() else {
                    return match bare {
                        true => no_replies(&list_theme).into_any_element(),
                        false => div().into_any_element(),
                    };
                };
                let unread = unread_seq == Some(message.seq);
                let card = message::card(chat, message, pane_for_items, window, cx, &list_theme);
                if unread {
                    // full width, as a bare card is: a row shrunk to its
                    // words took the hover and the action strip with it
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(unread_marker(&list_theme))
                        .child(card)
                        .into_any_element()
                } else {
                    card.into_any_element()
                }
            }),
        )
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .flex_1()
        .min_h(px(0.))
        .w_full();
        content = content.child(
            div()
                .id(match pane {
                    Pane::Timeline => "chat-message-list",
                    Pane::Thread => "chat-thread-list",
                })
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .child(list),
        );
    }
    if matches!(pane, Pane::Timeline) && chat.copy.is_some_and(|copy| copy.pane == pane) {
        content = content.child(selection_bar(chat, cx, theme));
    }
    let behind_head = chat
        .room
        .as_ref()
        .is_some_and(|room| room.landed && !room.reaches_head);
    let at_tail = chat.room.as_ref().is_some_and(|room| room.at_tail);
    if matches!(pane, Pane::Timeline)
        && !messages.is_empty()
        && (behind_head || !at_tail || chat.room.as_ref().is_some_and(|room| room.landed))
    {
        let id = chat
            .room
            .as_ref()
            .map(|room| room.id.clone())
            .unwrap_or_default();
        let latest = cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            chat.open(id.clone(), window, cx)
        });
        content = content.child(
            div()
                .id("chat-jump-latest")
                .flex()
                .justify_center()
                .p_2()
                .child(super::button(
                    "chat-jump-latest-button",
                    "Jump to latest",
                    theme,
                    latest,
                )),
        );
    }
    if let Some(editing) = super::menu::editing(chat, pane, cx, theme) {
        content = content.child(editing);
    }
    content
}

fn list_state(chat: &Chat, pane: Pane, keys: &[String]) -> ListState {
    let (slot, remembered, alignment) = match pane {
        Pane::Timeline => (
            &chat.timeline_list,
            &chat.timeline_rows,
            ListAlignment::Bottom,
        ),
        Pane::Thread => (&chat.thread_list, &chat.thread_rows, ListAlignment::Top),
    };
    let mut slot = slot.borrow_mut();
    let mut old = remembered.borrow_mut();
    if slot.is_none() {
        let state = ListState::new(keys.len(), alignment, px(160.));
        *slot = Some(state.clone());
        *old = keys.to_vec();
        return state;
    }
    let state = slot.as_ref().expect("initialized list state").clone();
    let prefix = old.iter().zip(keys).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(keys[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old.len() - suffix;
    let new_end = keys.len() - suffix;
    if prefix != old_end || prefix != new_end {
        state.splice(prefix..old_end, new_end - prefix);
    }
    *old = keys.to_vec();
    state
}

fn unread_marker(theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-unread-marker")
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .text_size(design::text::SECONDARY)
        .text_color(theme.accent_foreground)
        .child(div().h(px(1.)).flex_1().bg(theme.accent))
        .child("New messages")
}

/// The top of a room's history. The room's name is already its header's,
/// so this says only where the history starts.
fn intro(name: &str, dm: Option<&str>, theme: &Theme) -> impl IntoElement {
    let detail = match dm {
        Some(peer) => format!("This is the very beginning of your conversation with {peer}."),
        None => format!(
            "This is the very beginning of #{name}. Say hello, or pin what the room is for."
        ),
    };
    div()
        .id("chat-timeline-intro")
        .px_6()
        .pt_6()
        .pb_3()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(detail)
}

/// A thread with nothing under its root yet.
fn no_replies(theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-thread-no-replies")
        .px_4()
        .py_3()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child("No replies yet")
}
