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
        .size_full()
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
                design::tab(
                    format!("chat-reaction-tab-{}", category.name),
                    category.glyph,
                    chosen,
                    theme,
                    open,
                )
                .flex_1()
                .h_full()
                .justify_center()
                .text_size(px(14.))
                .aria_label(category.name)
                .cursor_pointer(),
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
        // every match, scrolled in the room the tabs and grid leave
        let mut cells = grid("chat-reaction-results");
        for emoji in found {
            let press = pick(cx, emoji);
            cells = cells.child(Reaction::new(
                format!("chat-reaction-{emoji}"),
                emoji,
                press,
                theme,
            ));
        }
        picker = picker.child(
            div()
                .id("chat-reaction-results-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(cells),
        );
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
