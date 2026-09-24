//! Small repeated presentation components local to Chat; the shared ones
//! (button, empty state, quiet line) come from `view_guest::design`.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;

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
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(design::text::CAPTION)
            .child(self.label)
    }
}
