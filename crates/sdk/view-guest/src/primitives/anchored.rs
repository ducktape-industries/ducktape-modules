use crate::{AnyElement, IntoElement, Lowering, ParentElement, wire};
use gpui::{Anchor, AnchoredFitMode, AnchoredPositionMode, Edges, Pixels, Point};

/// A native GPUI anchored element lowered as a bounded host primitive.
pub struct Anchored {
    children: Vec<AnyElement>,
    anchor: wire::Anchor,
    fit: wire::AnchoredFitMode,
    position: Option<[f32; 2]>,
    position_mode: wire::AnchoredPositionMode,
    offset: Option<[f32; 2]>,
}

#[track_caller]
pub fn anchored() -> Anchored {
    Anchored {
        children: Vec::new(),
        anchor: wire::Anchor::TopLeft,
        fit: wire::AnchoredFitMode::SwitchAnchor,
        position: None,
        position_mode: wire::AnchoredPositionMode::Window,
        offset: None,
    }
}

impl Anchored {
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = to_wire_anchor(anchor);
        self
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = Some([f32::from(position.x), f32::from(position.y)]);
        self
    }

    pub fn offset(mut self, offset: Point<Pixels>) -> Self {
        self.offset = Some([f32::from(offset.x), f32::from(offset.y)]);
        self
    }

    pub fn position_mode(mut self, mode: AnchoredPositionMode) -> Self {
        self.position_mode = to_wire_position_mode(mode);
        self
    }

    pub fn snap_to_window(mut self) -> Self {
        self.fit = wire::AnchoredFitMode::SnapToWindow;
        self
    }

    pub fn snap_to_window_with_margin(mut self, edges: impl Into<Edges<Pixels>>) -> Self {
        let edges = edges.into();
        self.fit = wire::AnchoredFitMode::SnapToWindowWithMargin(wire::Edges {
            top: f32::from(edges.top),
            right: f32::from(edges.right),
            bottom: f32::from(edges.bottom),
            left: f32::from(edges.left),
        });
        self
    }
}

impl ParentElement for Anchored {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl IntoElement for Anchored {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Anchored {
            key: "anchored".into(),
            anchor: self.anchor,
            fit: self.fit,
            position: self.position,
            position_mode: self.position_mode,
            offset: self.offset,
            children: self
                .children
                .into_iter()
                .map(|child| child.into_node(lowering))
                .collect(),
        }
    }
}

fn to_wire_anchor(value: Anchor) -> wire::Anchor {
    match value {
        Anchor::TopLeft => wire::Anchor::TopLeft,
        Anchor::TopRight => wire::Anchor::TopRight,
        Anchor::BottomLeft => wire::Anchor::BottomLeft,
        Anchor::BottomRight => wire::Anchor::BottomRight,
        Anchor::TopCenter => wire::Anchor::TopCenter,
        Anchor::BottomCenter => wire::Anchor::BottomCenter,
        Anchor::LeftCenter => wire::Anchor::LeftCenter,
        Anchor::RightCenter => wire::Anchor::RightCenter,
    }
}

fn to_wire_position_mode(value: AnchoredPositionMode) -> wire::AnchoredPositionMode {
    match value {
        AnchoredPositionMode::Window => wire::AnchoredPositionMode::Window,
        AnchoredPositionMode::Local => wire::AnchoredPositionMode::Local,
    }
}

impl gpui::prelude::FluentBuilder for Anchored {}
