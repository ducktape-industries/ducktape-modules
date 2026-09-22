//! Components local to this view.
use ducktape_view_guest::prelude::*;

#[derive(IntoElement)]
pub(super) struct Section {
    id: ElementId,
    label: SharedString,
}
impl Section {
    pub(super) fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into() }
    }
}
impl RenderOnce for Section {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div().id(self.id).role(Role::Heading).aria_level(2).h(px(28.)).flex().items_center().pl_2().pr_1()
            .text_size(px(12.)).font_medium().text_color(cx.global::<Theme>().muted).child(self.label)
    }
}

#[derive(IntoElement)]
pub(super) struct EmptyState {
    id: ElementId,
    title: SharedString,
    detail: SharedString,
}
impl EmptyState {
    pub(super) fn new(id: impl Into<ElementId>, title: impl Into<SharedString>, detail: impl Into<SharedString>) -> Self {
        Self { id: id.into(), title: title.into(), detail: detail.into() }
    }
}
impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div().id(self.id).flex().flex_col().gap_1().p_6().max_w(px(420.))
            .child(div().text_size(px(13.5)).font_medium().child(self.title))
            .child(div().text_size(px(12.)).text_color(cx.global::<Theme>().muted).child(self.detail))
    }
}
