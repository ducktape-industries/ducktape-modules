//! The channel creation dialog.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{AnyElement, ClickEvent, Context, ParentElement, Styled, Theme, div, px};

use crate::Chat;

pub fn channel_create(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let create = chat.create.as_ref()?;
    let busy = create.busy;
    let can_submit = !busy && chat.session.connected && chat.holds_account();
    let typed = cx.listener(|chat, event: &String, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.name = event.clone();
        }
        cx.notify();
    });
    let submit = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.create_channel(cx)
    });
    let voice = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.voice = !create.voice;
        }
        cx.notify();
    });
    let members = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        if let Some(create) = &mut chat.create
            && !create.voice
        {
            create.members_only = !create.members_only;
        }
        cx.notify();
    });
    let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = None;
        cx.notify();
    });
    let mut name = Input::new("chat-create-name")
        .h(px(28.))
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .value(create.name.clone())
        .placeholder("Channel name")
        .label("Channel name")
        .disabled(busy)
        .on_input(typed);
    if can_submit {
        name = name.on_submit(cx.listener(|chat, _: &(), _window, cx| {
            cx.notify();
            chat.create_channel(cx)
        }));
    }
    let mut card = div()
        .id("chat-create-card")
        .max_w(px(480.))
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(
            div()
                .text_size(design::text::TITLE)
                .child("Create a channel"),
        )
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("Channel name"),
        )
        .child(name)
        .child(
            design::button(
                "chat-create-voice",
                if create.voice {
                    "Voice room: On"
                } else {
                    "Voice room: Off"
                },
                theme,
                voice,
            )
            .enabled(!busy),
        )
        .child(
            design::button(
                "chat-create-members",
                if create.members_only {
                    "Members only: On"
                } else {
                    "Members only: Off"
                },
                theme,
                members,
            )
            .enabled(!busy && !create.voice),
        );
    if !create.error.is_empty() {
        card = card.child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(create.error.clone()),
        );
    }
    if !chat.holds_account() {
        card = card.child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("Create an account to create a channel"),
        );
    }
    card = card
        .child(design::button("chat-create-cancel", "Cancel", theme, cancel).enabled(!busy))
        .child(
            design::button("chat-create-submit", "Create channel", theme, submit)
                .enabled(can_submit),
        );
    Some(card.into_any_element())
}
