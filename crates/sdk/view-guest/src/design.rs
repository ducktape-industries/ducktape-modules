//! The shapes every view repeats, in one visual language: the design tokens
//! (re-exported whole), the type scale as [`Pixels`], and the empty state,
//! button, refused-with-retry screen, quiet line, heading and mono run views
//! used to copy between them. The number formatters sit here for the same
//! reason.
pub use ::design::*;

use crate::prelude::*;
use crate::{Div, FontWeight, Stateful};

/// [`type_scale`] as sizes an element takes.
pub mod text {
    use crate::{px, Pixels};

    pub const TITLE: Pixels = px(::design::type_scale::TITLE as f32);
    pub const SECTION: Pixels = px(::design::type_scale::SECTION as f32);
    pub const BODY: Pixels = px(::design::type_scale::BODY as f32);
    pub const SECONDARY: Pixels = px(::design::type_scale::SECONDARY as f32);
    pub const CAPTION: Pixels = px(::design::type_scale::CAPTION as f32);
    pub const MONO: Pixels = px(::design::type_scale::MONO as f32);
}

/// Nothing to show yet: what is missing, then what would fill it.
pub fn empty_state(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .p_6()
        .max_w(px(420.))
        .child(
            div()
                .text_size(text::SECTION)
                .font_weight(FontWeight::MEDIUM)
                .child(title.into()),
        )
        .child(
            div()
                .text_size(text::SECONDARY)
                .text_color(theme.muted)
                .child(detail.into()),
        )
}

/// A refused read: the reason, and Retry. The screen is `{name}-refused`,
/// its retry control `{name}-retry`.
pub fn refused(
    name: &str,
    sentence: impl Into<SharedString>,
    theme: &Theme,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(ElementId::Name(format!("{name}-refused").into()))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(theme.danger)
        .bg(theme.danger_soft)
        .text_size(text::SECONDARY)
        .child(sentence.into())
        .child(
            div()
                .id(ElementId::Name(format!("{name}-retry").into()))
                .px_2()
                .py_1()
                .w(px(64.))
                .bg(theme.surface)
                .hover(move |style| style.bg(theme.surface_raised))
                .role(Role::Button)
                .focusable()
                .on_click(retry)
                .child("Retry"),
        )
}

/// One muted line of status text.
pub fn quiet(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(text::SECONDARY)
        .text_color(theme.muted)
        .child(text.into())
}

/// A heading: the title size at level 1, the section size under it.
pub fn heading(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    level: usize,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .text_size(if level == 1 {
            text::TITLE
        } else {
            text::SECTION
        })
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .role(Role::Heading)
        .aria_level(level)
        .child(text.into())
}

/// A run of monospaced text on one line: hashes, keys, code.
pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(fonts::FAMILY_MONO)
        .text_size(text::MONO)
        .whitespace_nowrap()
        .child(text.into())
}

/// A button. Disabled keeps it visible, drops the click and says so.
#[derive(IntoElement)]
pub struct Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    label: SharedString,
    theme: Theme,
    enabled: bool,
    primary: bool,
    selected: bool,
    /// a tab: quiet text, the chosen one underlined, no fill
    tab: bool,
    /// a choice among many: no fill, muted text, the chosen one an fg edge
    /// like any selected button
    quiet: bool,
    click: F,
}

pub fn button<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    click: F,
) -> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    Button {
        id: id.into(),
        label: label.into(),
        theme: *theme,
        enabled: true,
        primary: false,
        selected: false,
        tab: false,
        quiet: false,
        click,
    }
}

impl<F> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn tab(mut self, tab: bool) -> Self {
        self.tab = tab;
        self
    }
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

impl<F> RenderOnce for Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let mut element = div()
            .id(self.id)
            .px_2()
            .py_1()
            .text_size(text::SECONDARY)
            .role(if self.tab { Role::Tab } else { Role::Button })
            .child(self.label);
        // The chosen one is the ink one: fg text on the window, an fg edge
        // (under a tab, around a button); the rest stay quiet.
        element = if self.tab {
            element
                .text_color(if self.selected {
                    theme.foreground
                } else {
                    theme.muted
                })
                .border_b_2()
                .border_color(if self.selected {
                    theme.foreground
                } else {
                    theme.background
                })
        } else if self.primary {
            element
                .bg(theme.primary)
                .text_color(theme.primary_foreground)
        } else if self.selected {
            element
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.foreground)
        } else if self.quiet {
            element
                .text_color(theme.muted)
                .border_1()
                .border_color(theme.background)
        } else {
            element
                .bg(theme.surface)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.surface)
        };
        if self.selected {
            element = element.font_weight(FontWeight::MEDIUM);
        }
        if self.enabled {
            let plain = !self.primary && !self.selected;
            let (tab, quiet) = (self.tab || (self.quiet && plain), plain);
            element = element
                .hover(move |style| match (tab, quiet) {
                    (true, _) => style.text_color(theme.foreground),
                    (false, true) => style.bg(theme.surface_raised),
                    (false, false) => style,
                })
                .focusable()
                .on_click(self.click);
            if quiet && !tab {
                element = element.active(move |style| style.bg(theme.accent_soft));
            }
        } else {
            element = element.text_color(theme.muted).aria_disabled(true);
        }
        if self.selected {
            element = element.aria_selected(true);
        }
        element
    }
}

/// `6230` → `6,230`.
pub fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `1 block`, `1,200 blocks`.
pub fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_read_grouped_and_agreed() {
        assert_eq!(super::grouped(0), "0");
        assert_eq!(super::grouped(999), "999");
        assert_eq!(super::grouped(6230), "6,230");
        assert_eq!(super::grouped(1_048_576), "1,048,576");
        assert_eq!(super::plural(1, "block", "blocks"), "1 block");
        assert_eq!(super::plural(1200, "block", "blocks"), "1,200 blocks");
    }
}
