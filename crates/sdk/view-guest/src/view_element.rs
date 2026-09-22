use crate::element::{Element, IntoElement, Lowering, RenderOnce};
use crate::wire;

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
