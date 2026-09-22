//! Native GPUI message lists. The room and thread use the same uniform list;
//! callbacks still call the root view's existing message operations.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px, uniform_list,
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
    if let Some(room) = &chat.room {
        if matches!(pane, Pane::Timeline) && room.has_older && !room.landed {
            let older = cx.listener(|chat, _: &ClickEvent, _window, cx| chat.load_older(cx));
            content = content.child(
                div()
                    .id(ElementId::Name("chat-load-older".into()))
                    .flex()
                    .justify_center()
                    .p_2()
                    .child(super::button(
                        ElementId::Name("chat-load-older-button".into()),
                        if room.older_loading {
                            "Loading older messages…"
                        } else {
                            "Load older messages"
                        },
                        theme,
                        older,
                    )),
            );
        }
    }
    if !messages.is_empty() {
        if matches!(pane, Pane::Timeline)
            && chat
                .room
                .as_ref()
                .is_some_and(|room| !room.has_older && !room.landed)
        {
            let room = chat.room.as_ref().expect("timeline room");
            let name = chat
                .info(&room.id)
                .map_or_else(|| room.id.clone(), |info| info.channel.name.clone());
            content = content.child(intro(
                &name,
                super::sidebar::dm_peer(chat)
                    .as_ref()
                    .map(|peer| peer.0.as_str()),
                theme,
            ));
        }
        let count = messages.len();
        let pane_for_items = pane;
        let list_theme = theme.clone();
        let list = uniform_list(
            ElementId::Name(match pane {
                Pane::Timeline => "chat-message-list".into(),
                Pane::Thread => "chat-thread-list".into(),
            }),
            count,
            cx.processor(move |chat, range, window, cx| {
                chat.messages(pane_for_items)
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| range.contains(index))
                    .map(|(_, message)| {
                        message::card(chat, message, pane_for_items, window, cx, &list_theme)
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        );
        content = content.child(list);
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
        let latest =
            cx.listener(move |chat, _: &ClickEvent, window, cx| chat.open(id.clone(), window, cx));
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
        .child(div().text_lg().child(title))
        .child(div().text_sm().text_color(theme.muted).child(detail))
        .child(div().h(px(1.)).w_full().bg(theme.border))
}

fn quiet(text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div()
        .p_4()
        .text_sm()
        .text_color(theme.muted)
        .child(text.into())
}
