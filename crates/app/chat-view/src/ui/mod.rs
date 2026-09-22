//! Native GPUI composition for Chat. State and module operations stay in the
//! root view; this module only builds the element tree and installs listeners.

mod components;
pub mod dialogs;
pub mod menu;
pub mod message;
pub mod room;
pub mod side;
pub mod sidebar;
mod timeline;

pub(crate) use components::{badge, button, empty_state};
use ducktape_view_guest::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, Theme, div,
    hsla, modal_overlay, px, resize_handle, sensor,
};

use crate::Chat;

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let mut screen = div()
        .id(ElementId::Name("chat-root".into()))
        .relative()
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(px(13.))
        .child(if chat.session.connected {
            connected(chat, cx, &theme).into_any_element()
        } else {
            empty_state(
                ElementId::Name("chat-disconnected".into()),
                "Not connected",
                "Choose a network from the sidebar to reconnect.",
                &theme,
            )
            .into_any_element()
        })
        .into_any_element();

    if let Some(menu) = menu::floating(chat, cx, &theme) {
        let dismiss = cx.listener(|chat, _: &(), _window, cx| {
            chat.close_menu();
            cx.notify();
        });
        screen = modal_overlay(ElementId::Name("chat-menu-overlay".into()), screen, menu)
            .label("Message menu")
            .on_dismiss(dismiss)
            .into_any_element();
    }
    if let Some(preview) = dialogs::preview(chat, cx, &theme) {
        let dismiss = cx.listener(|chat, _: &(), _window, cx| {
            chat.preview = None;
            cx.notify();
        });
        screen = modal_overlay(
            ElementId::Name("chat-preview-overlay".into()),
            screen,
            preview,
        )
        .label("Attachment preview")
        .flex()
        .items_center()
        .justify_center()
        .backdrop(hsla(0., 0., 0., 0.55))
        .on_dismiss(dismiss)
        .into_any_element();
    }
    if let Some(create) = dialogs::channel_create(chat, cx, &theme) {
        let dismiss = cx.listener(|chat, _: &(), _window, cx| {
            chat.create = None;
            cx.notify();
        });
        let overlay = modal_overlay(
            ElementId::Name("chat-create-overlay".into()),
            screen,
            create,
        )
        .label("Create channel")
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .backdrop(hsla(0., 0., 0., 0.55));
        screen = if chat.create.as_ref().is_some_and(|create| create.busy) {
            overlay.into_any_element()
        } else {
            overlay.on_dismiss(dismiss).into_any_element()
        };
    }
    let shown = cx.listener(|chat, size: &(Pixels, Pixels), _window, cx| {
        chat.layout.viewport = (size.0.into(), size.1.into());
        chat.layout.clamp();
        cx.notify();
    });
    let resized = cx.listener(|chat, size: &(Pixels, Pixels), _window, cx| {
        chat.layout.viewport = (size.0.into(), size.1.into());
        chat.layout.clamp();
        cx.notify();
    });
    sensor(ElementId::Name("chat-viewport".into()), screen)
        .on_show(shown)
        .on_resize(resized)
}

fn connected(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut panes = div()
        .id(ElementId::Name("chat-panes".into()))
        .flex()
        .size_full()
        .child(sidebar::render(chat, cx, theme))
        .child(divider("chat-sidebar-resize", theme, cx, |chat, dx| {
            chat.layout.sidebar += dx;
        }))
        .child(room::render(chat, cx, theme));
    if chat.details.is_some() && chat.room.is_some() {
        panes = panes
            .child(divider("chat-details-resize", theme, cx, |chat, dx| {
                chat.layout.details -= dx;
            }))
            .child(side::details(chat, cx, theme));
    } else if chat.room.as_ref().is_some_and(|room| room.thread.is_some()) {
        panes = panes
            .child(divider("chat-thread-resize", theme, cx, |chat, dx| {
                chat.layout.thread -= dx;
            }))
            .child(side::thread(chat, cx, theme));
    }
    panes
}

fn divider(
    id: &'static str,
    theme: &Theme,
    cx: &mut Context<Chat>,
    drag: impl Fn(&mut Chat, f32) + 'static,
) -> impl IntoElement {
    let dragged = cx.listener(move |chat, delta: &(Pixels, Pixels), _window, cx| {
        drag(chat, delta.0.into());
        chat.layout.clamp();
        cx.notify();
    });
    resize_handle(
        ElementId::Name(id.into()),
        div().w(px(1.)).h_full().bg(theme.border),
    )
    .on_drag(dragged)
}
