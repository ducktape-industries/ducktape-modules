//! Thread and channel-details panes.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px,
};

use super::timeline;
use crate::Chat;
use crate::ui::room;
use crate::ui::{button, empty_state};

pub fn thread(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_thread();
        cx.notify();
    });
    let mut pane = div()
        .id("chat-thread-pane")
        .w(px(chat.layout.thread))
        .h_full()
        .flex()
        .flex_col()
        // one ground under the header, the replies and the field: the
        // rows are drawn on `background`, so the pane is too
        .bg(theme.background)
        .child(
            div()
                .id("chat-thread-header")
                .flex()
                .items_center()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .id("chat-thread-title")
                        .flex_1()
                        .text_size(design::text::SECTION)
                        .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child("Thread"),
                )
                .child(button("chat-thread-close", "Close thread", theme, close)),
        );
    if let Some(room) = &chat.room
        && let Some(thread_state) = &room.thread
    {
        pane = pane.child(timeline::list(chat, crate::Pane::Thread, cx, theme));
        if thread_state.replies.is_loading() {
            pane = pane.child(
                div()
                    .p_2()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child("Loading replies…"),
            );
        }
        if chat.may_write() {
            let target = crate::composer::Target::Post {
                channel: room.id.clone(),
                thread: Some(thread_state.root),
            };
            pane = pane.child(div().p_3().child(room::composer(
                chat,
                target,
                "Reply in thread",
                chat.session.connected && !thread_state.replies.is_loading(),
                cx,
            )));
        }
    } else {
        pane = pane.child(empty_state(
            "chat-thread-empty",
            "No thread open",
            "Choose a reply from the room.",
            theme,
        ));
    }
    pane
}

/// The open channel's details: its name, archiving, and its members.
pub fn details(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let (Some(details), Some(info)) = (chat.details.as_ref(), chat.room_info()) else {
        return div().into_any_element();
    };
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details();
        cx.notify();
    });
    let typed_name = cx.listener(|chat, event: &String, _window, cx| {
        if let Some(details) = &mut chat.details {
            details.name_draft = event.clone();
        }
        cx.notify();
    });
    let rename = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.rename(cx)
    });
    let archived = info.channel.archived;
    let archive = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.set_archived(!archived, cx);
    });
    let typed_member = cx.listener(|chat, event: &String, _window, cx| {
        if let Some(details) = &mut chat.details {
            details.member_draft = event.clone();
        }
        cx.notify();
    });
    let add_member = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.add_member(cx);
    });
    let archive_label = match archived {
        true => "Unarchive channel",
        false => "Archive channel",
    };
    div()
        .id("chat-details-pane")
        .w(px(chat.layout.details))
        .h_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .bg(theme.surface)
        .child(
            div()
                .id("chat-details-header")
                .flex()
                .items_center()
                .child(
                    div()
                        .id("chat-details-title")
                        .flex_1()
                        .text_size(design::text::SECTION)
                        .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child("Channel details"),
                )
                .child(button("chat-details-close", "Close", theme, close)),
        )
        .child(rule(theme))
        .child(section("Name", theme))
        .child(
            field(
                "chat-details-name-input",
                &details.name_draft,
                "Channel name",
                theme,
            )
            .on_input(typed_name),
        )
        .child(button(
            "chat-details-rename-button",
            "Rename",
            theme,
            rename,
        ))
        .child(button(
            "chat-details-archive",
            archive_label,
            theme,
            archive,
        ))
        .child(rule(theme))
        .child(section("Members", theme))
        .child(
            field(
                "chat-details-member-input",
                &details.member_draft,
                "Add member",
                theme,
            )
            .on_input(typed_member),
        )
        .child(button(
            "chat-details-add-member",
            "Add member",
            theme,
            add_member,
        ))
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.faint)
                .child("Select a member below to remove it from this channel."),
        )
        .children(members(chat, cx, theme))
        .into_any_element()
}

fn rule(theme: &Theme) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(theme.border)
}

fn section(name: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(name)
}

fn field(id: &'static str, value: &str, label: &'static str, theme: &Theme) -> Input {
    Input::new(id)
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .value(value.to_owned())
        .label(label)
}

/// A row per member, each with its way out; a word when there are none.
fn members(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let roster = chat.roster();
    if roster.is_empty() {
        let none = div()
            .id("chat-details-no-members")
            .text_size(design::text::CAPTION)
            .text_color(theme.faint)
            .child("No members added. An open channel needs none.");
        return vec![none.into_any_element()];
    }
    let mut rows = Vec::new();
    for (index, (party, label)) in roster.into_iter().enumerate() {
        let remove = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_member(party.clone(), false, cx);
        });
        let remove = div()
            .id(ElementId::named_usize("chat-details-remove", index))
            .px_2()
            .py_1()
            .bg(theme.surface)
            .hover(|s| s.bg(theme.surface_raised))
            .child("Remove")
            .on_click(remove);
        let row = div()
            .id(ElementId::named_usize("chat-details-member", index))
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_size(design::text::SECONDARY)
                    .child(label),
            )
            .child(remove);
        rows.push(row.into_any_element());
    }
    rows
}
