//! Message menus at the pointer and the edit composer under its stream.
use crate::client::reaction_palette;
use crate::composer::Target;
use crate::{Chat, Menu, Mode, Pane};
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    Anchor, AnchoredPositionMode, AnyElement, App, ClickEvent, Context, Edges, ElementId,
    ParentElement, Point, RenderOnce, Role, Theme, Window,
};

const ROW: f32 = 28.;
const ROW_GAP: f32 = 2.;
const MENU_INSET: f32 = 6.;
const CELL: f32 = 32.;
const PICKER_GAP: f32 = 2.;
const PICKER_INSET: f32 = 8.;
const COLUMNS: u16 = 8;
type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn prefix(pane: Pane) -> &'static str {
    match pane {
        Pane::Timeline => "chat-room-message-",
        Pane::Thread => "chat-thread-message-",
    }
}
pub fn focus_key(pane: Pane, mode: Mode) -> String {
    let suffix = match mode {
        Mode::Reactions => "reaction-focus",
        Mode::Delete => "delete-focus",
        _ => "action-focus",
    };
    format!("{}{suffix}", prefix(pane))
}

pub fn floating(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> Option<AnyElement> {
    let menu = chat.menu.as_ref()?;
    let (at, size) = popup_geometry(menu, reaction_palette().len())?;
    let content = message_menu(chat, menu, cx, theme);
    let frame = div()
        .id(focus_key(menu.pane, menu.mode))
        .w(px(size.0))
        .h(px(size.1))
        .overflow_hidden()
        .focusable()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .child(content);
    Some(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(Point {
                x: px(at.0),
                y: px(at.1),
            })
            .position_mode(AnchoredPositionMode::Window)
            .snap_to_window_with_margin(Edges::all(px(8.)))
            .child(frame)
            .into_any_element(),
    )
}

fn popup_geometry(menu: &Menu, reaction_count: usize) -> Option<((f32, f32), (f32, f32))> {
    let size = match menu.mode {
        Mode::More => menu_size(if menu.pane == Pane::Thread { 4 } else { 5 }),
        Mode::Reactions => picker_size(reaction_count),
        Mode::Delete => (280., 96.),
        Mode::Toolbar | Mode::Editing => return None,
    };
    Some((menu.at, size))
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
    let close = cx.listener(|chat, _: &ClickEvent, _, cx| {
        chat.close_menu();
        cx.notify();
    });
    Some(
        div()
            .id("chat-message-editing")
            .px_4()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .child(crate::ui::room::composer(
                        chat,
                        target,
                        "Edit message",
                        true,
                        cx,
                    ))
                    .child(div().flex().justify_end().child(Item::text(
                        "chat-message-edit-cancel",
                        "Cancel message edit",
                        Some(Box::new(close)),
                        *theme,
                    ))),
            )
            .into_any_element(),
    )
}

fn message_menu(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    match menu.mode {
        Mode::Toolbar | Mode::More => actions(chat, menu, cx, theme),
        Mode::Reactions => reactions(chat, menu, cx, theme),
        Mode::Delete => delete(chat, cx, theme),
        Mode::Editing => div().into_any_element(),
    }
}

fn actions(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
    let writable = chat.may_write();
    let mut list = div()
        .id("chat-menu-actions")
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .p(px(MENU_INSET));
    if pane == Pane::Timeline {
        let press = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.open_thread(seq, cx)
        });
        list = list.child(Item::new(
            "chat-menu-reply",
            "↩",
            "Reply in thread",
            Some(Box::new(press)),
            *theme,
        ));
    }
    let react = writable.then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
        })) as Press
    });
    list = list.child(Item::new(
        "chat-menu-add-reaction",
        "😀",
        "Add reaction",
        react,
        *theme,
    ));
    let link = chat.message_link(seq);
    let copy = (!link.is_empty()).then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.close_menu();
            chat.copy_text(link.clone(), "Message link copied", cx);
        })) as Press
    });
    list = list.child(Item::new(
        "chat-menu-copy-link",
        "🔗",
        "Copy link",
        copy,
        *theme,
    ));
    let edit = writable.then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            chat.open_menu(pane, seq, rev, Mode::Editing, window, cx)
        })) as Press
    });
    list = list.child(Item::new(
        "chat-menu-edit",
        "✎",
        "Edit message",
        edit,
        *theme,
    ));
    let remove = writable.then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            chat.open_menu(pane, seq, rev, Mode::Delete, window, cx)
        })) as Press
    });
    list.child(Item::new(
        "chat-menu-delete",
        "🗑",
        "Delete message",
        remove,
        *theme,
    ))
    .into_any_element()
}

fn reactions(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let writable = chat.may_write();
    let seq = menu.seq;
    let mut grid = div()
        .id("chat-reaction-grid")
        .grid()
        .grid_cols(COLUMNS)
        .gap(px(PICKER_GAP))
        .p(px(PICKER_INSET));
    for emoji in reaction_palette() {
        let reaction = emoji.to_owned();
        let press = writable.then(|| {
            Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
                cx.notify();
                chat.react(seq, reaction.clone(), true, cx)
            })) as Press
        });
        grid = grid.child(Reaction {
            id: format!("chat-reaction-{emoji}").into(),
            emoji: emoji.into(),
            press,
            theme: *theme,
        });
    }
    grid.into_any_element()
}

fn delete(_chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let cancel = cx.listener(|chat, _: &ClickEvent, _, cx| {
        chat.close_menu();
        cx.notify();
    });
    let confirm = Some({
        Box::new(cx.listener(|chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.delete_armed(cx)
        })) as Press
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .child(div().text_size(px(13.)).child("Delete this message?"))
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("It leaves the room for everyone."),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(Item::text(
                    "chat-menu-cancel-delete",
                    "Cancel",
                    Some(Box::new(cancel)),
                    *theme,
                ))
                .child(Item::toned(
                    "chat-menu-confirm-delete",
                    "Delete",
                    confirm,
                    *theme,
                    theme.danger,
                    theme.danger_soft,
                )),
        )
        .into_any_element()
}

fn menu_size(items: usize) -> (f32, f32) {
    let n = items as f32;
    (220., MENU_INSET * 2. + n * ROW + (n - 1.).max(0.) * ROW_GAP)
}
fn picker_size(count: usize) -> (f32, f32) {
    let columns = COLUMNS as f32;
    let rows = (count as f32 / columns).ceil();
    (
        PICKER_INSET * 2. + columns * CELL + (columns - 1.) * PICKER_GAP,
        PICKER_INSET * 2. + rows * CELL + (rows - 1.).max(0.) * PICKER_GAP,
    )
}

#[derive(IntoElement)]
struct Item {
    id: ElementId,
    glyph: Option<String>,
    label: String,
    press: Option<Press>,
    theme: Theme,
    fg: Hsla,
    bg: Hsla,
}
impl Item {
    fn new(
        id: impl Into<ElementId>,
        glyph: &str,
        label: &str,
        press: Option<Press>,
        theme: Theme,
    ) -> Self {
        Self {
            id: id.into(),
            glyph: Some(glyph.into()),
            label: label.into(),
            press,
            fg: theme.foreground,
            bg: theme.surface,
            theme,
        }
    }
    fn text(id: impl Into<ElementId>, label: &str, press: Option<Press>, theme: Theme) -> Self {
        Self::toned(id, label, press, theme, theme.foreground, theme.surface)
    }
    fn toned(
        id: impl Into<ElementId>,
        label: &str,
        press: Option<Press>,
        theme: Theme,
        fg: Hsla,
        bg: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            glyph: None,
            label: label.into(),
            press,
            theme,
            fg,
            bg,
        }
    }
}
impl RenderOnce for Item {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let enabled = self.press.is_some();
        let mut row = div()
            .id(self.id)
            .w_full()
            .h(px(ROW))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .bg(self.bg)
            .text_color(if enabled { self.fg } else { self.theme.muted })
            .role(Role::Button)
            .aria_label(self.label.clone())
            .aria_disabled(!enabled);
        if let Some(glyph) = self.glyph {
            row = row.child(
                div()
                    .w(px(20.))
                    .h(px(ROW))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .whitespace_nowrap()
                    .child(glyph),
            );
        }
        row = row.child(div().whitespace_nowrap().child(self.label));
        match self.press {
            Some(press) => row
                .focusable()
                .hover(|s| s.bg(self.theme.surface_raised))
                .active(|s| s.bg(self.theme.accent_soft))
                .on_click(press)
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}

#[derive(IntoElement)]
struct Reaction {
    id: ElementId,
    emoji: String,
    press: Option<Press>,
    theme: Theme,
}
impl RenderOnce for Reaction {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let enabled = self.press.is_some();
        let cell = div()
            .id(self.id)
            .w(px(CELL))
            .h(px(CELL))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_lg()
            .role(Role::Button)
            .aria_label("Add reaction")
            .aria_description(self.emoji.clone())
            .aria_disabled(!enabled)
            .child(self.emoji);
        match self.press {
            Some(press) => cell
                .focusable()
                .hover(|s| s.bg(self.theme.surface_raised))
                .active(|s| s.bg(self.theme.accent_soft))
                .on_click(press)
                .into_any_element(),
            None => cell.text_color(self.theme.muted).into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_menu_and_picker_dimensions_are_preserved() {
        assert_eq!(menu_size(5), (220., 160.));
        assert_eq!(menu_size(4), (220., 130.));
        assert_eq!(picker_size(8), (286., 48.));
        assert_eq!(picker_size(9), (286., 82.));
        let menu = Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 2,
            mode: Mode::More,
            at: (617., 449.),
        };
        assert_eq!(
            popup_geometry(&menu, 16),
            Some(((617., 449.), (220., 160.)))
        );
    }
}
