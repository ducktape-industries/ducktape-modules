//! Small repeated presentation components local to Chat.

use ducktape_view_guest::prelude::*;

#[derive(IntoElement)]
pub(crate) struct Button<F> {
    id: ElementId,
    label: String,
    theme: Theme,
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
        click,
    }
}

impl<F> RenderOnce for Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .px_2()
            .py_1()
            .rounded_md()
            .bg(self.theme.surface)
            .hover(|style| style.bg(self.theme.surface_raised))
            .active(|style| style.bg(self.theme.accent_soft))
            .role(Role::Button)
            .focusable()
            .on_click(self.click)
            .child(self.label)
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
            .child(div().text_base().child(self.title))
            .child(div().text_sm().text_color(self.muted).child(self.detail))
    }
}

#[derive(IntoElement)]
pub(crate) struct Badge {
    id: ElementId,
    label: String,
    foreground: Hsla,
    background: Hsla,
}

pub(crate) fn badge(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    foreground: Hsla,
    background: Hsla,
) -> Badge {
    Badge {
        id: id.into(),
        label: label.into(),
        foreground,
        background,
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(self.background)
            .text_color(self.foreground)
            .text_xs()
            .child(self.label)
    }
}
