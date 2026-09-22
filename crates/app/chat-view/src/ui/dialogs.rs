//! Channel creation and attachment preview dialogs.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, App, ClickEvent, Context, ParentElement, Styled, Theme, Window, div, px, surface,
    wire,
};

use crate::ui::button;
use crate::{Chat, Loaded};

type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn dialog_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    press: Option<Press>,
) -> AnyElement {
    let enabled = press.is_some();
    let button = div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .bg(theme.surface)
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.muted
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_disabled(!enabled)
        .child(label);
    match press {
        Some(press) => button
            .focusable()
            .hover(|style| style.bg(theme.surface_raised))
            .active(|style| style.bg(theme.accent_soft))
            .on_click(press)
            .into_any_element(),
        None => button.into_any_element(),
    }
}

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
        .rounded_md()
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
    let voice = (!busy).then(|| Box::new(voice) as Press);
    let members = (!busy && !create.voice).then(|| Box::new(members) as Press);
    let cancel = (!busy).then(|| Box::new(cancel) as Press);
    let submit = can_submit.then(|| Box::new(submit) as Press);
    let mut card = div()
        .id("chat-create-card")
        .max_w(px(480.))
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(div().text_size(px(16.)).child("Create a channel"))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Channel name"),
        )
        .child(name)
        .child(dialog_button(
            "chat-create-voice",
            if create.voice {
                "Voice room: On"
            } else {
                "Voice room: Off"
            },
            theme,
            voice,
        ))
        .child(dialog_button(
            "chat-create-members",
            if create.members_only {
                "Members only: On"
            } else {
                "Members only: Off"
            },
            theme,
            members,
        ));
    if !create.error.is_empty() {
        card = card.child(
            div()
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(create.error.clone()),
        );
    }
    if !chat.holds_account() {
        card = card.child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Create an account to create a channel"),
        );
    }
    card = card
        .child(dialog_button("chat-create-cancel", "Cancel", theme, cancel))
        .child(dialog_button(
            "chat-create-submit",
            "Create channel",
            theme,
            submit,
        ));
    Some(card.into_any_element())
}

pub fn preview(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let preview = chat.preview.as_ref()?;
    let link = preview.link.clone();
    let path = crate::files::attachment_file_path(&link);
    let name = path.rsplit('/').next().unwrap_or_default().to_owned();
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.open_link(link.clone(), cx)
    });
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.preview = None;
        cx.notify();
    });
    let mut card = div()
        .id("chat-preview-card")
        .size_full()
        .max_w(px(720.))
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().child(name))
                .child(button("chat-preview-open", "Open in Files", theme, open))
                .child(button("chat-preview-close", "Close preview", theme, close)),
        );
    if let Some(&(width, height)) = chat.pictures.get(&preview.link)
        && width > 0
        && height > 0
    {
        let (width, height) = crate::files::preview_box(width, height, chat.layout.viewport);
        return Some(
            card.child(
                div()
                    .id("chat-preview-picture-frame")
                    .w(px(width))
                    .h(px(height))
                    .child(surface(
                        "chat-preview-picture",
                        "picture",
                        vec![
                            wire::SurfaceValue::Str(crate::files::PICTURE_SURFACE.into()),
                            wire::SurfaceValue::Str(path),
                        ],
                    )),
            )
            .into_any_element(),
        );
    }
    match &preview.read {
        Loaded::Idle | Loaded::Loading(_) => {
            card = card.child(
                div()
                    .p_4()
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child("Reading the file…"),
            );
        }
        Loaded::Failed(refusal) => {
            card = card.child(
                div()
                    .p_4()
                    .text_size(px(12.))
                    .text_color(theme.danger)
                    .child(format!("Could not read this file: {}", refusal.sentence)),
            );
        }
        Loaded::Ready(text) => {
            if text.binary {
                card = card.child(crate::ui::empty_state(
                    "chat-preview-binary",
                    "No preview",
                    crate::files::BINARY_PLATE,
                    theme,
                ));
            } else {
                let (width, height) = crate::files::preview_room(chat.layout.viewport);
                let document = if crate::files::markdown_path(&path) {
                    let open = cx.listener(|chat, event: &wire::SurfaceValue, _window, cx| {
                        cx.notify();
                        if let wire::SurfaceValue::Str(link) = event {
                            chat.open_link(link.clone(), cx);
                        }
                    });
                    surface(
                        "chat-preview-markdown",
                        "markdown",
                        vec![
                            wire::SurfaceValue::Str(text.text.clone()),
                            wire::SurfaceValue::Str(String::new()),
                            wire::SurfaceValue::Bool(chat.session.dark),
                        ],
                    )
                    .on_event(open)
                } else {
                    surface(
                        "chat-preview-code",
                        "code",
                        vec![
                            wire::SurfaceValue::Str(text.text.clone()),
                            wire::SurfaceValue::Str(path),
                            wire::SurfaceValue::Bool(chat.session.dark),
                        ],
                    )
                };
                card = card.child(div().w(px(width)).h(px(height)).child(document).when(
                    text.clipped,
                    |element| {
                        element.child(
                            "Only the beginning is shown here. Open in Files for the whole file.",
                        )
                    },
                ));
            }
        }
    }
    Some(card.into_any_element())
}
