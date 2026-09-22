//! Thread and channel-details panes.

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
        .id(ElementId::Name("chat-thread-pane".into()))
        .w(px(chat.layout.thread))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .child(
            div()
                .id(ElementId::Name("chat-thread-header".into()))
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
                        .text_size(px(13.5))
                        .font_semibold()
                        .role(Role::Heading)
                        .aria_level(2)
                        .child("Thread"),
                )
                .child(button(
                    ElementId::Name("chat-thread-close".into()),
                    "Close thread",
                    theme,
                    close,
                )),
        );
    if let Some(room) = &chat.room
        && let Some(thread_state) = &room.thread
    {
        pane = pane.child(timeline::list(chat, crate::Pane::Thread, cx, theme));
        if thread_state.replies.is_loading() {
            pane = pane.child(
                div()
                    .p_2()
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child("Loading replies…"),
            );
        }
        if chat.may_write() {
            let target = crate::composer::Target::Post {
                channel: room.id.clone(),
                thread: Some(thread_state.root),
            };
            pane = pane.child(room::composer(
                chat,
                target,
                "Reply in thread",
                chat.session.connected && !thread_state.replies.is_loading(),
                cx,
            ));
        }
    } else {
        pane = pane.child(empty_state(
            ElementId::Name("chat-thread-empty".into()),
            "No thread open",
            "Choose a reply from the room.",
            theme,
        ));
    }
    pane
}

pub fn details(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_details();
        cx.notify();
    });
    let Some(details) = chat.details.as_ref() else {
        return div().into_any_element();
    };
    let Some(info) = chat.room_info() else {
        return div().into_any_element();
    };
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
        let text = chat
            .details
            .as_ref()
            .map_or(String::new(), |details| details.member_draft.clone());
        chat.set_member(&text, true, cx);
    });
    let mut content = div()
        .id(ElementId::Name("chat-details-pane".into()))
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
                .id(ElementId::Name("chat-details-header".into()))
                .flex()
                .items_center()
                .child(
                    div()
                        .id("chat-details-title")
                        .flex_1()
                        .text_size(px(13.5))
                        .font_semibold()
                        .role(Role::Heading)
                        .aria_level(2)
                        .child("Channel details"),
                )
                .child(button(
                    ElementId::Name("chat-details-close".into()),
                    "Close",
                    theme,
                    close,
                )),
        )
        .child(div().h(px(1.)).w_full().bg(theme.border))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Name"),
        )
        .child(
            Input::new(ElementId::Name("chat-details-name-input".into()))
                .h(px(28.))
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .value(details.name_draft.clone())
                .label("Channel name")
                .on_input(typed_name),
        )
        .child(button(
            ElementId::Name("chat-details-rename-button".into()),
            "Rename",
            theme,
            rename,
        ))
        .child(button(
            ElementId::Name("chat-details-archive".into()),
            if info.channel.archived {
                "Unarchive channel"
            } else {
                "Archive channel"
            },
            theme,
            archive,
        ))
        .child(div().h(px(1.)).w_full().bg(theme.border))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Members"),
        )
        .child(
            Input::new(ElementId::Name("chat-details-member-input".into()))
                .h(px(28.))
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .value(details.member_draft.clone())
                .label("Add member")
                .on_input(typed_member),
        )
        .child(button(
            ElementId::Name("chat-details-add-member".into()),
            "Add member",
            theme,
            add_member,
        ))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.faint)
                .child("Select a member below to remove it from this channel."),
        );
    let roster = chat.members();
    if roster.is_empty() {
        content = content.child(
            div()
                .id(ElementId::Name("chat-details-no-members".into()))
                .text_size(px(11.))
                .text_color(theme.faint)
                .child("No members added. An open channel needs none."),
        );
    }
    for (index, member) in roster.iter().enumerate() {
        let party = member.key.clone();
        let remove = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_member(&party, false, cx);
        });
        let mut remove_button = div()
            .id(ElementId::named_usize("chat-details-remove", index))
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme.surface)
            .hover(|s| s.bg(theme.surface_raised))
            .child("Remove");
        if !chat.session.busy {
            remove_button = remove_button.on_click(remove);
        }
        let row = div()
            .id(ElementId::named_usize("chat-details-member", index))
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .child(member.label.clone()),
            )
            .child(remove_button);
        content = content.child(row);
    }
    content.into_any_element()
}
