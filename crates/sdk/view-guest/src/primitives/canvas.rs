use crate::{IntoElement, Lowering, wire};
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

impl IntoElement for Canvas {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Canvas {
            key: "canvas".into(),
            width: None,
            height: None,
            style: self.style,
            commands: self.commands,
        }
    }
}

impl gpui::prelude::FluentBuilder for Canvas {}
