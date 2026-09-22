//! Native GPUI message lists. The room and thread use native variable-height lists;
//! callbacks still call the root view's existing message operations.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    ClickEvent, Context, ElementId, FollowMode, ListAlignment, ListSizingBehavior, ListState,
    ParentElement, Styled, Theme, div, list as gpui_list, px,
};

use crate::ui::message;
use crate::ui::room::selection_bar;
use crate::{Chat, Loaded, Pane};

pub fn list(chat: &Chat, pane: Pane, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let messages = chat.messages(pane);
    let mut content = div()
        .id(ElementId::Name(match pane {
            Pane::Timeline => "chat-timeline".into(),
            Pane::Thread => "chat-thread-messages".into(),
        }))
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
            return content.child(quiet("Loading messages…", theme));
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
            return content.child(quiet(error, theme));
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
        let control = if room.older_loading || chat.session.busy {
            div()
                .id(ElementId::Name("chat-load-older-button".into()))
                .text_color(theme.muted)
                .child(label)
                .into_any_element()
        } else {
            super::button(
                ElementId::Name("chat-load-older-button".into()),
                label,
                theme,
                older,
            )
            .into_any_element()
        };
        content = content.child(
            div()
                .id(ElementId::Name("chat-load-older".into()))
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
        let keys = lead
            .then_some("intro".to_owned())
            .into_iter()
            .chain(messages.iter().map(|message| message.id.clone()))
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
                    return div().into_any_element();
                };
                let unread = unread_seq == Some(message.seq);
                let card = message::card(chat, message, pane_for_items, window, cx, &list_theme);
                if unread {
                    div()
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
                .id(ElementId::Name(match pane {
                    Pane::Timeline => "chat-message-list".into(),
                    Pane::Thread => "chat-thread-list".into(),
                }))
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
                .id(ElementId::Name("chat-jump-latest".into()))
                .flex()
                .justify_center()
                .p_2()
                .child(super::button(
                    ElementId::Name("chat-jump-latest-button".into()),
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
        .id(ElementId::Name("chat-unread-marker".into()))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .text_size(px(12.))
        .text_color(theme.accent_foreground)
        .child(div().h(px(1.)).flex_1().bg(theme.accent))
        .child("New messages")
}

fn intro(name: &str, dm: Option<&str>, theme: &Theme) -> impl IntoElement {
    let (title, detail) = match dm {
        Some(peer) => (
            peer.to_owned(),
            format!("This is the very beginning of your conversation with {peer}."),
        ),
        None => (
            format!("#{name}"),
            format!(
                "This is the very beginning of #{name}. Say hello, or pin what the room is for."
            ),
        ),
    };
    div()
        .id(ElementId::Name("chat-timeline-intro".into()))
        .p_6()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .id("chat-timeline-intro-title")
                .text_size(px(16.))
                .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(1)
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(detail),
        )
        .child(div().h(px(1.)).w_full().bg(theme.border))
}

fn quiet(text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div()
        .p_4()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text.into())
}
