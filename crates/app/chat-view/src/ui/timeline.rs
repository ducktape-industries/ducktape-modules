//! Native GPUI message lists. The room and thread use native variable-height lists;
//! callbacks still call the root view's existing message operations.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    ClickEvent, Context, FollowMode, ListAlignment, ListSizingBehavior, ListState, ParentElement,
    Styled, Theme, div, list as gpui_list, px,
};

use ducktape_view_guest::AnyElement;
use ducktape_view_guest::view::Loadable;

use crate::message::{ChatMessage, unread_seq};
use crate::ui::room::selection_bar;
use crate::ui::{message, quiet};
use crate::{Chat, Pane};

/// A row's height before the list has measured it: an overdraw budget, not
/// a layout size.
const UNMEASURED_ROW: f32 = 160.;

/// A pane's messages: the room's timeline or the open thread. Around the
/// list: paging older, the copy range's bar, the way back to the latest
/// message, and the edit field.
pub fn list(chat: &Chat, pane: Pane, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let messages = chat.messages(pane);
    let content = div()
        .id(match pane {
            Pane::Timeline => "chat-timeline",
            Pane::Thread => "chat-thread-messages",
        })
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
    if messages.is_empty()
        && let Some(nothing) = nothing_yet(chat, pane, theme)
    {
        return content.child(nothing);
    }
    let timeline = pane == Pane::Timeline;
    content
        .when_some(older(chat, cx, theme).filter(|_| timeline), |el, older| {
            el.child(older)
        })
        .when(!messages.is_empty(), |el| {
            el.child(rows(chat, pane, messages.clone(), cx, theme))
        })
        .when(
            timeline && chat.copy.is_some_and(|copy| copy.pane == pane),
            |el| el.child(selection_bar(chat, cx, theme)),
        )
        .when(timeline && !messages.is_empty(), |el| {
            el.when_some(jump_to_latest(chat, cx, theme), |el, jump| el.child(jump))
        })
        .when_some(
            super::menu::editing(chat, pane, cx, theme),
            |el, editing| el.child(editing),
        )
}

/// What an empty pane says: loading, refused, no replies yet, or the
/// room's beginning. None while the room itself is gone.
fn nothing_yet(chat: &Chat, pane: Pane, theme: &Theme) -> Option<AnyElement> {
    let room = chat.room.as_ref()?;
    let rows = match pane {
        Pane::Timeline => Some(&room.messages),
        Pane::Thread => room.thread.as_ref().map(|thread| &thread.replies),
    };
    if rows.is_some_and(|rows| matches!(rows, Loadable::Loading(_))) {
        return Some(quiet("Loading messages…", theme).p_4().into_any_element());
    }
    if let Some(refusal) = rows.and_then(Loadable::failed) {
        return Some(
            quiet(refusal.message.clone(), theme)
                .p_4()
                .into_any_element(),
        );
    }
    Some(match pane {
        Pane::Thread => no_replies(theme).into_any_element(),
        Pane::Timeline => {
            let (name, dm) = beginning(chat)?;
            intro(&name, dm.as_deref(), theme).into_any_element()
        }
    })
}

/// The open room's name and, for a dm, its peer: what its intro names.
fn beginning(chat: &Chat) -> Option<(String, Option<String>)> {
    let room = chat.room.as_ref()?;
    let name = chat
        .info(&room.id)
        .map_or_else(|| room.id.clone(), |info| info.channel.name.clone());
    Some((name, super::sidebar::dm_peer(chat).map(|peer| peer.0)))
}

/// The way to older history, while there is some.
fn older(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let room = chat
        .room
        .as_ref()
        .filter(|room| room.has_older && !room.landed)?;
    let control = if room.older_loading {
        div()
            .id("chat-load-older-button")
            .text_color(theme.muted)
            .child("Loading older messages…")
            .into_any_element()
    } else {
        let older = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.load_older(cx)
        });
        super::button(
            "chat-load-older-button",
            "Load older messages",
            theme,
            older,
        )
        .into_any_element()
    };
    let row = div()
        .id("chat-load-older")
        .flex()
        .justify_center()
        .p_2()
        .child(control);
    Some(row.into_any_element())
}

/// The native list: the room's intro first once its history is all here,
/// the unread divider over the first unread message, and "No replies yet"
/// under a thread that is only its root.
fn rows(
    chat: &Chat,
    pane: Pane,
    messages: Vec<ChatMessage>,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let room = chat.room.as_ref();
    let lead = pane == Pane::Timeline && room.is_some_and(|room| !room.has_older && !room.landed);
    let lead_text = lead.then(|| beginning(chat)).flatten();
    let bare = pane == Pane::Thread
        && messages.len() == 1
        && room
            .and_then(|room| room.thread.as_ref())
            .is_some_and(|thread| thread.replies.ready().is_some_and(Vec::is_empty));
    let keys = lead
        .then_some("intro".to_owned())
        .into_iter()
        .chain(messages.iter().map(|message| message.id.clone()))
        .chain(bare.then_some("no-replies".to_owned()))
        .collect::<Vec<_>>();
    let state = list_state(chat, pane, &keys);
    state.set_follow_mode(match pane {
        Pane::Timeline => FollowMode::Tail,
        Pane::Thread => FollowMode::Normal,
    });
    state.set_scroll_handler(cx.listener(move |chat, event, _window, cx| {
        chat.list_scrolled(pane, event, cx);
    }));
    let theme = *theme;
    let unread = unread_seq(
        &messages,
        (pane == Pane::Timeline).then_some(chat.reads.boundary),
    );
    let list = gpui_list(
        state,
        cx.processor(move |chat, index: usize, window, cx| {
            if lead && index == 0 {
                let (name, dm) = lead_text.clone().expect("lead row");
                return intro(&name, dm.as_deref(), &theme).into_any_element();
            }
            let Some(message) = messages.get(index - usize::from(lead)).cloned() else {
                return match bare {
                    true => no_replies(&theme).into_any_element(),
                    false => div().into_any_element(),
                };
            };
            let first_unread = unread == Some(message.seq);
            let card = message::card(chat, message, pane, window, cx, &theme);
            if !first_unread {
                return card.into_any_element();
            }
            // full width, as a bare card is: a row shrunk to its words
            // took the hover and the action strip with it
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(unread_marker(&theme))
                .child(card)
                .into_any_element()
        }),
    )
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .flex_1()
    .min_h(px(0.))
    .w_full();
    div()
        .id(match pane {
            Pane::Timeline => "chat-message-list",
            Pane::Thread => "chat-thread-list",
        })
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(list)
}

/// "Jump to latest", when the timeline is not at its live tail.
fn jump_to_latest(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let room = chat.room.as_ref()?;
    if !room.landed && room.at_tail {
        return None;
    }
    let id = room.id.clone();
    let latest = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        cx.notify();
        chat.open(id.clone(), window, cx)
    });
    let jump = div()
        .id("chat-jump-latest")
        .flex()
        .justify_center()
        .p_2()
        .child(super::button(
            "chat-jump-latest-button",
            "Jump to latest",
            theme,
            latest,
        ));
    Some(jump.into_any_element())
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
        let state = ListState::new(keys.len(), alignment, px(UNMEASURED_ROW));
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
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child("No replies yet")
}
