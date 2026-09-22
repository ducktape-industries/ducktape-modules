//! Native GPUI composition for Chat. State and module operations stay in the
//! root view; this module only builds the element tree and installs listeners.

pub mod dialogs;
pub mod menu;
pub mod message;
pub mod room;
pub mod side;
pub mod sidebar;
mod timeline;

use ducktape_view_guest::{
    ClickEvent, Context, ElementId, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    Theme, Window, div, px,
};

use crate::Chat;

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> impl IntoElement {
    let theme = cx.global::<Theme>();
    let mut screen = div()
        .id(ElementId::Name("chat-root".into()))
        .relative()
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .child(if chat.session.connected {
            connected(chat, cx, theme).into_any_element()
        } else {
            empty_state(
                ElementId::Name("chat-disconnected".into()),
                "Not connected",
                "Choose a network from the sidebar to reconnect.",
                theme,
            )
            .into_any_element()
        });

    if let Some(menu) = menu::floating(chat, cx, theme) {
        screen = screen.child(menu);
    }
    if let Some(preview) = dialogs::preview(chat, cx, theme) {
        screen = screen.child(preview);
    }
    if let Some(create) = dialogs::channel_create(chat, cx, theme) {
        screen = screen.child(create);
    }
    screen
}

fn connected(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut panes = div()
        .id(ElementId::Name("chat-panes".into()))
        .flex()
        .size_full()
        .child(sidebar::render(chat, cx, theme))
        .child(
            div()
                .id(ElementId::Name("chat-sidebar-resize".into()))
                .w(px(1.))
                .bg(theme.border),
        )
        .child(room::render(chat, cx, theme));
    if chat.details.is_some() && chat.room.is_some() {
        panes = panes
            .child(
                div()
                    .id(ElementId::Name("chat-details-resize".into()))
                    .w(px(1.))
                    .bg(theme.border),
            )
            .child(side::details(chat, cx, theme));
    } else if chat.room.as_ref().is_some_and(|room| room.thread.is_some()) {
        panes = panes
            .child(
                div()
                    .id(ElementId::Name("chat-thread-resize".into()))
                    .w(px(1.))
                    .bg(theme.border),
            )
            .child(side::thread(chat, cx, theme));
    }
    panes
}

pub(crate) fn button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .bg(theme.surface)
        .hover(|s| s.bg(theme.surface_raised))
        .active(|s| s.bg(theme.accent_soft))
        .on_click(click)
        .child(label.into())
}

pub(crate) fn empty_state(
    id: impl Into<ElementId>,
    title: impl Into<String>,
    detail: impl Into<String>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .p_6()
        .max_w(px(420.))
        .child(div().text_base().child(title.into()))
        .child(div().text_sm().text_color(theme.muted).child(detail.into()))
}

pub(crate) fn badge(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    foreground: ducktape_view_guest::Hsla,
    background: ducktape_view_guest::Hsla,
) -> impl IntoElement {
    div()
        .id(id)
        .px_1()
        .py_0p5()
        .rounded_sm()
        .bg(background)
        .text_color(foreground)
        .text_xs()
        .child(label.into())
}
