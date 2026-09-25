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
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, Theme,
    div, hsla, modal_overlay, sensor,
};

use crate::Chat;

const MENU_OVERLAY: &str = "chat-menu-overlay";

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let screen = div()
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
    let screen = with_menu(chat, screen, cx, &theme);
    let screen = with_create(chat, screen, cx, &theme);
    let measured = cx.listener(|chat, size: &(Pixels, Pixels), _window, cx| {
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
        .on_show(measured)
        .on_resize(resized)
}

/// The screen under the open message menu, if any.
///
/// The room sits under the id "chat-menu-overlay" whether or not a menu
/// is open: the host keeps a list's scroll (and a field's state) by the
/// ids above it, so a menu opening over the room must not move it to a
/// new path, or the timeline starts over at its latest message.
fn with_menu(chat: &Chat, screen: AnyElement, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let Some(menu) = menu::floating(chat, cx, theme) else {
        return div()
            .id(MENU_OVERLAY)
            .size_full()
            .child(screen)
            .into_any_element();
    };
    let dismiss = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let overlay = modal_overlay(MENU_OVERLAY, screen, menu)
        .label("Message menu")
        .on_dismiss(dismiss);
    // a confirm asks before anything else happens: it dims the room
    let confirming = chat
        .menu
        .as_ref()
        .is_some_and(|menu| menu.mode == crate::Mode::Delete);
    match confirming {
        true => overlay.backdrop(hsla(0., 0., 0., 0.35)).into_any_element(),
        false => overlay.into_any_element(),
    }
}

/// The screen under the create-channel dialog, if it is open; a busy
/// dialog is not dismissed.
fn with_create(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let Some(create) = dialogs::channel_create(chat, cx, theme) else {
        return screen;
    };
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
    match chat.create.as_ref().is_some_and(|create| create.busy) {
        true => overlay.into_any_element(),
        false => overlay.on_dismiss(dismiss).into_any_element(),
    }
}

fn connected(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut panes = div()
        .id("chat-panes")
        .flex()
        .size_full()
        .child(sidebar::render(chat, cx, theme))
        .child(design::divider(
            "chat-sidebar-resize",
            theme,
            cx,
            |chat, dx| {
                chat.layout.sidebar += dx;
                chat.layout.clamp();
            },
        ))
        .child(room::render(chat, cx, theme));
    if chat.details.is_some() && chat.room.is_some() {
        panes = panes
            .child(design::divider(
                "chat-details-resize",
                theme,
                cx,
                |chat, dx| {
                    chat.layout.details -= dx;
                    chat.layout.clamp();
                },
            ))
            .child(side::details(chat, cx, theme));
    } else if chat.room.as_ref().is_some_and(|room| room.thread.is_some()) {
        panes = panes
            .child(design::divider(
                "chat-thread-resize",
                theme,
                cx,
                |chat, dx| {
                    chat.layout.thread -= dx;
                    chat.layout.clamp();
                },
            ))
            .child(side::thread(chat, cx, theme));
    }
    panes
}
