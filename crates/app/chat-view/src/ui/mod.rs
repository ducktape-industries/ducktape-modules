//! The frame: one render function per pane reading the state,
//! handlers registered as closures over the smallest slice of it, nodes
//! styled with the kit and the helpers in `controls`. Nothing here mutates state.

pub mod dialogs;
pub mod menu;
pub mod message;
pub mod room;
pub mod side;
pub mod sidebar;

use ducktape_view_guest::view::Cx;
use ducktape_view_guest::wire::{AlignX, AlignY, Node, kit};

use crate::Chat;
use ducktape_view_guest::wire::kit::*;

pub fn render(chat: &Chat, cx: &mut Cx<Chat>) -> Node {
    let screen = if chat.session.connected {
        connected(chat, cx)
    } else {
        kit::empty_state(
            "chat/disconnected",
            "Not connected",
            "Choose a network from the sidebar to reconnect.",
        )
    };
    // every press reports where it landed before the control under it
    // answers, so a menu opens at the pointer
    let pressed = cx.on_value(|chat, at: (f32, f32), _| chat.layout.press = at);
    let screen = with_press_at(mouse_area("chat/press-area", screen), pressed);
    let screen = match menu::floating(chat, cx) {
        None => screen,
        Some(menu) => {
            let dismiss = cx.on(|chat, _| chat.close_menu());
            overlay(
                "chat/menu-overlay",
                "Message menu",
                0.,
                [0.; 4],
                AlignX::Left,
                AlignY::Top,
                Some(dismiss),
                screen,
                menu,
            )
        }
    };
    let screen = match dialogs::preview(chat, cx) {
        None => screen,
        Some(card) => {
            let close = cx.on(|chat, _| chat.preview = None);
            overlay(
                "chat/preview-overlay",
                "Attachment preview",
                30.,
                [0., 0., 0., 0.55],
                AlignX::Center,
                AlignY::Center,
                Some(close),
                screen,
                card,
            )
        }
    };
    let screen = match dialogs::channel_create(chat, cx) {
        None => screen,
        Some(card) => {
            let busy = chat.create.as_ref().is_some_and(|c| c.busy);
            let close = (!busy).then(|| cx.on(|chat, _| chat.create = None));
            overlay(
                "chat/create-overlay",
                "Create channel",
                kit::spacing::XL as f32,
                [0., 0., 0., 0.55],
                AlignX::Center,
                AlignY::Center,
                close,
                screen,
                card,
            )
        }
    };
    let measured = cx.on_value(|chat, size: (f32, f32), _| {
        chat.layout.viewport = size;
        chat.layout.clamp();
    });
    Node::Sensor {
        key: "chat/viewport".into(),
        reset: None,
        on_show: Some(measured),
        on_resize: Some(measured),
        on_hide: None,
        anticipate: None,
        delay: None,
        child: Box::new(screen),
    }
}

/// Sidebar, room, and one side pane: details in front of a thread when both
/// are open, so each pane's width is clamped as the only one beside the room.
fn connected(chat: &Chat, cx: &mut Cx<Chat>) -> Node {
    let mut panes = vec![
        sidebar::render(chat, cx),
        divider("chat/sidebar-resize", cx, |chat, dx| {
            chat.layout.sidebar += dx
        }),
        room::render(chat, cx),
    ];
    let room_open = chat.room.is_some();
    if chat.details.is_some() && room_open {
        panes.push(divider("chat/details-resize", cx, |chat, dx| {
            chat.layout.details -= dx
        }));
        panes.push(side::details(chat, cx));
    } else if room_open && chat.room.as_ref().is_some_and(|r| r.thread.is_some()) {
        panes.push(divider("chat/thread-resize", cx, |chat, dx| {
            chat.layout.thread -= dx
        }));
        panes.push(side::thread(chat, cx));
    }
    fill(kit::spaced(kit::row("chat/panes", panes), 0.))
}

fn divider(key: &str, cx: &mut Cx<Chat>, drag: impl Fn(&mut Chat, f32) + 'static) -> Node {
    let on_drag = cx.on_value(move |chat, (dx, _): (f64, f64), _| {
        drag(chat, dx as f32);
        chat.layout.clamp();
    });
    resize_handle(key, on_drag)
}

pub(crate) fn close_glyph(
    key: &str,
    label: &str,
    cx: &mut Cx<Chat>,
    run: impl FnMut(&mut Chat, &mut Cx<Chat>) + 'static,
) -> Node {
    let press = cx.on(run);
    glyph(key, "✕", label, Some(press))
}
