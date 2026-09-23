//! The repeated shapes of this view, in chat's visual language: the same
//! Theme tokens, the same row height, the same button, chip and empty state.
//! They live here because the crate boundary forbids reaching into chat-view.
use std::ops::Range;

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{FontWeight, Hsla, UniformListScrollHandle};

pub(crate) fn id(text: impl Into<String>) -> ElementId {
    ElementId::Name(text.into().into())
}

/// A button. Disabled keeps the row visible, drops the route and says so.
#[derive(IntoElement)]
pub(crate) struct Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    label: String,
    theme: Theme,
    enabled: bool,
    primary: bool,
    selected: bool,
    click: F,
}

pub(crate) fn button<F>(
    id: impl Into<ElementId>,
    label: impl Into<String>,
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
}

impl<F> RenderOnce for Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let background = if self.primary {
            theme.primary
        } else if self.selected {
            theme.accent_soft
        } else {
            theme.surface
        };
        let foreground = if self.primary {
            theme.primary_foreground
        } else {
            theme.foreground
        };
        let mut element = div()
            .id(self.id)
            .px_2()
            .py_1()
            .bg(background)
            .text_color(foreground)
            .text_size(px(12.))
            .role(Role::Button)
            .child(self.label);
        if self.enabled {
            element = element
                .hover(|style| style.bg(theme.surface_raised))
                .active(|style| style.bg(theme.accent_soft))
                .focusable()
                .on_click(self.click);
        } else {
            element = element.text_color(theme.muted).aria_disabled(true);
        }
        if self.selected {
            element = element.aria_selected(true);
        }
        element
    }
}

/// A list row: the one interactive line every list of this view uses.
#[derive(IntoElement)]
pub(crate) struct Row<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    theme: Theme,
    selected: bool,
    /// on the ink rail a row wears the sidebar tones, not the surface ones
    sidebar: bool,
    children: Vec<AnyElement>,
    click: Option<F>,
}

pub(crate) fn row<F>(id: impl Into<ElementId>, theme: &Theme) -> Row<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    Row {
        id: id.into(),
        theme: *theme,
        selected: false,
        sidebar: false,
        children: Vec::new(),
        click: None,
    }
}

impl<F> Row<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    pub fn on_click(mut self, click: F) -> Self {
        self.click = Some(click);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn sidebar(mut self, sidebar: bool) -> Self {
        self.sidebar = sidebar;
        self
    }
    pub fn cell(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl<F> RenderOnce for Row<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let mut element = div()
            .id(self.id)
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .min_h(px(28.))
            .px_2()
            .children(self.children);
        let (chosen, hovered) = if self.sidebar {
            (theme.sidebar_raised, theme.sidebar_raised)
        } else {
            (theme.accent_soft, theme.hover)
        };
        if self.selected {
            element = element.bg(chosen).aria_selected(true);
        }
        if let Some(click) = self.click {
            element = element
                .hover(move |style| style.bg(hovered))
                .role(Role::Button)
                .focusable()
                .on_click(click);
        }
        element
    }
}

#[derive(IntoElement)]
pub(crate) struct EmptyState {
    id: ElementId,
    title: String,
    detail: String,
    muted: Hsla,
}

pub(crate) fn empty_state(
    id: impl Into<ElementId>,
    title: impl Into<String>,
    detail: impl Into<String>,
    theme: &Theme,
) -> EmptyState {
    EmptyState {
        id: id.into(),
        title: title.into(),
        detail: detail.into(),
        muted: theme.muted,
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap_1()
            .p_6()
            .max_w(px(420.))
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.title),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(self.muted)
                    .child(self.detail),
            )
    }
}

#[derive(IntoElement)]
pub(crate) struct Chip {
    id: ElementId,
    label: String,
    foreground: Hsla,
    background: Hsla,
}

pub(crate) fn chip(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    foreground: Hsla,
    background: Hsla,
) -> Chip {
    Chip {
        id: id.into(),
        label: label.into(),
        foreground,
        background,
    }
}

impl RenderOnce for Chip {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .px_1()
            .py_0p5()
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(px(11.))
            .child(self.label)
    }
}

pub(crate) fn loading(id: impl Into<ElementId>, text: &str, theme: &Theme) -> AnyElement {
    div()
        .id(id.into())
        .p_3()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text.to_owned())
        .into_any_element()
}

/// A refused read, with the reason and the one thing to do about it.
#[derive(IntoElement)]
pub(crate) struct Refused<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    sentence: String,
    theme: Theme,
    retry: F,
}

pub(crate) fn refused<F>(
    id: impl Into<ElementId>,
    sentence: impl Into<String>,
    theme: &Theme,
    retry: F,
) -> Refused<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    Refused {
        id: id.into(),
        sentence: sentence.into(),
        theme: *theme,
        retry,
    }
}

impl<F> RenderOnce for Refused<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let retry_id = match &self.id {
            ElementId::Name(name) => id(format!("{name}-retry")),
            _ => id("forge-retry"),
        };
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .m_2()
            .border_1()
            .border_color(theme.danger)
            .bg(theme.danger_soft)
            .text_size(px(12.))
            .child(self.sentence)
            .child(
                div()
                    .id(retry_id)
                    .px_2()
                    .py_1()
                    .w(px(64.))
                    .bg(theme.surface)
                    .hover(|style| style.bg(theme.surface_raised))
                    .role(Role::Button)
                    .focusable()
                    .on_click(self.retry)
                    .child("Retry"),
            )
    }
}

/// A section heading, the same weight everywhere.
pub(crate) fn heading(
    element_id: impl Into<ElementId>,
    text: impl Into<String>,
    level: usize,
    theme: &Theme,
) -> AnyElement {
    heading_in(element_id, text, level, theme.foreground)
}

/// The same heading in a chosen tone — the ink rail reads its own.
pub(crate) fn heading_in(
    element_id: impl Into<ElementId>,
    text: impl Into<String>,
    level: usize,
    color: Hsla,
) -> AnyElement {
    div()
        .id(element_id.into())
        .text_size(px(if level == 1 { 16. } else { 13. }))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .role(Role::Heading)
        .aria_level(level)
        .child(text.into())
        .into_any_element()
}

/// Under this many rows a list is drawn whole. Virtualization buys nothing
/// at that size, and a renderer that has not sent a visible range yet — a
/// headless screenshot, a first frame — still gets the screen.
pub(crate) const VIRTUALIZE_ABOVE: usize = 200;

/// A scrolling list of `count` rows. Over [`VIRTUALIZE_ABOVE`] it is virtual;
/// at or under it the rows are drawn whole, because virtualization buys
/// nothing at that size and a renderer that has not asked for a visible range
/// yet — a first frame, a headless screenshot — would otherwise be handed the
/// single measured row instead of the screen.
pub(crate) fn rows(
    element_id: &str,
    count: usize,
    widest: Option<usize>,
    scroll: Option<&UniformListScrollHandle>,
    paint: impl Fn(usize) -> AnyElement + 'static,
) -> AnyElement {
    if count <= VIRTUALIZE_ABOVE {
        let mut column = div()
            .id(id(element_id.to_owned()))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll();
        for index in 0..count {
            column = column.child(paint(index));
        }
        return column.into_any_element();
    }
    let mut list = uniform_list(
        id(element_id.to_owned()),
        count,
        move |range: Range<usize>, _, _| range.map(&paint).collect::<Vec<_>>(),
    )
    .with_width_from_item(widest);
    if let Some(scroll) = scroll {
        list = list.track_scroll(scroll);
    }
    list.flex_1().min_h(px(0.)).into_any_element()
}

pub(crate) fn quiet(text: impl Into<String>, theme: &Theme) -> AnyElement {
    div()
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text.into())
        .into_any_element()
}

/// One monospaced source line, as the code and diff screens draw it.
pub(crate) fn mono(text: impl Into<String>, color: Hsla) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .font_family("monospace")
        .text_size(px(12.))
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
        .into_any_element()
}

/// A path's bytes as something a screen can say out loud.
pub(crate) fn path_text(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

/// A ref's full name shortened to what a reader calls it.
pub(crate) fn ref_label(name: &[u8]) -> String {
    let text = path_text(name);
    text.strip_prefix("refs/heads/")
        .or_else(|| text.strip_prefix("refs/tags/"))
        .unwrap_or(&text)
        .to_owned()
}

pub(crate) fn short_oid(oid: &str) -> String {
    oid.chars().take(8).collect()
}
