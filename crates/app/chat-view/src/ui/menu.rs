//! Message menus at the pointer and the edit composer under its stream.
use crate::composer::Target;
use crate::emoji;
use crate::{Chat, Menu, Mode, Pane};
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    Anchor, AnchoredPositionMode, AnyElement, App, ClickEvent, Context, Edges, ElementId,
    ParentElement, Point, RenderOnce, Role, Theme, Window,
};

// A popup's size is fixed from these, so it never jumps under the pointer.
const ROW: f32 = design::height::CONTROL as f32;
const ROW_GAP: f32 = design::spacing::HAIR as f32;
const MENU_INSET: f32 = design::spacing::XS as f32;
const CELL: f32 = 32.;
const PICKER_GAP: f32 = design::spacing::HAIR as f32;
const PICKER_INSET: f32 = design::spacing::SM as f32;
const COLUMNS: u16 = 8;
/// Rows of the picker's grid: a tab's `emoji::PER_TAB` in rows of
/// `COLUMNS`.
const GRID_ROWS: f32 = emoji::PER_TAB.div_ceil(COLUMNS as usize) as f32;
const SEARCH: f32 = design::height::CONTROL as f32;
const CAPTION: f32 = 14.;
const TABS: f32 = design::height::CONTROL as f32;
const STACK_GAP: f32 = design::spacing::XS as f32;
type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

mod picker;
use picker::{picker_size, reactions};

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
    let (at, size) = popup_geometry(menu, more_items(chat, menu).len())?;
    // a fixed size where the popup must not jump under the pointer; the
    // delete confirmation sizes to its words and buttons
    let content = message_menu(chat, menu, cx, theme);
    // the picker's focus key names its search field, which takes the keys
    let id = match menu.mode {
        Mode::Reactions => format!("{}reaction-frame", prefix(menu.pane)),
        _ => focus_key(menu.pane, menu.mode),
    };
    let frame = div()
        .id(id)
        .when_some(size, |frame, (w, h)| {
            frame.w(px(w)).h(px(h)).overflow_hidden()
        })
        .focusable()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        // Anchored near the row it was opened from, this popup can overlap
        // the message card beneath it; without occlude a click here (e.g.
        // "Delete") also fires the card's row-select handler, which resets
        // `self.menu` before this popup's own handler gets to read it.
        .occlude()
        .child(content);
    Some(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(Point {
                x: px(at.0),
                y: px(at.1),
            })
            .position_mode(AnchoredPositionMode::Window)
            .snap_to_window_with_margin(Edges::all(design::space::SM))
            .child(frame)
            .into_any_element(),
    )
}

/// A width and height, or a point, in pixels.
type Pair = (f32, f32);

fn popup_geometry(menu: &Menu, items: usize) -> Option<(Pair, Option<Pair>)> {
    let size = match menu.mode {
        Mode::More => Some(menu_size(items)),
        Mode::Reactions => Some(picker_size()),
        Mode::Delete => None,
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
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .child(div().w(px(96.)).child(Item::text(
                                "chat-message-edit-cancel",
                                "Cancel edit",
                                Some(Box::new(close)),
                                *theme,
                            ))),
                    ),
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

/// What the "More" menu offers on this message: only what the reader may
/// do to it — an edit or a delete the chat module would refuse is not
/// offered, and "Reply in thread" not for the thread already open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Reply,
    React,
    CopyLink,
    Edit,
    Delete,
}

fn more_items(chat: &Chat, menu: &Menu) -> Vec<Action> {
    let (pane, seq) = (menu.pane, menu.seq);
    let writable = chat.may_write();
    let open = chat
        .room
        .as_ref()
        .and_then(|room| room.thread.as_ref())
        .is_some_and(|thread| thread.root == seq);
    [
        (Action::Reply, pane == Pane::Timeline && !open),
        (Action::React, writable),
        (Action::CopyLink, chat.message_link(seq).is_some()),
        (Action::Edit, writable && chat.wrote(pane, seq)),
        (Action::Delete, writable && chat.may_delete(pane, seq)),
    ]
    .into_iter()
    .filter_map(|(action, offered)| offered.then_some(action))
    .collect()
}

fn actions(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
    let mut list = div()
        .id("chat-menu-actions")
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .p(px(MENU_INSET));
    for action in more_items(chat, menu) {
        let item = match action {
            Action::Reply => {
                let press = cx.listener(move |chat, _: &ClickEvent, _, cx| {
                    cx.notify();
                    chat.open_thread(seq, cx)
                });
                Item::new(
                    "chat-menu-reply",
                    "↩",
                    "Reply in thread",
                    Some(Box::new(press)),
                    *theme,
                )
            }
            Action::React => {
                let press = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
                });
                Item::new(
                    "chat-menu-add-reaction",
                    "😀",
                    "Add reaction",
                    Some(Box::new(press)),
                    *theme,
                )
            }
            Action::CopyLink => {
                let link = chat.message_link(seq);
                let press = cx.listener(move |chat, _: &ClickEvent, _, cx| {
                    cx.notify();
                    chat.close_menu();
                    match &link {
                        Some(link) => chat.copy_text(link.clone(), "message link", cx),
                        None => cx.host().log("no message link: the session names no chain"),
                    }
                });
                Item::new(
                    "chat-menu-copy-link",
                    "🔗",
                    "Copy link",
                    Some(Box::new(press)),
                    *theme,
                )
            }
            Action::Edit => {
                let press = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Editing, window, cx)
                });
                Item::new(
                    "chat-menu-edit",
                    "✎",
                    "Edit message",
                    Some(Box::new(press)),
                    *theme,
                )
            }
            Action::Delete => {
                let press = cx.listener(move |chat, _: &ClickEvent, window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Delete, window, cx)
                });
                Item::toned(
                    "chat-menu-delete",
                    "Delete message",
                    Some(Box::new(press)),
                    *theme,
                    theme.danger,
                    theme.background,
                )
                .glyph("🗑")
            }
        };
        list = list.child(item);
    }
    list.into_any_element()
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
        .child(
            div()
                .text_size(design::text::BODY)
                .child("Delete this message?"),
        )
        .child(
            div()
                .text_size(design::text::SECONDARY)
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
            bg: theme.background,
            theme,
        }
    }
    fn glyph(mut self, glyph: &str) -> Self {
        self.glyph = Some(glyph.into());
        self
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
                    .text_size(design::text::BODY)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_and_picker_dimensions() {
        assert_eq!(menu_size(5), (220., 160.));
        assert_eq!(menu_size(4), (220., 130.));
        assert_eq!(menu_size(2), (220., 70.));
        // 8 cells a row; search, two captions, the frequent row, tabs and
        // five rows of grid
        assert_eq!(
            picker_size(),
            (286., 16. + 28. + 14. + 32. + 28. + 14. + 168. + 30.)
        );
        let menu = Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 2,
            mode: Mode::More,
            at: (617., 449.),
        };
        assert_eq!(
            popup_geometry(&menu, 3),
            Some(((617., 449.), Some((220., 100.))))
        );
        let delete = Menu {
            mode: Mode::Delete,
            ..menu
        };
        assert_eq!(popup_geometry(&delete, 0), Some(((617., 449.), None)));
    }
}
