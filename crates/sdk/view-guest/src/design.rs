//! The shapes every view repeats, in one visual language: the design tokens
//! (re-exported whole), the type scale, heights and spacing as [`Pixels`]
//! (`text`, `size`, `space`), and the empty state,
//! button, refused-with-retry screen, quiet line, heading and mono run views
//! used to copy between them. The number formatters sit here for the same
//! reason.
pub use ::design::*;

use crate::prelude::*;
use crate::{Div, FontWeight, Hsla, Pixels, Stateful};

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

/// [`height`] as sizes an element takes.
pub mod size {
    use crate::{px, Pixels};

    pub const ROW: Pixels = px(::design::height::ROW as f32);
    pub const CONTROL: Pixels = px(::design::height::CONTROL as f32);
    pub const AVATAR_SM: Pixels = px(::design::height::AVATAR_SM as f32);
    pub const AVATAR: Pixels = px(::design::height::AVATAR as f32);
    pub const AVATAR_LG: Pixels = px(::design::height::AVATAR_LG as f32);
}

/// [`spacing`] as gaps and insets an element takes.
pub mod space {
    use crate::{px, Pixels};

    pub const HAIR: Pixels = px(::design::spacing::HAIR as f32);
    pub const XXS: Pixels = px(::design::spacing::XXS as f32);
    pub const XS: Pixels = px(::design::spacing::XS as f32);
    pub const SM: Pixels = px(::design::spacing::SM as f32);
    pub const MD: Pixels = px(::design::spacing::MD as f32);
    pub const LG: Pixels = px(::design::spacing::LG as f32);
    pub const BLOCK: Pixels = px(::design::spacing::BLOCK as f32);
    pub const XL: Pixels = px(::design::spacing::XL as f32);
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

/// What a [`Button`] is among its neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// a surface fill
    #[default]
    Plain,
    /// the one action a screen leads with: the primary fill
    Primary,
    /// a choice among many: no fill, muted text
    Quiet,
}

/// A button. Disabled keeps it visible, drops the click and says so.
/// Selected is the chosen one: fg text on the window and an fg edge.
#[derive(IntoElement)]
pub struct Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    label: SharedString,
    theme: Theme,
    enabled: bool,
    kind: Kind,
    selected: bool,
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
        kind: Kind::Plain,
        selected: false,
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
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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
            .role(Role::Button)
            .child(self.label);
        // The chosen one is the ink one: fg text on the window, an fg edge
        // around it; the rest stay quiet.
        element = match (self.kind, self.selected) {
            (Kind::Primary, _) => element
                .bg(theme.primary)
                .text_color(theme.primary_foreground),
            (_, true) => element
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.foreground),
            (Kind::Quiet, false) => element
                .text_color(theme.muted)
                .border_1()
                .border_color(theme.background),
            (Kind::Plain, false) => element
                .bg(theme.surface)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.surface),
        };
        if self.selected {
            element = element.font_weight(FontWeight::MEDIUM).aria_selected(true);
        }
        if !self.enabled {
            return element.text_color(theme.muted).aria_disabled(true);
        }
        element = match (self.kind, self.selected) {
            (Kind::Quiet, false) => element.hover(move |style| style.text_color(theme.foreground)),
            (Kind::Plain, false) => element
                .hover(move |style| style.bg(theme.surface_raised))
                .active(move |style| style.bg(theme.accent_soft)),
            _ => element,
        };
        element.focusable().on_click(self.click)
    }
}

/// A tab: quiet text, the chosen one fg and underlined, no fill. A caller
/// sizes it to its bar (`h_full`, `flex_1`) and may label a glyph.
pub fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .flex()
        .items_center()
        .px_2()
        .py_1()
        .text_size(text::SECONDARY)
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted
        })
        .border_b_2()
        .border_color(if selected {
            theme.foreground
        } else {
            theme.background
        })
        .when(selected, |tab| tab.font_weight(FontWeight::MEDIUM))
        .hover(move |style| style.text_color(theme.foreground))
        .role(Role::Tab)
        .aria_selected(selected)
        .focusable()
        .on_click(click)
        .child(label.into())
}

/// A person's round initial at `size`, on the raised surface. A caller
/// recolours it (an agent, a speaker) with `bg` / `text_color`.
pub fn avatar(name: &str, size: Pixels, theme: &Theme) -> Div {
    div()
        .size(size)
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.surface_raised)
        .text_color(theme.muted)
        .text_size(size * 0.45)
        .child(initial(name))
}

/// The line between two panes, dragged to move it: `drag` takes the
/// horizontal delta (and clamps the layout it moves).
pub fn divider<V: crate::View>(
    id: impl Into<ElementId>,
    theme: &Theme,
    cx: &mut crate::Context<V>,
    drag: impl Fn(&mut V, f32) + 'static,
) -> crate::ResizeHandle {
    let dragged = cx.listener(move |view, delta: &(Pixels, Pixels), _window, cx| {
        drag(view, delta.0.into());
        cx.notify();
    });
    crate::resize_handle(id, div().w(crate::px(1.)).h_full().bg(theme.border)).on_drag(dragged)
}

/// A small tag: a state, a role, a count, in its own colours.
pub fn badge(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    foreground: Hsla,
    background: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_1()
        .py_0p5()
        .bg(background)
        .text_color(foreground)
        .text_size(text::CAPTION)
        .child(label.into())
}

/// Explorer's pages as short `duck://explorer/<path>` links: the one
/// spelling every view opens a block, a transaction or an account by.
/// Explorer's own `Route::path` writes the same paths through these.
pub mod explorer {
    /// `block/<height>`
    pub fn block_path(height: u64) -> String {
        format!("block/{height}")
    }

    /// `tx/<hash hex>`
    pub fn tx_path(hash: &[u8]) -> String {
        let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("tx/{hex}")
    }

    /// `account/<number>`
    pub fn account_path(number: u64) -> String {
        format!("account/{number}")
    }

    /// `duck://explorer/<path>`: the host opens Explorer at `path`.
    pub fn link(path: &str) -> String {
        format!("duck://explorer/{path}")
    }
}

/// `block 1,024`, quiet and mono, opening Explorer at that block.
pub fn block_link(id: impl Into<ElementId>, height: u64, theme: &Theme) -> Stateful<Div> {
    let label = format!("block {}", grouped(height));
    explorer_link(id, label, explorer::block_path(height), theme)
}

/// A transaction's short hash, opening Explorer at that transaction.
pub fn tx_link(id: impl Into<ElementId>, hash: &[u8], theme: &Theme) -> Stateful<Div> {
    let path = explorer::tx_path(hash);
    let label = short_hex(&path["tx/".len()..]);
    explorer_link(id, label, path, theme)
}

/// `account 7`, opening Explorer at that account.
pub fn account_link(id: impl Into<ElementId>, number: u64, theme: &Theme) -> Stateful<Div> {
    let label = format!("account {number}");
    explorer_link(id, label, explorer::account_path(number), theme)
}

/// Subdued mono text that underlines under the pointer and opens
/// Explorer at `path` through `link.open`.
fn explorer_link(
    id: impl Into<ElementId>,
    label: String,
    path: String,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    let link = explorer::link(&path);
    div()
        .id(id)
        .text_size(text::CAPTION)
        .text_color(theme.muted)
        .font_family(fonts::FAMILY_MONO)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.foreground).text_decoration_1())
        .role(Role::Link)
        .aria_label(format!("Open {label} in Explorer"))
        .focusable()
        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.host().open_link(&link))
        .child(label)
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

/// An avatar's letter: the first grapheme of `name`, uppercased where
/// that applies (`alice` → `A`, `김민지` → `김`), else `•`.
pub fn initial(name: &str) -> String {
    unicode_segmentation::UnicodeSegmentation::graphemes(name.trim_start(), true)
        .next()
        .map_or_else(|| "•".into(), str::to_uppercase)
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

    #[test]
    fn explorer_links_spell_explorers_paths() {
        use super::explorer::*;
        assert_eq!(link(&block_path(30)), "duck://explorer/block/30");
        assert_eq!(link(&tx_path(&[0xab, 0x01])), "duck://explorer/tx/ab01");
        assert_eq!(link(&account_path(7)), "duck://explorer/account/7");
    }

    #[test]
    fn an_initial_is_the_first_grapheme() {
        assert_eq!(super::initial("alice park"), "A");
        assert_eq!(super::initial("김민지"), "김");
        assert_eq!(super::initial(" 한글"), "한");
        assert_eq!(super::initial("e\u{301}va"), "E\u{301}");
        assert_eq!(super::initial(""), "•");
    }
}
