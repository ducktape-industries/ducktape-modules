//! Native GPUI composition for Chat. State and module operations stay in the
//! root view; this module only builds the element tree and installs listeners.

pub mod dialogs;
pub mod menu;
pub mod message;
pub mod room;
pub mod side;
pub mod sidebar;
mod timeline;

use ducktape_view_guest::design;
pub(crate) use ducktape_view_guest::design::{badge, button, empty_state, quiet};
use ducktape_view_guest::{
    Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, Theme, div, hsla,
    modal_overlay, px, resize_handle, sensor,
};

use crate::Chat;

const MENU_OVERLAY: &str = "chat-menu-overlay";

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let mut screen = div()
        .id("chat-root")
        .relative()
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(if chat.session.connected {
            connected(chat, cx, &theme).into_any_element()
        } else {
            empty_state(
                "chat-disconnected",
                "Not connected",
                "Choose a network from the sidebar to reconnect.",
                &theme,
            )
            .into_any_element()
        })
        .into_any_element();

    // The room sits under the id "chat-menu-overlay" whether or not a menu
    // is open: the host keeps a list's scroll (and a field's state) by the
    // ids above it, so a menu opening over the room must not move it to a
    // new path, or the timeline starts over at its latest message.
    screen = match menu::floating(chat, cx, &theme) {
        Some(menu) => {
            let dismiss = cx.listener(|chat, _: &(), _window, cx| {
                chat.close_menu();
                cx.notify();
            });
            let mut overlay = modal_overlay(MENU_OVERLAY, screen, menu)
                .label("Message menu")
                .on_dismiss(dismiss);
            // a confirm asks before anything else happens: it dims the room
            if chat
                .menu
                .as_ref()
                .is_some_and(|menu| menu.mode == crate::Mode::Delete)
            {
                overlay = overlay.backdrop(hsla(0., 0., 0., 0.35));
            }
            overlay.into_any_element()
        }
        None => div()
            .id(MENU_OVERLAY)
            .size_full()
            .child(screen)
            .into_any_element(),
    };
    if let Some(create) = dialogs::channel_create(chat, cx, &theme) {
        let dismiss = cx.listener(|chat, _: &(), _window, cx| {
            chat.create = None;
            cx.notify();
        });
        let overlay = modal_overlay("chat-create-overlay", screen, create)
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
    sensor("chat-viewport", screen)
        .size_full()
        .on_show(shown)
        .on_resize(resized)
}

fn connected(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut panes = div()
        .id("chat-panes")
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
    resize_handle(id, div().w(px(1.)).h_full().bg(theme.border)).on_drag(dragged)
}
