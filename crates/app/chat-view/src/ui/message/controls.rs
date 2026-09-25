//! The small controls on a message card: the action strip's buttons, a
//! reaction, and the way into a thread.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{ClickEvent, ElementId, ParentElement, Styled, Theme, Window, div, px};

pub(super) fn action_button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    accessible: &str,
    theme: &Theme,
    enabled: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    let control = div()
        .id(id)
        .w(px(26.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background)
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.faint
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_label(accessible)
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY)
        .child(label.into());
    if enabled {
        control
            .focusable()
            .cursor_pointer()
            .hover(|s| s.bg(theme.surface_raised))
            .focus_visible(|s| s.bg(theme.surface_raised))
            .on_click(click)
    } else {
        control
    }
}

/// What a reaction button shows: an emoji and how many chose it, or the
/// `+` that opens the picker.
pub(super) enum Face<'a> {
    Emoji { emoji: &'a str, count: u64 },
    Add,
}

pub(super) fn reaction_button(
    id: impl Into<ElementId>,
    face: Face,
    mine: bool,
    theme: &Theme,
    enabled: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    // `+` is the picker's door, not a toggle
    let add = matches!(face, Face::Add);
    let mut control = div()
        .id(id)
        .h(px(22.))
        .px(design::space::XS)
        .flex()
        .items_center()
        .gap_1()
        .border_1()
        // the reader's own wear the strong line: accent_soft is the
        // hover grey in the calm palette, so a fill alone can't say "mine"
        .border_color(if mine { theme.accent } else { theme.border })
        .bg(if mine {
            theme.accent_soft
        } else {
            theme.background
        })
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.muted
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_label(if add {
            "Add reaction"
        } else if mine {
            "Remove reaction"
        } else {
            "Add reaction"
        })
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY);
    control = match face {
        // the count in the data face, as every count here is
        Face::Emoji { emoji, count } => control
            .aria_description(emoji.to_owned())
            .aria_toggled(mine.into())
            .child(emoji.to_owned())
            .child(
                div()
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::CAPTION)
                    .child(count.to_string()),
            ),
        Face::Add => control.child("+"),
    };
    if enabled {
        control
            .focusable()
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(theme.surface_raised)
                    .border_color(theme.border_strong)
            })
            .focus_visible(|style| style.border_color(theme.accent))
            .on_click(click)
    } else {
        control
    }
}

/// Under a message with replies: how many, and the way into them, drawn as
/// the button it is.
pub(super) fn replies_button(
    id: impl Into<ElementId>,
    count: u64,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    let noun = if count == 1 { "reply" } else { "replies" };
    div()
        .id(id)
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap(design::space::XS)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(theme.surface_raised)
                .border_color(theme.border_strong)
        })
        .active(|style| style.bg(theme.accent_soft))
        .focus_visible(|style| style.border_color(theme.accent))
        .role(ducktape_view_guest::Role::Button)
        .aria_label(format!("Open thread, {count} {noun}"))
        .focusable()
        .on_click(click)
        .child(
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .child(count.to_string()),
        )
        .child(noun)
        .child(div().text_color(theme.muted).child("Open thread →"))
}
