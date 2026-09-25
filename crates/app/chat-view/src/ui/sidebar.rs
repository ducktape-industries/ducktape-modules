//! The channel and direct-message pane, authored as native GPUI elements.

use ducktape_view_guest::AnyElement;
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px};

use chat::view::Names;
use chat::{ChannelInfo, Principal};

use crate::names::dm_peer_of;
use crate::{ChannelCreate, Chat};

pub fn render(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-sidebar")
        .flex()
        .flex_col()
        .w(px(chat.layout.sidebar))
        .h_full()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .child(
            div()
                .id("chat-sidebar-search-row")
                .flex()
                .items_center()
                .gap_1()
                .p_2()
                .child(search(chat, cx, theme)),
        )
        .child(rooms(chat, cx, theme))
}

/// The message search field, and its clear button once it holds a search.
fn search(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let typed = cx.listener(|chat, event: &String, _window, cx| {
        chat.search.draft = event.clone();
        cx.notify();
    });
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.search_submit(cx)
    });
    let input = Input::new("chat-sidebar-search")
        .h(design::size::CONTROL)
        .flex_1()
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.sidebar_border)
        .bg(theme.sidebar_raised)
        .text_color(theme.sidebar_foreground)
        .value(chat.search.draft.clone())
        .placeholder("Search messages…")
        .label("Search messages")
        .on_input(typed)
        .on_submit(submit);
    let searching = !chat.search.query.is_empty() || !chat.search.draft.trim().is_empty();
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    div()
        .flex()
        .items_center()
        .gap_1()
        .flex_1()
        .child(input)
        .when(searching, |el| {
            el.child(
                div()
                    .id("chat-sidebar-clear-search")
                    .px_1()
                    .role(ducktape_view_guest::Role::Button)
                    .focusable()
                    .on_click(clear)
                    .child("✕"),
            )
        })
}

/// The rooms in three sections: channels, voice rooms, direct messages.
/// A program's own rooms (`forge:…` review threads) are its to show.
fn rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut list = div()
        .id("chat-sidebar-rooms")
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .child(section_header(
            "chat-sidebar-channels-header",
            "Channels",
            new_channel(chat, cx, theme),
            theme,
        ));
    let channels: Vec<&ChannelInfo> = chat.channels.ready().into_iter().flatten().collect();
    if chat.channels.is_loading() && channels.is_empty() {
        list = list.child(quiet("chat-sidebar-loading", "Loading rooms…", theme));
    }
    if let Some(refusal) = chat.channels.failed() {
        list = list.child(quiet(
            "chat-sidebar-failed",
            refusal.sentence.clone(),
            theme,
        ));
    }
    let open = chat.room.as_ref().map(|room| room.id.as_str());
    let mine = chat.my_account();
    let mut voice = Vec::new();
    let mut dms = Vec::new();
    for info in channels {
        let id = info.channel.id.as_str();
        if chat::module_of(id).is_some() {
            continue;
        }
        if chat::dm_peers(id).is_some() {
            dms.extend(
                mine.and_then(|mine| dm_peer_of(mine, id))
                    .map(|peer| (info, peer)),
            );
        } else if info.channel.voice {
            voice.push(info);
        } else {
            list = list.child(channel_button(chat, info, open == Some(id), cx, theme));
        }
    }
    if !voice.is_empty() {
        list = list.child(section_header(
            "chat-sidebar-voice-header",
            "Voice",
            div(),
            theme,
        ));
        for info in voice {
            list = list.child(voice_button(chat, info, cx, theme));
        }
    }
    if !dms.is_empty() {
        list = list.child(section_header(
            "chat-sidebar-dm-header",
            "Direct messages",
            div(),
            theme,
        ));
        for (info, peer) in dms {
            let selected = open == Some(info.channel.id.as_str());
            list = list.child(dm_button(chat, info, peer, selected, cx, theme));
        }
    }
    let roster_more = chat.names.ready().is_some_and(Names::more);
    if chat.channels_more || roster_more {
        list = list.child(design::more_not_shown("chat-sidebar-more", theme).px_2());
    }
    list
}

/// "+ New channel", or "✕ Close" while the create dialog is open.
fn new_channel(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = match chat.create.take() {
            Some(_) => None,
            None => Some(ChannelCreate::default()),
        };
        cx.notify();
    });
    div()
        .id("chat-sidebar-new-channel")
        .px_1()
        .py_0p5()
        .hover(|s| s.bg(theme.sidebar_raised))
        .role(ducktape_view_guest::Role::Button)
        .focusable()
        .on_click(toggle)
        .child(if chat.create.is_some() {
            "✕ Close"
        } else {
            "+ New channel"
        })
}

fn section_header(
    id: impl Into<ElementId>,
    label: &str,
    control: impl IntoElement,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .py_1()
        .text_size(design::text::CAPTION)
        .text_color(theme.sidebar_muted)
        .child(div().flex_1().child(label.to_owned()))
        .child(control)
}

fn quiet(id: impl Into<ElementId>, text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .px_1()
        .py_1()
        .text_size(design::text::CAPTION)
        .text_color(theme.sidebar_muted)
        .child(text.into())
}

fn channel_button(
    chat: &Chat,
    info: &ChannelInfo,
    selected: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = info.channel.id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        cx.notify();
        chat.choose(id.clone(), window, cx)
    });
    let unread = chat.unread(info) && !selected;
    let mut row = div()
        .id(format!("chat-sidebar-channel-{}", info.channel.id))
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .hover(|s| s.bg(theme.sidebar_raised))
        .role(ducktape_view_guest::Role::Button)
        .aria_selected(selected)
        .focusable()
        .on_click(click)
        .child(div().text_color(theme.sidebar_muted).child("#"))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if unread {
                    theme.sidebar_foreground
                } else {
                    theme.sidebar_muted
                })
                .child(info.channel.name.clone()),
        );
    if !info.channel.huddle.is_empty() {
        row = row.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.success)
                .child(format!("🔊 {}", info.channel.huddle.len())),
        );
    }
    if info.channel.members_only() {
        row = row.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.sidebar_muted)
                .child("Members only"),
        );
    }
    if info.channel.archived {
        row = row.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.sidebar_muted)
                .child("Archived"),
        );
    }
    if unread {
        row = row.child(
            div()
                .id(format!("chat-sidebar-channel-{}-unread", info.channel.id))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    with_seats(chat, info, row, theme)
}

fn voice_button(
    chat: &Chat,
    info: &ChannelInfo,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = info.channel.id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        chat.open(id.clone(), window, cx);
    });
    let selected = chat
        .room
        .as_ref()
        .is_some_and(|room| room.id == info.channel.id);
    let row = div()
        .id(format!("chat-sidebar-voice-{}", info.channel.id))
        .flex()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .hover(|s| s.bg(theme.sidebar_raised))
        .when(!info.channel.archived, |el| {
            el.role(ducktape_view_guest::Role::Button)
                .focusable()
                .on_click(click)
        })
        .child("🔊")
        .child(div().flex_1().child(info.channel.name.clone()))
        .when(info.channel.archived, |el| {
            el.child(quiet("archived", "Archived", theme))
        });
    with_seats(chat, info, row, theme)
}

fn with_seats(chat: &Chat, info: &ChannelInfo, row: impl IntoElement, theme: &Theme) -> AnyElement {
    let mut content = div()
        .id(format!("chat-sidebar-seats-{}", info.channel.id))
        .flex()
        .flex_col()
        .child(row);
    let Some(names) = chat.names.ready() else {
        return content.into_any_element();
    };
    let me = chat.me();
    for (index, seat) in info.channel.huddle.iter().enumerate() {
        let label = names.member(&seat.principal);
        let is_you = Some(&seat.principal) == me.as_ref();
        let speaking = false;
        let note = if is_you { "you" } else { "" };
        content = content.child(
            div()
                .id(ElementId::named_usize("chat-sidebar-seat", index))
                .flex()
                .items_center()
                .gap_1()
                .pl_7()
                .py_0p5()
                .child(
                    design::avatar(&label, design::size::AVATAR, theme)
                        .bg(if speaking {
                            theme.success_soft
                        } else {
                            theme.sidebar_raised
                        })
                        .text_size(design::text::CAPTION)
                        .text_color(if speaking {
                            theme.success
                        } else {
                            theme.sidebar_muted
                        }),
                )
                .child(div().flex_1().text_size(design::text::CAPTION).child(label))
                .when(!note.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(design::text::CAPTION)
                            .text_color(theme.sidebar_muted)
                            .child(note),
                    )
                }),
        );
    }
    content.into_any_element()
}

fn dm_button(
    chat: &Chat,
    info: &ChannelInfo,
    peer: u64,
    selected: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let names = chat.names.ready();
    let name = names.map_or_else(
        || format!("account {peer}"),
        |n| n.member(&Principal::Account(peer)),
    );
    let agent = names.is_some_and(|n| n.is_program(peer));
    let unread = chat.unread(info) && !selected;
    let id = info.channel.id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        cx.notify();
        chat.choose(id.clone(), window, cx)
    });
    let mut row = div()
        .id(format!("chat-sidebar-dm-{peer}"))
        .flex()
        .items_center()
        .gap_1()
        .min_h(design::size::CONTROL)
        .px_1()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .hover(|s| s.bg(theme.sidebar_raised))
        .role(ducktape_view_guest::Role::Button)
        .focusable()
        .on_click(click)
        .child(avatar(
            format!("chat-sidebar-dm-{peer}-avatar"),
            &name,
            agent,
            theme.sidebar_raised,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if unread {
                    theme.sidebar_foreground
                } else {
                    theme.sidebar_muted
                })
                .child(name),
        );
    if agent {
        row = row.child(super::badge(
            format!("chat-sidebar-dm-{peer}-agent"),
            "Agent",
            theme.agent,
            theme.agent_soft,
        ));
    }
    if unread {
        row = row.child(
            div()
                .id(format!("chat-sidebar-dm-{peer}-unread"))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    row
}

/// A person's round initials: a direct room's face, in the sidebar and
/// over the room.
pub fn avatar(
    id: impl Into<ElementId>,
    name: &str,
    agent: bool,
    fill: ducktape_view_guest::Hsla,
    theme: &Theme,
) -> impl IntoElement {
    design::avatar(name, design::size::AVATAR_LG, theme)
        .id(id)
        .bg(if agent { theme.agent_soft } else { fill })
        .text_color(theme.foreground)
        .text_size(design::text::CAPTION)
}

pub fn dm_peer(chat: &Chat) -> Option<(String, bool)> {
    let room = chat.room.as_ref()?;
    let names = chat.names.ready()?;
    let peer = dm_peer_of(chat.my_account()?, &room.id)?;
    Some((
        names.member(&Principal::Account(peer)),
        names.is_program(peer),
    ))
}
