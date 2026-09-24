//! Components local to this view.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;

#[derive(IntoElement)]
pub(super) struct Section {
    id: ElementId,
    label: SharedString,
}
impl Section {
    pub(super) fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}
impl RenderOnce for Section {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .role(Role::Heading)
            .aria_level(2)
            .h(px(28.))
            .flex()
            .items_center()
            .pl_2()
            .pr_1()
            .text_size(design::text::SECONDARY)
            .font_weight(ducktape_view_guest::FontWeight::MEDIUM)
            .text_color(cx.global::<Theme>().muted)
            .child(self.label)
    }
}
