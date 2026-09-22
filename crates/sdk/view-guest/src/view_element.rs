use crate::element::{AnyElement, Element, IntoElement, Lowering, RenderOnce};
use crate::wire;

/// Type-erased entity-backed view, matching native GPUI's `AnyView` shape.
/// Guest views remain `View` entities because the wasm driver owns no native entity arena.
pub struct AnyView {
    render: Box<dyn FnOnce(&mut crate::Window, &mut crate::App) -> AnyElement>,
}

impl<V: crate::View> From<crate::Entity<V>> for AnyView {
    fn from(entity: crate::Entity<V>) -> Self {
        Self {
            render: Box::new(move |window, app| {
                entity.update_in_window(app, window, |view, window, cx| {
                    view.render(window, cx).into_any_element()
                })
            }),
        }
    }
}

impl RenderOnce for AnyView {
    fn render(self, window: &mut crate::Window, cx: &mut crate::App) -> impl IntoElement {
        (self.render)(window, cx)
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<Self>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

/// The guest counterpart of GPUI's `ViewElement`: it defers a `RenderOnce`
/// component until the lowering pass reaches this element.
#[doc(hidden)]
pub struct ViewElement<V: RenderOnce> {
    view: V,
}

impl<V: RenderOnce> ViewElement<V> {
    #[track_caller]
    pub fn new(view: V) -> Self {
        Self { view }
    }
}

impl<V: RenderOnce> Element for ViewElement<V> {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.render_once(self.view)
    }
}

impl<V: RenderOnce> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<V: RenderOnce> gpui::prelude::FluentBuilder for ViewElement<V> {}
