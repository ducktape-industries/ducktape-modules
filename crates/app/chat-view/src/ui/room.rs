//! The open room, search results, notices, and composer.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px};

use super::timeline;
use crate::chat::ChannelInfo;
use crate::composer::Target;
use crate::ui::{badge, button, empty_state};
use crate::{Chat, Loaded, Pane, Room};

pub fn render(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut pane = div()
        .id(ElementId::Name("chat-room".into()))
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground);
    if let Some(room) = &chat.room {
        pane = pane.child(header(chat, room, cx, theme));
        if !chat.notice.is_empty() {
            let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.notice.clear();
                cx.notify();
            });
            pane = pane.child(
                div()
                    .id(ElementId::Name("chat-room-notice".into()))
                    .mx_3()
                    .my_2()
                    .p_2()
                    .rounded_md()
                    .bg(theme.danger_soft)
                    .text_color(theme.danger)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(chat.notice.clone()))
                    .child(button(
                        ElementId::Name("chat-room-notice-dismiss".into()),
                        "Dismiss",
                        theme,
                        dismiss,
                    )),
            );
        }
        if !chat.search.query.is_empty() {
            pane = pane.child(search_results(chat, cx, theme));
        } else {
            pane = pane.child(timeline::list(chat, Pane::Timeline, cx, theme));
        }
        if let Some(info) = chat.room_info() {
            if !info.channel.huddle.is_empty() {
                pane = pane.child(huddle(chat, info, cx, theme));
            }
        }
        let refusal = chat.write_refusal();
        if refusal.is_empty() {
            let target = Target::Post {
                channel: room.id.clone(),
                thread: None,
            };
            let hint = match super::sidebar::dm_peer(chat) {
                Some((peer, _)) => format!("Message {peer}"),
                None => {
                    let name = chat
                        .info(&room.id)
                        .map_or_else(|| short_id(&room.id, 8), |info| info.channel.name.clone());
                    format!("Message #{name}")
                }
            };
            let editable = !chat.session.loading && chat.session.connected;
            pane = pane.child(composer(chat, target, &hint, editable, cx));
        } else {
            pane = pane.child(gate(chat, refusal, cx, theme));
        }
    } else {
        pane = pane.child(no_room(chat, cx, theme));
    }
    pane
}

fn header(chat: &Chat, room: &Room, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let info = chat.info(&room.id);
    let name = info.map_or_else(|| short_id(&room.id, 8), |info| info.channel.name.clone());
    let details = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details();
        cx.notify();
    });
    let mut title = div()
        .id(ElementId::Name("chat-room-title".into()))
        .flex()
        .items_center()
        .gap_2()
        .child(div().text_lg().child(format!("#{}", name)));
    if info.is_some_and(|info| info.channel.archived) {
        title = title.child(badge(
            "chat-room-archived",
            "Archived",
            theme.warning,
            theme.warning_soft,
        ));
    }
    if info.is_some_and(crate::chat::members_only) {
        title = title.child(badge(
            "chat-room-members-only",
            "Members only",
            theme.muted,
            theme.surface_raised,
        ));
    }
    div()
        .id(ElementId::Name("chat-room-header".into()))
        .flex()
        .items_center()
        .gap_2()
        .p_3()
        .border_b_1()
        .border_color(theme.border)
        .child(div().flex_1().child(title))
        .child(button(
            ElementId::Name("chat-room-details".into()),
            "Channel details",
            theme,
            details,
        ))
}

fn no_room(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    match &chat.channels {
        Loaded::Idle | Loaded::Loading(_) => empty_state(
            ElementId::Name("chat-no-room-loading".into()),
            "Loading channels…",
            "Choose a room when they arrive.",
            theme,
        )
        .into_any_element(),
        Loaded::Failed(refusal) => empty_state(
            ElementId::Name("chat-no-room-failed".into()),
            "Couldn’t read the channels",
            refusal.sentence.clone(),
            theme,
        )
        .into_any_element(),
        Loaded::Ready(rooms) if !rooms.is_empty() => empty_state(
            ElementId::Name("chat-no-room".into()),
            "No channel open",
            "Choose a channel from the sidebar.",
            theme,
        )
        .into_any_element(),
        Loaded::Ready(_) => {
            let open = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.create = Some(Default::default());
                cx.notify();
            });
            div()
                .id(ElementId::Name("chat-no-room-empty".into()))
                .flex()
                .flex_col()
                .gap_2()
                .p_6()
                .child(div().text_base().child("No channels"))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted)
                        .child("Create the first channel in this network."),
                )
                .child(button(
                    ElementId::Name("chat-no-room-create".into()),
                    "Create a channel",
                    theme,
                    open,
                ))
                .into_any_element()
        }
    }
    .into_any_element()
}

fn huddle(
    chat: &Chat,
    info: &ChannelInfo,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let mut row = div()
        .id(ElementId::Name("chat-room-huddle".into()))
        .flex()
        .items_center()
        .gap_2()
        .mx_3()
        .my_1()
        .p_2()
        .rounded_md()
        .bg(theme.surface)
        .child(badge(
            "chat-room-huddle-live",
            "Voice",
            theme.success,
            theme.success_soft,
        ))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted)
                .child(format!("{} people", info.channel.huddle.len())),
        );
    if chat.session.huddle_joined && chat.session.huddle_channel == info.channel.id {
        row = row.child(badge(
            "chat-room-huddle-joined",
            "Joined",
            theme.accent_foreground,
            theme.accent_soft,
        ));
        let show = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.host().notify::<crate::api::ShowHuddle>(());
            cx.notify();
        });
        let leave = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.host().notify::<crate::api::LeaveHuddle>(());
            cx.notify();
        });
        row = row
            .child(button(
                ElementId::Name("chat-room-huddle-show".into()),
                "Show",
                theme,
                show,
            ))
            .child(button(
                ElementId::Name("chat-room-huddle-leave".into()),
                "Leave",
                theme,
                leave,
            ));
    } else {
        let channel = info.channel.id.clone();
        let voice = info.channel.voice;
        let join = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            if voice {
                cx.host()
                    .notify::<crate::api::JoinVoice>(serde_json::json!({"id": channel}));
            } else {
                cx.host().notify::<crate::api::JoinHuddle>(());
            }
            cx.notify();
        });
        row = row.child(button(
            ElementId::Name("chat-room-huddle-join".into()),
            if info.channel.huddle.is_empty() {
                "Call"
            } else {
                "Join"
            },
            theme,
            join,
        ));
    }
    row
}

fn search_results(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut content = div()
        .id(ElementId::Name("chat-search-results".into()))
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3();
    match &chat.search.hits {
        Loaded::Idle | Loaded::Loading(_) => {
            content = content.child(quiet("Searching…", theme));
        }
        Loaded::Failed(refusal) => {
            content = content.child(quiet(refusal.sentence.clone(), theme));
        }
        Loaded::Ready(hits) if hits.rows.is_empty() => {
            content = content.child(empty_state(
                ElementId::Name("chat-search-empty".into()),
                "No results",
                "Nothing matched this message search.",
                theme,
            ));
        }
        Loaded::Ready(hits) => {
            content = content.child(div().text_sm().text_color(theme.muted).child(format!(
                "{} result{} for “{}”",
                hits.rows.len(),
                if hits.rows.len() == 1 { "" } else { "s" },
                chat.search.query
            )));
            for row in &hits.rows {
                let id = row.channel_id.clone();
                let seq = row.seq;
                let open = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_hit(id.clone(), seq, window, cx)
                });
                content = content.child(
                    div()
                        .id(ElementId::named_usize("chat-search-hit", seq as usize))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_2()
                        .rounded_md()
                        .bg(theme.surface)
                        .hover(|s| s.bg(theme.surface_raised))
                        .role(ducktape_view_guest::Role::Button)
                        .focusable()
                        .on_click(open)
                        .child(div().text_sm().child(row.text.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted)
                                .child(format!("message {}", row.seq)),
                        ),
                );
            }
            if hits.has_more {
                let more = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                    cx.notify();
                    chat.search_more(cx)
                });
                content = content.child(button(
                    ElementId::Name("chat-search-more".into()),
                    "More results",
                    theme,
                    more,
                ));
            }
        }
    }
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    content.child(button(
        ElementId::Name("chat-search-clear".into()),
        "Clear message search",
        theme,
        clear,
    ))
}

fn gate(chat: &Chat, refusal: &str, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let mut notice = div()
        .id(ElementId::Name("chat-room-write-refusal".into()))
        .mx_3()
        .my_2()
        .p_3()
        .rounded_md()
        .bg(if refusal == "channel_archived" {
            theme.surface
        } else {
            theme.warning_soft
        })
        .text_color(theme.muted)
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(match refusal {
            "channel_archived" => {
                "This channel is archived. It keeps its history and takes no new messages."
            }
            "members_only" => {
                "This channel is members-only and your key is not on its roster. Ask a member to add your key from Channel details."
            }
            _ => "To send messages, create or join an account in Settings → Account. You can read this channel without an account.",
        }));
    if refusal == "channel_archived" {
        let reopen = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_archived(false, cx);
        });
        notice = notice.child(button(
            ElementId::Name("chat-room-unarchive".into()),
            "Unarchive",
            theme,
            reopen,
        ));
    }
    notice.into_any_element()
}

pub fn composer(
    chat: &Chat,
    target: Target,
    hint: &str,
    editable: bool,
    cx: &mut Context<Chat>,
) -> impl IntoElement {
    let key = crate::draft_key(&target);
    let empty = crate::composer::Draft::default();
    let draft = chat.drafts.get(&key).unwrap_or(&empty);
    let choices = chat.mention_choices();
    crate::composer::view(
        draft,
        &key,
        hint,
        editable,
        &choices,
        cx,
        move |chat, event, window, cx| chat.composer(target.clone(), event, window, cx),
    )
}

fn quiet(text: impl Into<String>, theme: &Theme) -> impl IntoElement {
    div().text_sm().text_color(theme.muted).child(text.into())
}

fn short_id(id: &str, keep: usize) -> String {
    let mut head: String = id.chars().take(keep).collect();
    if id.chars().count() > keep {
        head.push('…');
    }
    head
}

pub fn loading(key: String) -> impl IntoElement {
    div()
        .id(ElementId::Name(key.into()))
        .p_6()
        .child("Loading messages…")
}

pub fn selection_bar(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let count = chat.copy_count();
    if count == 0 {
        return div().into_any_element();
    }
    let copy = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.copy_range(cx)
    });
    div()
        .id(ElementId::Name("chat-selection-bar".into()))
        .flex()
        .items_center()
        .gap_2()
        .p_2()
        .bg(theme.accent_soft)
        .child(div().flex_1().child(format!(
            "{count} message{} selected",
            if count == 1 { "" } else { "s" }
        )))
        .child(button(
            ElementId::Name("chat-selection-copy".into()),
            "Copy",
            theme,
            copy,
        ))
        .into_any_element()
}

pub fn composer_key(target: &Target) -> String {
    crate::draft_key(target)
}
