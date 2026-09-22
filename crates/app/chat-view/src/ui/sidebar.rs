//! The channel and direct-message pane, authored as native GPUI elements.

use ducktape_view_guest::AnyElement;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px};

use crate::chat::ChannelInfo;
use crate::{ChannelCreate, Chat};

pub fn render(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let typed = cx.listener(|chat, event: &String, _window, cx| {
        chat.search.draft = event.clone();
        cx.notify();
    });
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.search_submit(cx)
    });
    let search_input = Input::new(ElementId::Name("chat-sidebar-search".into()))
        .h(px(28.))
        .flex_1()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.sidebar_border)
        .bg(theme.sidebar_raised)
        .text_color(theme.sidebar_foreground)
        .value(chat.search.draft.clone())
        .placeholder("Search messages…")
        .label("Search messages")
        .on_input(typed)
        .on_submit(submit);
    let mut search = div().flex().items_center().gap_1().flex_1().child(search_input);
    if !chat.search.query.is_empty() || !chat.search.draft.trim().is_empty() {
        let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            chat.search_clear();
            cx.notify();
        });
        search = search.child(
            div()
                .id(ElementId::Name("chat-sidebar-clear-search".into()))
                .px_1()
                .role(ducktape_view_guest::Role::Button)
                .focusable()
                .on_click(clear)
                .child("✕"),
        );
    }

    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = match chat.create.take() {
            Some(_) => None,
            None => Some(ChannelCreate::default()),
        };
        cx.notify();
    });
    let mut content = div()
        .id(ElementId::Name("chat-sidebar".into()))
        .flex()
        .flex_col()
        .w(px(chat.layout.sidebar))
        .h_full()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .child(
            div()
                .id(ElementId::Name("chat-sidebar-search-row".into()))
                .flex()
                .items_center()
                .gap_1()
                .p_2()
                .child(search),
        );

    let busy = chat.session.loading || chat.session.busy;
    let door = div()
        .id(ElementId::Name("chat-sidebar-new-channel".into()))
        .px_1()
        .py_0p5()
        .rounded_sm()
        .hover(|s| s.bg(theme.sidebar_raised))
        .when(!busy || chat.create.is_some(), |el| {
            el.role(ducktape_view_guest::Role::Button)
                .focusable()
                .on_click(toggle)
        })
        .child(if chat.create.is_some() {
            "✕ Close"
        } else {
            "+ New channel"
        });
    let mut list = div()
        .id(ElementId::Name("chat-sidebar-rooms".into()))
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2();
    list = list.child(section_header(
        "chat-sidebar-channels-header",
        "Channels",
        door,
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
        if crate::chat::dm_peers(&info.channel.id).is_some() {
            if let Some(peer) =
                mine.and_then(|mine| crate::client::dm_peer_of(mine, &info.channel.id))
            {
                dms.push((info, peer));
            }
        } else if info.channel.voice {
            voice.push(info);
        } else {
            list = list.child(channel_button(
                chat,
                info,
                open == Some(info.channel.id.as_str()),
                cx,
                theme,
            ));
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
            list = list.child(dm_button(
                chat,
                info,
                peer,
                open == Some(info.channel.id.as_str()),
                cx,
                theme,
            ));
        }
    }
    content.child(list)
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
        .text_xs()
        .text_color(theme.sidebar_muted)
        .child(div().flex_1().child(label.to_owned()))
        .child(control)
}

fn quiet(id: impl Into<ElementId>, text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .px_1()
        .py_1()
        .text_xs()
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
        .id(ElementId::Name(
            format!("chat-sidebar-channel-{}", info.channel.id).into(),
        ))
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .min_h(px(28.))
        .px_1()
        .rounded_md()
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
                .text_xs()
                .text_color(theme.success)
                .child(format!("🔊 {}", info.channel.huddle.len())),
        );
    }
    if crate::chat::members_only(info) {
        row = row.child(
            div()
                .text_xs()
                .text_color(theme.sidebar_muted)
                .child("Members only"),
        );
    }
    if info.channel.archived {
        row = row.child(
            div()
                .text_xs()
                .text_color(theme.sidebar_muted)
                .child("Archived"),
        );
    }
    if unread {
        row = row.child(
            div()
                .id(ElementId::Name(
                    format!("chat-sidebar-channel-{}-unread", info.channel.id).into(),
                ))
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
    let click = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.host()
            .notify::<crate::api::JoinVoice>(serde_json::json!({"id": id}));
        cx.notify();
        chat.notice.clear();
    });
    let selected = chat.session.huddle_joined && chat.session.huddle_channel == info.channel.id;
    let row = div()
        .id(ElementId::Name(
            format!("chat-sidebar-voice-{}", info.channel.id).into(),
        ))
        .flex()
        .items_center()
        .gap_1()
        .min_h(px(28.))
        .px_1()
        .rounded_md()
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
        .id(ElementId::Name(
            format!("chat-sidebar-seats-{}", info.channel.id).into(),
        ))
        .flex()
        .flex_col()
        .child(row);
    let Some(names) = chat.names.ready() else {
        return content.into_any_element();
    };
    let me = chat.me_key();
    for (index, seat) in info.channel.huddle.iter().enumerate() {
        let label = names.member_label(&seat.party);
        let is_you = names.owns_handle(&seat.party, &me);
        let speaking = if is_you {
            chat.session.call_speaking
        } else {
            chat.session
                .call_peers
                .iter()
                .any(|peer| peer.peer == seat.node && peer.speaking && !peer.muted)
        };
        let note = if is_you {
            if chat.session.call_muted {
                "you · muted"
            } else {
                "you"
            }
        } else {
            ""
        };
        content = content.child(
            div()
                .id(ElementId::named_usize("chat-sidebar-seat", index))
                .flex()
                .items_center()
                .gap_1()
                .pl_7()
                .py_0p5()
                .child(
                    div()
                        .size_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(if speaking {
                            theme.success_soft
                        } else {
                            theme.sidebar_raised
                        })
                        .text_xs()
                        .text_color(if speaking {
                            theme.success
                        } else {
                            theme.sidebar_muted
                        })
                        .child(initials(&label)),
                )
                .child(div().flex_1().text_xs().child(label))
                .when(!note.is_empty(), |el| {
                    el.child(div().text_xs().text_color(theme.sidebar_muted).child(note))
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
        |n| n.member_label(&format!("acct:{peer}")),
    );
    let agent = names.is_some_and(|n| n.is_program(peer));
    let unread = chat.unread(info) && !selected;
    let id = info.channel.id.clone();
    let click = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        cx.notify();
        chat.choose(id.clone(), window, cx)
    });
    let mut row = div()
        .id(ElementId::Name(format!("chat-sidebar-dm-{peer}").into()))
        .flex()
        .items_center()
        .gap_1()
        .min_h(px(28.))
        .px_1()
        .rounded_md()
        .bg(if selected {
            theme.sidebar_raised
        } else {
            theme.sidebar
        })
        .hover(|s| s.bg(theme.sidebar_raised))
        .role(ducktape_view_guest::Role::Button)
        .focusable()
        .on_click(click)
        .child(
            div()
                .size_6()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(if agent {
                    theme.agent_soft
                } else {
                    theme.sidebar_raised
                })
                .text_xs()
                .child(initials(&name)),
        )
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
            ElementId::Name(format!("chat-sidebar-dm-{peer}-agent").into()),
            "Agent",
            theme.agent,
            theme.agent_soft,
        ));
    }
    if unread {
        row = row.child(
            div()
                .id(ElementId::Name(
                    format!("chat-sidebar-dm-{peer}-unread").into(),
                ))
                .size_2()
                .rounded_full()
                .bg(theme.accent),
        );
    }
    row
}

fn initials(name: &str) -> String {
    let mut chars = name
        .split_whitespace()
        .filter_map(|word| word.chars().next());
    let first = chars.next();
    let second = chars.next();
    match (first, second) {
        (Some(first), Some(second)) => format!("{first}{second}").to_uppercase(),
        _ => name.chars().take(2).collect::<String>().to_uppercase(),
    }
}

pub fn dm_peer(chat: &Chat) -> Option<(String, bool)> {
    let room = chat.room.as_ref()?;
    let names = chat.names.ready()?;
    let peer = crate::client::dm_peer_of(chat.my_account()?, &room.id)?;
    Some((
        names.member_label(&format!("acct:{peer}")),
        names.is_program(peer),
    ))
}
