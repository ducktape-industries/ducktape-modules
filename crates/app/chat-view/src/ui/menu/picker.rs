//! The reaction picker: a search field that takes the keys, the reader's
//! frequent row, one tab of emoji at a time, or the search's matches.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, App, ClickEvent, Context, ElementId, RenderOnce, Role, Theme, Window,
};

use super::{
    CAPTION, CELL, COLUMNS, GRID_ROWS, PICKER_GAP, PICKER_INSET, Press, SEARCH, STACK_GAP, TABS,
    focus_key,
};
use crate::{Chat, Menu, Mode, emoji};

/// The reaction picker: a search field that takes the keys, the reader's
/// frequent row, one tab of emoji at a time under a strip of tabs, or the
/// search's matches in their place. Enter picks the first match.
pub(super) fn reactions(
    chat: &Chat,
    menu: &Menu,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let picker = div()
        .id("chat-reaction-picker")
        .size_full()
        .flex()
        .flex_col()
        .gap(px(STACK_GAP))
        .p(px(PICKER_INSET))
        .child(search_field(chat, menu, cx, theme));
    let body = match chat.picker.query.trim().is_empty() {
        true => browse(chat, menu.seq, cx, theme),
        false => matches(chat, menu.seq, cx, theme),
    };
    picker.children(body).into_any_element()
}

/// What choosing `emoji` does: react to `seq`, or nothing where the reader
/// may not write.
fn pick(chat: &Chat, seq: u64, emoji: &str, cx: &mut Context<Chat>) -> Option<Press> {
    let emoji = emoji.to_owned();
    chat.may_write().then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.react(seq, emoji.clone(), true, cx)
        })) as Press
    })
}

/// The field that takes the keys; Enter reacts with its first match.
fn search_field(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> Input {
    let typed = cx.listener(|chat, query: &String, _, cx| {
        chat.picker.query = query.clone();
        cx.notify();
    });
    let search = Input::new(focus_key(menu.pane, Mode::Reactions))
        .h(px(SEARCH))
        .w_full()
        .px_2()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .value(chat.picker.query.clone())
        .placeholder("Search emoji")
        .label("Search emoji")
        .on_input(typed);
    let first = emoji::search(&chat.picker.query).first().copied();
    match first.filter(|_| chat.may_write()) {
        Some(first) => {
            let seq = menu.seq;
            search.on_submit(cx.listener(move |chat, _: &(), _, cx| {
                cx.notify();
                chat.react(seq, first.into(), true, cx)
            }))
        }
        None => search,
    }
}

/// With no search: the frequent row, the tabs, and the open tab's emoji.
fn browse(chat: &Chat, seq: u64, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let mut frequent = grid("chat-reaction-frequent");
    for emoji in emoji::frequent(&chat.recent_emoji) {
        let press = pick(chat, seq, &emoji, cx);
        let id = format!("chat-reaction-{emoji}");
        frequent = frequent.child(Reaction::new(id, &emoji, press, theme));
    }
    let tab = chat.picker.tab.min(emoji::CATEGORIES.len() - 1);
    let category = &emoji::CATEGORIES[tab];
    let mut cells = grid("chat-reaction-grid");
    for (emoji, _) in category.emoji {
        let press = pick(chat, seq, emoji, cx);
        let id = format!("chat-reaction-{}-{emoji}", category.name);
        cells = cells.child(Reaction::new(id, emoji, press, theme));
    }
    vec![
        caption("Frequently used", theme).into_any_element(),
        frequent.into_any_element(),
        tabs(tab, cx, theme).into_any_element(),
        caption(category.name, theme).into_any_element(),
        cells.into_any_element(),
    ]
}

/// A tab per emoji category, `chosen` marked.
fn tabs(chosen: usize, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
    let mut tabs = div()
        .id("chat-reaction-tabs")
        .h(px(TABS))
        .flex()
        .border_b_1()
        .border_color(theme.border);
    for (index, category) in emoji::CATEGORIES.iter().enumerate() {
        let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            chat.picker.tab = index;
            cx.notify();
        });
        let id = format!("chat-reaction-tab-{}", category.name);
        tabs = tabs.child(
            design::tab(id, category.glyph, index == chosen, theme, open)
                .flex_1()
                .h_full()
                .justify_center()
                .text_size(design::text::SECTION)
                .aria_label(category.name)
                .cursor_pointer(),
        );
    }
    tabs
}

/// A search's matches, counted, scrolled in the room the tabs and grid
/// leave.
fn matches(chat: &Chat, seq: u64, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let found = emoji::search(&chat.picker.query);
    let count = match found.len() {
        0 => "No emoji match".to_owned(),
        n => design::plural(n as u64, "match", "matches"),
    };
    let mut cells = grid("chat-reaction-results");
    for emoji in found {
        let press = pick(chat, seq, emoji, cx);
        cells = cells.child(Reaction::new(
            format!("chat-reaction-{emoji}"),
            emoji,
            press,
            theme,
        ));
    }
    let scroll = div()
        .id("chat-reaction-results-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(cells);
    vec![
        caption(&count, theme).into_any_element(),
        scroll.into_any_element(),
    ]
}

fn grid(id: &'static str) -> ducktape_view_guest::Stateful<ducktape_view_guest::Div> {
    div().id(id).grid().grid_cols(COLUMNS).gap(px(PICKER_GAP))
}

/// A section's name over its cells, in the data face.
fn caption(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(CAPTION))
        .text_size(design::text::CAPTION)
        .font_family(design::fonts::FAMILY_MONO)
        .text_color(theme.muted)
        .child(text.to_uppercase())
}

/// The picker keeps one size whatever it shows, so it never jumps under
/// the pointer as a search narrows it: search, caption, frequent row, tabs,
/// caption, grid.
pub(super) fn picker_size() -> (f32, f32) {
    let columns = COLUMNS as f32;
    let grid = GRID_ROWS * CELL + (GRID_ROWS - 1.) * PICKER_GAP;
    (
        PICKER_INSET * 2. + columns * CELL + (columns - 1.) * PICKER_GAP,
        PICKER_INSET * 2. + SEARCH + CAPTION + CELL + TABS + CAPTION + grid + 5. * STACK_GAP,
    )
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
            .text_size(design::text::TITLE)
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
