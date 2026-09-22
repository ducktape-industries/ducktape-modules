use crate::{wire, Element, IntoElement, Lowering};
use gpui::{StyleRefinement, Styled};

/// A bounded declarative host canvas. Native GPUI closures cannot cross the guest ABI.
pub struct Canvas {
    commands: Vec<wire::CanvasCommand>,
    style: StyleRefinement,
}

pub fn canvas(commands: impl Into<Vec<wire::CanvasCommand>>) -> Canvas {
    Canvas {
        commands: commands.into(),
        style: StyleRefinement::default(),
    }
}

impl Styled for Canvas {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Element for Canvas {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        let this = *self;
        wire::Node::Canvas {
            style: this.style,
            commands: this.commands,
        }
    }
}

impl IntoElement for Canvas {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Canvas {}
