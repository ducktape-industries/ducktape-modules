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

const ROW: f32 = 28.;
const ROW_GAP: f32 = 2.;
const MENU_INSET: f32 = 6.;
const CELL: f32 = 32.;
const PICKER_GAP: f32 = 2.;
const PICKER_INSET: f32 = 8.;
const COLUMNS: u16 = 8;
/// Rows of the picker's grid: a tab's `emoji::PER_TAB` in rows of eight.
const GRID_ROWS: f32 = 5.;
const SEARCH: f32 = 28.;
const CAPTION: f32 = 14.;
const TABS: f32 = 28.;
const STACK_GAP: f32 = 6.;
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
    let (at, size) = popup_geometry(menu, more_items(chat, menu).len())?;
    let content = message_menu(chat, menu, cx, theme);
    // the picker's focus key names its search field, which takes the keys
    let id = match menu.mode {
        Mode::Reactions => format!("{}reaction-frame", prefix(menu.pane)),
        _ => focus_key(menu.pane, menu.mode),
    };
    let frame = div()
        .id(id)
        .w(px(size.0))
        .h(px(size.1))
        .overflow_hidden()
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
            .snap_to_window_with_margin(Edges::all(px(8.)))
            .child(frame)
            .into_any_element(),
    )
}

fn popup_geometry(menu: &Menu, items: usize) -> Option<((f32, f32), (f32, f32))> {
    let size = match menu.mode {
        Mode::More => menu_size(items),
        Mode::Reactions => picker_size(),
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
        (Action::CopyLink, !chat.message_link(seq).is_empty()),
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
                    chat.copy_text(link.clone(), "message link", cx);
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

/// The reaction picker: a search field that takes the keys, the reader's
/// frequent row, one tab of emoji at a time under a strip of tabs, or the
/// search's matches in their place. Enter picks the first match.
fn reactions(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let writable = chat.may_write();
    let seq = menu.seq;
    let pick = |cx: &mut Context<Chat>, emoji: &str| {
        let emoji = emoji.to_owned();
        writable.then(|| {
            Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
                cx.notify();
                chat.react(seq, emoji.clone(), true, cx)
            })) as Press
        })
    };
    let typed = cx.listener(|chat, query: &String, _, cx| {
        chat.picker.query = query.clone();
        cx.notify();
    });
    let first = emoji::search(&chat.picker.query).first().copied();
    let mut search = Input::new(focus_key(menu.pane, Mode::Reactions))
        .h(px(SEARCH))
        .w_full()
        .px_2()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(px(12.5))
        .value(chat.picker.query.clone())
        .placeholder("Search emoji")
        .label("Search emoji")
        .on_input(typed);
    if let Some(first) = first.filter(|_| writable) {
        search = search.on_submit(cx.listener(move |chat, _: &(), _, cx| {
            cx.notify();
            chat.react(seq, first.into(), true, cx)
        }));
    }
    let mut picker = div()
        .id("chat-reaction-picker")
        .flex()
        .flex_col()
        .gap(px(STACK_GAP))
        .p(px(PICKER_INSET))
        .child(search);
    if chat.picker.query.trim().is_empty() {
        let mut frequent = grid("chat-reaction-frequent");
        for emoji in emoji::frequent(&chat.recent_emoji) {
            let press = pick(cx, &emoji);
            frequent = frequent.child(Reaction::new(
                format!("chat-reaction-{emoji}"),
                &emoji,
                press,
                theme,
            ));
        }
        let tab = chat.picker.tab.min(emoji::CATEGORIES.len() - 1);
        let mut tabs = div()
            .id("chat-reaction-tabs")
            .h(px(TABS))
            .flex()
            .border_b_1()
            .border_color(theme.border);
        for (index, category) in emoji::CATEGORIES.iter().enumerate() {
            let chosen = index == tab;
            let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
                chat.picker.tab = index;
                cx.notify();
            });
            tabs = tabs.child(
                div()
                    .id(format!("chat-reaction-tab-{}", category.name))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .border_b_2()
                    .border_color(if chosen {
                        theme.accent
                    } else {
                        theme.background
                    })
                    .role(Role::Tab)
                    .aria_label(category.name)
                    .aria_selected(chosen)
                    .focusable()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.surface_raised))
                    .on_click(open)
                    .child(category.glyph),
            );
        }
        let category = &emoji::CATEGORIES[tab];
        let mut cells = grid("chat-reaction-grid");
        for (emoji, _) in category.emoji {
            let press = pick(cx, emoji);
            cells = cells.child(Reaction::new(
                format!("chat-reaction-{}-{emoji}", category.name),
                emoji,
                press,
                theme,
            ));
        }
        picker = picker
            .child(caption("Frequently used", theme))
            .child(frequent)
            .child(tabs)
            .child(caption(category.name, theme))
            .child(cells);
    } else {
        let found = emoji::search(&chat.picker.query);
        picker = picker.child(caption(
            &match found.len() {
                0 => "No emoji match".to_owned(),
                1 => "1 match".to_owned(),
                n => format!("{n} matches"),
            },
            theme,
        ));
        let mut cells = grid("chat-reaction-results");
        for emoji in found.into_iter().take(emoji::PER_TAB) {
            let press = pick(cx, emoji);
            cells = cells.child(Reaction::new(
                format!("chat-reaction-{emoji}"),
                emoji,
                press,
                theme,
            ));
        }
        picker = picker.child(cells);
    }
    picker.into_any_element()
}

fn grid(id: &'static str) -> ducktape_view_guest::Stateful<ducktape_view_guest::Div> {
    div().id(id).grid().grid_cols(COLUMNS).gap(px(PICKER_GAP))
}

/// A section's name over its cells, in the data face.
fn caption(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(CAPTION))
        .text_size(px(10.5))
        .font_family(design::fonts::FAMILY_MONO)
        .text_color(theme.muted)
        .child(text.to_uppercase())
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
/// The picker keeps one size whatever it shows, so it never jumps under
/// the pointer as a search narrows it: search, caption, frequent row, tabs,
/// caption, grid.
fn picker_size() -> (f32, f32) {
    let columns = COLUMNS as f32;
    let grid = GRID_ROWS * CELL + (GRID_ROWS - 1.) * PICKER_GAP;
    (
        PICKER_INSET * 2. + columns * CELL + (columns - 1.) * PICKER_GAP,
        PICKER_INSET * 2. + SEARCH + CAPTION + CELL + TABS + CAPTION + grid + 5. * STACK_GAP,
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

#[derive(IntoElement)]
struct Reaction {
    id: ElementId,
    emoji: String,
    press: Option<Press>,
    theme: Theme,
}
impl Reaction {
    fn new(id: impl Into<ElementId>, emoji: &str, press: Option<Press>, theme: &Theme) -> Self {
        Self {
            id: id.into(),
            emoji: emoji.into(),
            press,
            theme: *theme,
        }
    }
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
            .text_size(px(18.))
            .role(Role::Button)
            .aria_label("Add reaction")
            .aria_description(self.emoji.clone())
            .aria_disabled(!enabled)
            .child(self.emoji);
        match self.press {
            // hover and nothing more: each state is a style the frame
            // carries for every cell (see `emoji::PER_TAB`)
            Some(press) => cell
                .focusable()
                .cursor_pointer()
                .hover(|s| s.bg(self.theme.surface_raised))
                .on_click(press)
                .into_any_element(),
            None => cell.opacity(0.4).into_any_element(),
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
        assert_eq!(popup_geometry(&menu, 3), Some(((617., 449.), (220., 100.))));
    }
}
