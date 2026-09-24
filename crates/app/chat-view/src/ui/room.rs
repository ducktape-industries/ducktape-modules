//! The open room, search results, notices, and composer.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px};

use super::timeline;
use crate::chat::ChannelInfo;
use crate::composer::Target;
use crate::ui::{badge, button, empty_state, quiet};
use crate::{Chat, Loaded, Pane, Room};

pub fn render(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut pane = div()
        .id("chat-room")
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
                    .id("chat-room-notice")
                    .mx_3()
                    .my_2()
                    .p_2()
                    .bg(theme.danger_soft)
                    .text_color(theme.danger)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(chat.notice.clone()))
                    .child(button(
                        "chat-room-notice-dismiss",
                        "Dismiss",
                        theme,
                        dismiss,
                    )),
            );
        }
        if !chat.confirmation.is_empty() {
            let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.confirmation.clear();
                cx.notify();
            });
            pane = pane.child(
                div()
                    .id("chat-room-confirmation")
                    .mx_3()
                    .my_2()
                    .px_2()
                    .h(px(30.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child(chat.confirmation.clone()))
                    .child(
                        div()
                            .id("chat-room-confirmation-dismiss")
                            .px_1()
                            .text_color(theme.muted)
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme.foreground))
                            .role(ducktape_view_guest::Role::Button)
                            .aria_label("Dismiss")
                            .focusable()
                            .on_click(dismiss)
                            .child("✕"),
                    ),
            );
        }
        if !chat.search.query.is_empty() {
            pane = pane.child(search_results(chat, cx, theme));
        } else {
            pane = pane.child(timeline::list(chat, Pane::Timeline, cx, theme));
        }
        if let Some(info) = chat.room_info()
            && !info.channel.huddle.is_empty()
        {
            pane = pane.child(huddle(info, theme));
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
                    let name = chat.info(&room.id).map_or_else(
                        || crate::client::short_id(&room.id, 8),
                        |info| info.channel.name.clone(),
                    );
                    match crate::chat::dm_peers(&room.id) {
                        Some(_) => format!("Message {name}"),
                        None => format!("Message #{name}"),
                    }
                }
            };
            let editable = chat.session.connected;
            // inset from the pane's edges, so the field reads as a field
            pane = pane.child(
                div()
                    .p_3()
                    .child(composer(chat, target, &hint, editable, cx)),
            );
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
    let name = info.map_or_else(
        || crate::client::short_id(&room.id, 8),
        |info| info.channel.name.clone(),
    );
    let details = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details();
        cx.notify();
    });
    // a direct room is a person: their initials and name, no `#`, and no
    // "Members only" (a direct room always is)
    let direct = crate::chat::dm_peers(&room.id).is_some();
    let peer = super::sidebar::dm_peer(chat);
    let mut title = div().flex().items_center().gap_2();
    if direct {
        let (name, agent) = peer.clone().unwrap_or((name.clone(), false));
        title = title.child(super::sidebar::avatar(
            "chat-room-avatar",
            &name,
            agent,
            theme.surface_raised,
            theme,
        ));
    }
    title = title.child(
        div()
            .id("chat-room-title")
            .text_size(px(13.5))
            .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
            .role(Role::Heading)
            .aria_level(2)
            .child(match (direct, peer) {
                (true, Some((peer, _))) => peer,
                (true, None) => name,
                (false, _) => format!("#{name}"),
            }),
    );
    if info.is_some_and(|info| info.channel.archived) {
        title = title.child(badge(
            "chat-room-archived",
            "Archived",
            theme.warning,
            theme.warning_soft,
        ));
    }
    if !direct && info.is_some_and(crate::chat::members_only) {
        title = title.child(badge(
            "chat-room-members-only",
            "Members only",
            theme.muted,
            theme.surface_raised,
        ));
    }
    div()
        .id("chat-room-header")
        .flex()
        .items_center()
        .gap_2()
        .p_3()
        .border_b_1()
        .border_color(theme.border)
        .child(div().flex_1().child(title))
        .child(button(
            "chat-room-details",
            if direct { "Details" } else { "Channel details" },
            theme,
            details,
        ))
}

fn no_room(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    match &chat.channels {
        Loaded::Idle | Loaded::Loading(_) => empty_state(
            "chat-no-room-loading",
            "Loading channels…",
            "Choose a room when they arrive.",
            theme,
        )
        .into_any_element(),
        Loaded::Failed(refusal) => empty_state(
            "chat-no-room-failed",
            "Couldn’t read the channels",
            refusal.sentence.clone(),
            theme,
        )
        .into_any_element(),
        Loaded::Ready(rooms) if !rooms.is_empty() => empty_state(
            "chat-no-room",
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
                .id("chat-no-room-empty")
                .flex()
                .flex_col()
                .gap_2()
                .p_6()
                .child(div().text_size(px(13.)).child("No channels"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted)
                        .child("Create the first channel in this network."),
                )
                .child(button(
                    "chat-no-room-create",
                    "Create a channel",
                    theme,
                    open,
                ))
                .into_any_element()
        }
    }
    .into_any_element()
}

fn huddle(info: &ChannelInfo, theme: &Theme) -> impl IntoElement {
    div()
        .id("chat-room-huddle")
        .flex()
        .items_center()
        .gap_2()
        .mx_3()
        .my_1()
        .p_2()
        .bg(theme.surface)
        .child(badge(
            "chat-room-huddle-live",
            "Voice",
            theme.success,
            theme.success_soft,
        ))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(format!("{} people", info.channel.huddle.len())),
        )
}

fn search_results(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut content = div()
        .id("chat-search-results")
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
                "chat-search-empty",
                "No results",
                "Nothing matched this message search.",
                theme,
            ));
        }
        Loaded::Ready(hits) => {
            content = content.child(div().text_size(px(12.)).text_color(theme.muted).child(
                format!(
                    "{} result{} for “{}”",
                    hits.rows.len(),
                    if hits.rows.len() == 1 { "" } else { "s" },
                    chat.search.query
                ),
            ));
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
                        .bg(theme.surface)
                        .hover(|s| s.bg(theme.surface_raised))
                        .role(ducktape_view_guest::Role::Button)
                        .focusable()
                        .on_click(open)
                        .child(div().text_size(px(12.)).child(row.text.clone()))
                        .child(
                            div()
                                .text_size(px(11.))
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
                content = content.child(button("chat-search-more", "More results", theme, more));
            }
        }
    }
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    content.child(button(
        "chat-search-clear",
        "Clear message search",
        theme,
        clear,
    ))
}

fn gate(_chat: &Chat, refusal: &str, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let mut notice = div()
        .id("chat-room-write-refusal")
        .mx_3()
        .my_2()
        .p_3()
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
        notice = notice.child(button("chat-room-unarchive", "Unarchive", theme, reopen));
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
    crate::composer::view::<Chat>(
        draft,
        &key,
        hint,
        editable,
        &choices,
        cx,
        move |chat, event, window, cx| chat.composer(target.clone(), event, window, cx),
    )
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
        .id("chat-selection-bar")
        .flex()
        .items_center()
        .gap_2()
        .p_2()
        .bg(theme.accent_soft)
        .child(div().flex_1().child(format!(
            "{count} message{} selected",
            if count == 1 { "" } else { "s" }
        )))
        .child(button("chat-selection-copy", "Copy", theme, copy))
        .into_any_element()
}
