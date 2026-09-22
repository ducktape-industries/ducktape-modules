use crate::interactivity::{Interactivity, InteractiveElement, StatefulInteractiveElement};
use crate::{IntoElement, Lowering, wire};
use gpui::{SharedString, StyleRefinement, Styled};

/// A bounded SVG host primitive. Raw bytes are cached by content hash.
pub struct Svg {
    pub(crate) interactivity: Interactivity,
    bytes: Option<Vec<u8>>,
    path: Option<SharedString>,
    style: StyleRefinement,
}

#[track_caller]
pub fn svg() -> Svg {
    Svg {
        interactivity: Interactivity::default(),
        bytes: None,
        path: None,
        style: StyleRefinement::default(),
    }
}

impl Svg {
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.path = Some(path.into());
        self.bytes = None;
        self
    }

    pub fn external_path(self, path: impl Into<SharedString>) -> Self {
        self.path(path)
    }

    pub fn data(mut self, data: &[u8]) -> Self {
        self.bytes = Some(data.to_vec());
        self.path = None;
        self
    }
}

impl Styled for Svg {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for Svg {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for Svg {}

impl IntoElement for Svg {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        let hash = stable_hash(self.bytes.as_deref(), self.path.as_deref());
        let key = self
            .interactivity
            .id
            .as_ref()
            .map(|id| wire::ElementIdWire::from_gpui(id.clone()))
            .and_then(|id| id.name().map(str::to_owned))
            .unwrap_or_else(|| format!("svg:{hash}"));
        let label = self
            .interactivity
            .aria
            .label
            .as_ref()
            .map(ToString::to_string);
        let color = self.style.text.color.map(|color| {
            let color = color.to_rgb();
            wire::Rgba([color.r, color.g, color.b, color.a])
        });
        let interactivity = self.interactivity.into_wire(lowering);
        wire::Node::Svg {
            key,
            inherit_button_ink: false,
            hash,
            bytes: self.bytes,
            path: self.path.map(|path| path.to_string()),
            label,
            color,
            hover: None,
            fit: None,
            opacity: None,
            width: None,
            height: None,
            style: self.style,
            interactivity,
        }
    }
}

fn stable_hash(bytes: Option<&[u8]>, path: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    path.hash(&mut hasher);
    hasher.finish()
}

impl gpui::prelude::FluentBuilder for Svg {}
