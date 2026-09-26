//! The open room, search results, notices, and composer.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px,
};

use chat::{ChannelInfo, MsgRow};
use ducktape_view_guest::view::Loadable;

use super::timeline;
use crate::composer::Target;
use crate::session::Gate;
use crate::ui::{badge, button, empty_state, quiet};
use crate::{Chat, Hits, Pane, Room};

pub fn render(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let pane = div()
        .id("chat-room")
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground);
    let Some(room) = &chat.room else {
        return pane.child(no_room(chat, cx, theme));
    };
    let body = match chat.search.query.is_empty() {
        true => timeline::list(chat, Pane::Timeline, cx, theme).into_any_element(),
        false => search_results(chat, cx, theme).into_any_element(),
    };
    let huddled = chat
        .room_info()
        .filter(|info| !info.channel.huddle.is_empty())
        .map(|info| huddle(info, theme));
    pane.child(header(chat, room, cx, theme))
        .children(notice(chat, cx, theme))
        .children(confirmation(chat, cx, theme))
        .child(body)
        .children(huddled)
        .child(compose(chat, room, cx, theme))
}

/// A refused write, until dismissed.
fn notice(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<impl IntoElement> {
    if chat.notice.is_empty() {
        return None;
    }
    let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.notice.clear();
        cx.notify();
    });
    Some(
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
    )
}

/// A write that landed, said once, until dismissed.
fn confirmation(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<impl IntoElement> {
    if chat.confirmation.is_empty() {
        return None;
    }
    let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.confirmation.clear();
        cx.notify();
    });
    Some(
        div()
            .id("chat-room-confirmation")
            .mx_3()
            .my_2()
            .px_2()
            .h(design::size::CONTROL)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_size(design::text::SECONDARY)
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
    )
}

/// The composer, or why the reader may not write here.
fn compose(chat: &Chat, room: &Room, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    if let Some(gate) = chat.write_gate() {
        return write_gate(gate, cx, theme).into_any_element();
    }
    let target = Target::Post {
        channel: room.id.clone(),
        thread: None,
    };
    let editable = chat.session.connected;
    // inset from the pane's edges, so the field reads as a field
    div()
        .p_3()
        .child(composer(chat, target, &hint(chat, room), editable, cx))
        .into_any_element()
}

/// The composer's placeholder: who or where a message goes.
fn hint(chat: &Chat, room: &Room) -> String {
    if let Some((peer, _)) = super::sidebar::dm_peer(chat) {
        return format!("Message {peer}");
    }
    let name = chat.info(&room.id).map_or_else(
        || ducktape_view_guest::design::short_hex(&room.id),
        |info| info.channel.name.clone(),
    );
    match chat::dm_peers(&room.id) {
        Some(_) => format!("Message {name}"),
        None => format!("Message #{name}"),
    }
}

fn header(chat: &Chat, room: &Room, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let info = chat.info(&room.id);
    let name = info.map_or_else(
        || ducktape_view_guest::design::short_hex(&room.id),
        |info| info.channel.name.clone(),
    );
    let details = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details();
        cx.notify();
    });
    // a direct room is a person: their initials and name, no `#`, and no
    // "Members only" (a direct room always is)
    let direct = chat::dm_peers(&room.id).is_some();
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
            .text_size(design::text::SECTION)
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
    if !direct && info.is_some_and(|info| info.channel.members_only()) {
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
            if direct {
                "Conversation details"
            } else {
                "Channel details"
            },
            theme,
            details,
        ))
}

fn no_room(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    match &chat.channels {
        Loadable::Idle | Loadable::Loading(_) => empty_state(
            "chat-no-room-loading",
            "Loading channels…",
            "Choose a room when they arrive.",
            theme,
        )
        .into_any_element(),
        Loadable::Failed(refusal) => empty_state(
            "chat-no-room-failed",
            "Couldn’t read the channels",
            refusal.message.clone(),
            theme,
        )
        .into_any_element(),
        Loadable::Ready(rooms) if !rooms.is_empty() => empty_state(
            "chat-no-room",
            "No channel open",
            "Choose a channel from the sidebar.",
            theme,
        )
        .into_any_element(),
        Loadable::Ready(_) => {
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
                .child(div().text_size(design::text::BODY).child("No channels"))
                .child(
                    div()
                        .text_size(design::text::SECONDARY)
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
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(format!("{} people", info.channel.huddle.len())),
        )
}

fn search_results(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let found = match &chat.search.hits {
        Loadable::Idle | Loadable::Loading(_) => {
            vec![quiet("Searching…", theme).into_any_element()]
        }
        Loadable::Failed(refusal) => vec![quiet(refusal.message.clone(), theme).into_any_element()],
        Loadable::Ready(hits) if hits.rows.is_empty() => vec![
            empty_state(
                "chat-search-empty",
                "No results",
                "Nothing matched this message search.",
                theme,
            )
            .into_any_element(),
        ],
        Loadable::Ready(hits) => hit_list(chat, hits, cx, theme),
    };
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_clear();
        cx.notify();
    });
    div()
        .id("chat-search-results")
        .flex_1()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .children(found)
        .child(button(
            "chat-search-clear",
            "Clear message search",
            theme,
            clear,
        ))
}

/// How many hits, a row per hit, and the way to more.
fn hit_list(chat: &Chat, hits: &Hits, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let count = design::plural(hits.rows.len() as u64, "result", "results");
    let mut list = vec![
        div()
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(format!("{count} for “{}”", chat.search.query))
            .into_any_element(),
    ];
    list.extend(hits.rows.iter().map(|row| hit(row, cx, theme)));
    if hits.has_more {
        let more = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.search_more(cx)
        });
        list.push(button("chat-search-more", "More results", theme, more).into_any_element());
    }
    list
}

/// One hit: its text and where it sits, opening the room at it.
fn hit(row: &MsgRow, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let id = row.channel_id.clone();
    let seq = row.seq;
    let open = cx.listener(move |chat, _: &ClickEvent, window, cx| {
        cx.notify();
        chat.open_hit(id.clone(), seq, window, cx)
    });
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
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .child(row.text.clone()),
        )
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(format!("message {}", row.seq)),
        )
        .into_any_element()
}

/// Where the composer would be, why the reader may not write here.
fn write_gate(gate: Gate, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let why = match gate {
        Gate::Archived => {
            "This channel is archived. It keeps its history and takes no new messages."
        }
        Gate::NotMember => {
            "This channel is members-only and your key is not on its roster. Ask a member to add your key from Channel details."
        }
        Gate::NoAccount => {
            "To send messages, create or join an account in Settings → Account. You can read this channel without an account."
        }
    };
    let mut notice = div()
        .id("chat-room-write-refusal")
        .mx_3()
        .my_2()
        .p_3()
        .bg(match gate {
            Gate::Archived => theme.surface,
            Gate::NotMember | Gate::NoAccount => theme.warning_soft,
        })
        .text_color(theme.muted)
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(why));
    if gate == Gate::Archived {
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
    let key = target.key();
    let empty = crate::composer::Draft::default();
    let draft = chat.drafts.get(&key).unwrap_or(&empty);
    let choices = chat.mention_choices();
    let commit = match target {
        Target::Post { .. } => "Send",
        Target::Edit { .. } => "Save",
    };
    crate::composer::view::<Chat>(
        draft,
        &key,
        hint,
        commit,
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
            "{} selected",
            design::plural(count as u64, "message", "messages")
        )))
        .child(button("chat-selection-copy", "Copy", theme, copy))
        .into_any_element()
}
