//! Wire-backed behavior elements that have no native GPUI element equivalent.

use crate::{wire, AnyElement, App, ElementId, IntoElement, Lowering, Window};

type SizeListener = Box<dyn Fn(&(f32, f32), &mut Window, &mut App)>;
type DragListener = Box<dyn Fn(&(f64, f64), &mut Window, &mut App)>;
type UnitListener = Box<dyn Fn(&(), &mut Window, &mut App)>;

fn key(id: ElementId) -> String {
    wire::ElementIdWire::from_gpui(id)
        .expect("element ID must be portable across the view boundary")
        .name()
        .expect("behavior element IDs require names until retained nodes carry ElementIdWire")
        .to_owned()
}

pub struct Sensor {
    id: ElementId,
    child: AnyElement,
    on_show: Option<SizeListener>,
    on_resize: Option<SizeListener>,
}

pub fn sensor(id: impl Into<ElementId>, child: impl IntoElement) -> Sensor {
    Sensor {
        id: id.into(),
        child: child.into_any_element(),
        on_show: None,
        on_resize: None,
    }
}

impl Sensor {
    pub fn on_show(
        mut self,
        listener: impl Fn(&(f32, f32), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_show = Some(Box::new(listener));
        self
    }

    pub fn on_resize(
        mut self,
        listener: impl Fn(&(f32, f32), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_resize = Some(Box::new(listener));
        self
    }
}

impl IntoElement for Sensor {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Sensor {
            key: key(self.id),
            reset: None,
            on_show: self.on_show.map(|listener| lowering.route(listener)),
            on_resize: self.on_resize.map(|listener| lowering.route(listener)),
            on_hide: None,
            anticipate: None,
            delay: None,
            child: Box::new(self.child.into_node(lowering)),
        }
    }
}

pub struct ResizeHandle {
    id: ElementId,
    child: AnyElement,
    on_drag: Option<DragListener>,
    cursor: Option<wire::mouse::Cursor>,
}

pub fn resize_handle(id: impl Into<ElementId>, child: impl IntoElement) -> ResizeHandle {
    ResizeHandle {
        id: id.into(),
        child: child.into_any_element(),
        on_drag: None,
        cursor: Some(wire::mouse::Cursor::ResizingHorizontally),
    }
}

impl ResizeHandle {
    pub fn on_drag(
        mut self,
        listener: impl Fn(&(f64, f64), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drag = Some(Box::new(listener));
        self
    }

    pub fn cursor(mut self, cursor: wire::mouse::Cursor) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

impl IntoElement for ResizeHandle {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::ResizeHandle {
            key: key(self.id),
            on_press: None,
            on_release: None,
            on_drag: self.on_drag.map(|listener| lowering.route(listener)),
            cursor: self.cursor,
            content: Box::new(self.child.into_node(lowering)),
        }
    }
}

pub struct ModalOverlay {
    id: ElementId,
    base: AnyElement,
    modal: AnyElement,
    label: Option<String>,
    padding: f32,
    backdrop: [f32; 4],
    centered: bool,
    on_dismiss: Option<UnitListener>,
}

pub fn modal_overlay(
    id: impl Into<ElementId>,
    base: impl IntoElement,
    modal: impl IntoElement,
) -> ModalOverlay {
    ModalOverlay {
        id: id.into(),
        base: base.into_any_element(),
        modal: modal.into_any_element(),
        label: None,
        padding: 0.,
        backdrop: [0.; 4],
        centered: false,
        on_dismiss: None,
    }
}

impl ModalOverlay {
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }
    pub fn padding(mut self, pixels: f32) -> Self {
        self.padding = pixels;
        self
    }
    pub fn backdrop(mut self, color: [f32; 4]) -> Self {
        self.backdrop = color;
        self
    }
    pub fn on_dismiss(mut self, listener: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(listener));
        self
    }
}

impl IntoElement for ModalOverlay {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Overlay {
            key: key(self.id),
            label: self.label,
            padding: self.padding,
            backdrop: wire::Rgba(self.backdrop),
            align_x: if self.centered {
                wire::AlignX::Center
            } else {
                wire::AlignX::Left
            },
            align_y: if self.centered {
                wire::AlignY::Center
            } else {
                wire::AlignY::Top
            },
            on_dismiss: self
                .on_dismiss
                .map(|listener| lowering.message_route(listener)),
            children: vec![
                self.base.into_node(lowering),
                self.modal.into_node(lowering),
            ],
        }
    }
}

impl gpui::prelude::FluentBuilder for Sensor {}
impl gpui::prelude::FluentBuilder for ResizeHandle {}
impl gpui::prelude::FluentBuilder for ModalOverlay {}
