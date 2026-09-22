//! Channel creation and attachment preview dialogs.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div, px, surface,
    wire,
};

use crate::ui::button;
use crate::{Chat, Loaded};

pub fn channel_create(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let create = chat.create.as_ref()?;
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
    let mut card = div()
        .id(ElementId::Name("chat-create-card".into()))
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
        .child(div().text_lg().child("Create a channel"))
        .child(
            div()
                .text_sm()
                .text_color(theme.muted)
                .child("Channel name"),
        )
        .child(
            div()
                .id(ElementId::Name("chat-create-name".into()))
                .h(px(28.))
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .child(if create.name.is_empty() {
                    "Channel name".into()
                } else {
                    create.name.clone()
                })
                .on_input(typed),
        )
        .child(button(
            ElementId::Name("chat-create-voice".into()),
            if create.voice {
                "Voice room: On"
            } else {
                "Voice room: Off"
            },
            theme,
            voice,
        ))
        .child(button(
            ElementId::Name("chat-create-members".into()),
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
                .text_sm()
                .text_color(theme.danger)
                .child(create.error.clone()),
        );
    }
    card = card
        .child(button(
            ElementId::Name("chat-create-cancel".into()),
            "Cancel",
            theme,
            cancel,
        ))
        .child(button(
            ElementId::Name("chat-create-submit".into()),
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
        .id(ElementId::Name("chat-preview-card".into()))
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
                .child(button(
                    ElementId::Name("chat-preview-open".into()),
                    "Open in Files",
                    theme,
                    open,
                ))
                .child(button(
                    ElementId::Name("chat-preview-close".into()),
                    "Close preview",
                    theme,
                    close,
                )),
        );
    if let Some(&(width, height)) = chat.pictures.get(&preview.link)
        && width > 0
        && height > 0
    {
        let (width, height) = crate::files::preview_box(width, height, chat.layout.viewport);
        return Some(
            card.child(
                div()
                    .id(ElementId::Name("chat-preview-picture-frame".into()))
                    .w(px(width))
                    .h(px(height))
                    .child(surface(
                        ElementId::Name("chat-preview-picture".into()),
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
                    .text_sm()
                    .text_color(theme.muted)
                    .child("Reading the file…"),
            );
        }
        Loaded::Failed(refusal) => {
            card = card.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(refusal.sentence.clone()),
            );
        }
        Loaded::Ready(text) => {
            if text.binary {
                card = card.child(crate::ui::empty_state(
                    ElementId::Name("chat-preview-binary".into()),
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
                        ElementId::Name("chat-preview-markdown".into()),
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
                        ElementId::Name("chat-preview-code".into()),
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
