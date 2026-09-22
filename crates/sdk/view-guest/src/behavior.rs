//! Wire-backed behavior elements that have no native GPUI element equivalent.
use crate::Element;

use crate::{wire, AnyElement, App, ElementId, IntoElement, Lowering, Window};
use gpui::{
    AbsoluteLength, AlignContent, AlignItems, CursorStyle, DefiniteLength, Hsla, Pixels,
    StyleRefinement, Styled,
};

type SizeListener = Box<dyn Fn(&(Pixels, Pixels), &mut Window, &mut App)>;
type DragListener = Box<dyn Fn(&(Pixels, Pixels), &mut Window, &mut App)>;
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
        listener: impl Fn(&(Pixels, Pixels), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_show = Some(Box::new(listener));
        self
    }

    pub fn on_resize(
        mut self,
        listener: impl Fn(&(Pixels, Pixels), &mut Window, &mut App) + 'static,
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
}

impl Element for Sensor {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Sensor {
            key: key(self.id),
            reset: None,
            on_show: self.on_show.map(|listener| lowering.route(listener)),
            on_resize: self.on_resize.map(|listener| lowering.route(listener)),
            on_hide: None,
            anticipate: None,
            delay: None,
            child: Box::new(lowering.lower(self.child)),
        }
    }
}

pub struct ResizeHandle {
    id: ElementId,
    child: AnyElement,
    on_drag: Option<DragListener>,
    cursor: Option<CursorStyle>,
}

pub fn resize_handle(id: impl Into<ElementId>, child: impl IntoElement) -> ResizeHandle {
    ResizeHandle {
        id: id.into(),
        child: child.into_any_element(),
        on_drag: None,
        cursor: Some(CursorStyle::ResizeLeftRight),
    }
}

impl ResizeHandle {
    pub fn on_drag(
        mut self,
        listener: impl Fn(&(Pixels, Pixels), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drag = Some(Box::new(listener));
        self
    }

    pub fn cursor(mut self, cursor: CursorStyle) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

impl IntoElement for ResizeHandle {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ResizeHandle {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::ResizeHandle {
            key: key(self.id),
            on_press: None,
            on_release: None,
            on_drag: self.on_drag.map(|listener| lowering.route(listener)),
            cursor: self.cursor.map(wire_cursor),
            content: Box::new(lowering.lower(self.child)),
        }
    }
}

pub struct ModalOverlay {
    id: ElementId,
    base: AnyElement,
    modal: AnyElement,
    label: Option<String>,
    style: StyleRefinement,
    backdrop: Hsla,
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
        style: StyleRefinement::default(),
        backdrop: Hsla::transparent_black(),
        on_dismiss: None,
    }
}

impl ModalOverlay {
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn backdrop(mut self, color: impl Into<Hsla>) -> Self {
        self.backdrop = color.into();
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
}

impl Element for ModalOverlay {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Overlay {
            key: key(self.id),
            label: self.label,
            padding: pixel_padding(self.style.padding.top),
            backdrop: wire_rgba(self.backdrop),
            align_x: if self.style.justify_content == Some(AlignContent::Center) {
                wire::AlignX::Center
            } else {
                wire::AlignX::Left
            },
            align_y: if self.style.align_items == Some(AlignItems::Center) {
                wire::AlignY::Center
            } else {
                wire::AlignY::Top
            },
            on_dismiss: self
                .on_dismiss
                .map(|listener| lowering.message_route(listener)),
            children: vec![lowering.lower(self.base), lowering.lower(self.modal)],
        }
    }
}

impl Styled for ModalOverlay {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn pixel_padding(length: Option<DefiniteLength>) -> f32 {
    match length {
        Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(value))) => value.into(),
        _ => 0.,
    }
}

fn wire_rgba(color: Hsla) -> wire::Rgba {
    let color: gpui::Rgba = color.into();
    wire::Rgba([color.r, color.g, color.b, color.a])
}

fn wire_cursor(cursor: CursorStyle) -> wire::mouse::Cursor {
    use wire::mouse::Cursor;
    match cursor {
        CursorStyle::Arrow => Cursor::Idle,
        CursorStyle::IBeam | CursorStyle::IBeamCursorForVerticalLayout => Cursor::Text,
        CursorStyle::Crosshair => Cursor::Crosshair,
        CursorStyle::ClosedHand => Cursor::Grabbing,
        CursorStyle::OpenHand => Cursor::Grab,
        CursorStyle::PointingHand => Cursor::Pointer,
        CursorStyle::ResizeLeft | CursorStyle::ResizeRight | CursorStyle::ResizeLeftRight => {
            Cursor::ResizingHorizontally
        }
        CursorStyle::ResizeUp | CursorStyle::ResizeDown | CursorStyle::ResizeUpDown => {
            Cursor::ResizingVertically
        }
        CursorStyle::ResizeUpLeftDownRight => Cursor::ResizingDiagonallyUp,
        CursorStyle::ResizeUpRightDownLeft => Cursor::ResizingDiagonallyDown,
        CursorStyle::ResizeColumn => Cursor::ResizingColumn,
        CursorStyle::ResizeRow => Cursor::ResizingRow,
        CursorStyle::OperationNotAllowed => Cursor::NotAllowed,
        CursorStyle::DragLink => Cursor::Alias,
        CursorStyle::DragCopy => Cursor::Copy,
        CursorStyle::ContextualMenu => Cursor::ContextMenu,
    }
}

impl gpui::prelude::FluentBuilder for Sensor {}
impl gpui::prelude::FluentBuilder for ResizeHandle {}
impl gpui::prelude::FluentBuilder for ModalOverlay {}
