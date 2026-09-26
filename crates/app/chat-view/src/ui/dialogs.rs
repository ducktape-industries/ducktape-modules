//! The channel creation dialog.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{AnyElement, ClickEvent, Context, ParentElement, Styled, Theme, div, px};

use crate::{ChannelCreate, Chat};

/// How wide the dialog's card grows.
const CARD_MAX_WIDTH: f32 = 480.;

pub fn channel_create(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let create = chat.create.as_ref()?;
    let busy = create.busy;
    let can_submit = !busy && chat.session.connected && chat.holds_account();
    let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.create = None;
        cx.notify();
    });
    let submit = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.create_channel(cx)
    });
    let card = div()
        .id("chat-create-card")
        .max_w(px(CARD_MAX_WIDTH))
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
        .child(name_field(create, can_submit, cx, theme))
        .child(members_only(create, cx, theme))
        .children(notes(chat, create, theme))
        .child(design::button("chat-create-cancel", "Cancel", theme, cancel).enabled(!busy))
        .child(
            design::button("chat-create-submit", "Create channel", theme, submit)
                .enabled(can_submit),
        );
    Some(card.into_any_element())
}

/// The name, typed; Enter creates where the dialog may.
fn name_field(
    create: &ChannelCreate,
    can_submit: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Input {
    let typed = cx.listener(|chat, event: &String, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.name = event.clone();
        }
        cx.notify();
    });
    let name = Input::new("chat-create-name")
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .value(create.name.clone())
        .placeholder("Channel name")
        .label("Channel name")
        .disabled(create.busy)
        .on_input(typed);
    match can_submit {
        true => name.on_submit(cx.listener(|chat, _: &(), _window, cx| {
            cx.notify();
            chat.create_channel(cx)
        })),
        false => name,
    }
}

/// Members only: on, posting takes a seat.
fn members_only(create: &ChannelCreate, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        if let Some(create) = &mut chat.create {
            create.members_only = !create.members_only;
        }
        cx.notify();
    });
    let label = match create.members_only {
        true => "Members only: On",
        false => "Members only: Off",
    };
    design::button("chat-create-members", label, theme, toggle)
        .enabled(!create.busy)
        .into_any_element()
}

/// Why the dialog failed, and why it cannot create at all.
fn notes(chat: &Chat, create: &ChannelCreate, theme: &Theme) -> Vec<AnyElement> {
    let note = |text: String, color| {
        div()
            .text_size(design::text::SECONDARY)
            .text_color(color)
            .child(text)
            .into_any_element()
    };
    let mut notes = Vec::new();
    if !create.error.is_empty() {
        notes.push(note(create.error.clone(), theme.danger));
    }
    if !chat.holds_account() {
        let why = "Create an account to create a channel".to_owned();
        notes.push(note(why, theme.muted));
    }
    notes
}
