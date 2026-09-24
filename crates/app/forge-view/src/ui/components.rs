//! The repeated shapes of this view; the ones every view shares (button,
//! empty state, heading, quiet line) come from `view_guest::design`.
use std::ops::Range;

use ducktape_view_guest::design;
pub(crate) use ducktape_view_guest::design::{button, empty_state, heading};
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Hsla, UniformListScrollHandle};

pub(crate) fn id(text: impl Into<String>) -> ElementId {
    ElementId::Name(text.into().into())
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
            .text_size(design::text::CAPTION)
            .child(self.label)
    }
}

pub(crate) fn loading(id: impl Into<ElementId>, text: &str, theme: &Theme) -> AnyElement {
    div()
        .id(id.into())
        .p_3()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text.to_owned())
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

/// [`design::quiet`], as the screens that return it hand it on.
pub(crate) fn quiet(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    design::quiet(text, theme).into_any_element()
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
