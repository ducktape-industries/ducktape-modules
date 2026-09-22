//! Floating message actions and the native reaction picker.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, ParentElement, Styled, Theme, div,
};

use crate::client::reaction_palette;
use crate::composer::Target;
use crate::ui::button;
use crate::{Chat, Menu, Mode, Pane};

fn prefix(pane: Pane) -> &'static str {
    match pane {
        Pane::Timeline => "chat-room-message-",
        Pane::Thread => "chat-thread-message-",
    }
}

pub fn focus_key(pane: Pane, mode: Mode) -> String {
    let focus = match mode {
        Mode::Reactions => "reaction-focus",
        Mode::Delete => "delete-focus",
        _ => "action-focus",
    };
    format!("{}{focus}", prefix(pane))
}

pub fn floating(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let menu = chat.menu.as_ref()?;
    if matches!(menu.mode, Mode::Toolbar | Mode::Editing) {
        return None;
    }
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    Some(
        div()
            .id(ElementId::Name("chat-floating-menu".into()))
            .absolute()
            .top_3()
            .right_3()
            .min_w_48()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(message_menu(chat, menu, cx, theme))
            .child(button(
                ElementId::Name("chat-floating-close".into()),
                "Close",
                theme,
                close,
            ))
            .into_any_element(),
    )
}

pub fn editing(
    chat: &Chat,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Option<AnyElement> {
    let menu = chat.menu.as_ref()?;
    if menu.mode != Mode::Editing || menu.pane != pane {
        return None;
    }
    let target = Target::Edit {
        channel: chat.room_id(),
        seq: menu.seq,
        base_rev: menu.rev,
    };
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    Some(
        div()
            .id(ElementId::Name("chat-message-editing".into()))
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(crate::ui::room::composer(
                chat,
                target,
                "Edit message",
                !chat.session.busy,
                cx,
            ))
            .child(button(
                ElementId::Name("chat-message-edit-cancel".into()),
                "Cancel message edit",
                theme,
                close,
            ))
            .into_any_element(),
    )
}

fn message_menu(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
    let mut content = div()
        .id(ElementId::Name(focus_key(pane, menu.mode).into()))
        .focusable()
        .flex()
        .flex_col()
        .gap_1();
    match menu.mode {
        Mode::More | Mode::Toolbar => {
            if pane == Pane::Timeline {
                let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                    cx.notify();
                    chat.open_thread(seq, cx)
                });
                content = content.child(button(
                    ElementId::Name("chat-menu-reply".into()),
                    "Reply in thread",
                    theme,
                    open,
                ));
            }
            let link = chat.message_link(seq);
            if !link.is_empty() {
                let copy = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                    cx.notify();
                    chat.close_menu();
                    chat.copy_text(link.clone(), "Message link copied", cx);
                });
                content = content.child(button(
                    ElementId::Name("chat-menu-copy-link".into()),
                    "Copy link",
                    theme,
                    copy,
                ));
            }
            if chat.may_write() {
                let react = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
                });
                content = content.child(button(
                    ElementId::Name("chat-menu-add-reaction".into()),
                    "Add reaction",
                    theme,
                    react,
                ));
                let edit = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Editing, window, cx)
                });
                content = content.child(button(
                    ElementId::Name("chat-menu-edit".into()),
                    "Edit message",
                    theme,
                    edit,
                ));
                let delete = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Delete, window, cx)
                });
                content = content.child(button(
                    ElementId::Name("chat-menu-delete".into()),
                    "Delete message",
                    theme,
                    delete,
                ));
            }
        }
        Mode::Reactions => {
            let mut grid = div()
                .id(ElementId::Name("chat-reaction-grid".into()))
                .grid()
                .grid_cols(8)
                .gap_1();
            for emoji in reaction_palette() {
                let emoji = emoji.to_owned();
                let reaction = emoji.clone();
                let click = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                    cx.notify();
                    chat.react(seq, reaction.clone(), true, cx)
                });
                grid = grid.child(
                    div()
                        .id(ElementId::Name(format!("chat-reaction-{}", emoji).into()))
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .hover(|s| s.bg(theme.surface_raised))
                        .role(ducktape_view_guest::Role::Button)
                        .focusable()
                        .on_click(click)
                        .child(emoji),
                );
            }
            content = content.child(grid);
        }
        Mode::Delete => {
            content = content
                .child(div().text_base().child("Delete this message?"))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted)
                        .child("It leaves the room for everyone."),
                );
            let cancel = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                chat.close_menu();
                cx.notify();
            });
            let confirm = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.delete_armed(cx)
            });
            content = content
                .child(button(
                    ElementId::Name("chat-menu-cancel-delete".into()),
                    "Cancel",
                    theme,
                    cancel,
                ))
                .child(button(
                    ElementId::Name("chat-menu-confirm-delete".into()),
                    "Delete",
                    theme,
                    confirm,
                ));
        }
        Mode::Editing => {}
    }
    content.into_any_element()
}
