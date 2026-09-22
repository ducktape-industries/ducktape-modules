//! Guest-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm guest. Lowering turns this small recipe into wire
//! data once per frame.

use crate::{App, Window, slots, wire};
use crate::interactivity::{ClickListener, Interactivity};
use gpui::{ElementId, SharedString, StyleRefinement, Styled};
use std::ops::Range;

/// A value that can be lowered into the SDK wire tree.
pub trait IntoElement: Sized + 'static {
    type Element;

    fn into_element(self) -> Self::Element;

    fn into_any_element(self) -> AnyElement {
        AnyElement(Box::new(self))
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node;
}

trait ErasedElement {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node;
}

impl<T: IntoElement> ErasedElement for T {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        (*self).into_node(lowering)
    }
}

/// A type-erased guest element, used for conditional children and components.
pub struct AnyElement(Box<dyn ErasedElement>);
impl IntoElement for AnyElement {
    type Element = Self;
    fn into_element(self) -> Self { self }
    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        self.0.lower(lowering)
    }
}

/// The explicit lowering context for one driver frame.
pub struct Lowering<'a> {
    window: &'a mut Window,
    app: &'a mut App,
}

impl<'a> Lowering<'a> {
    pub(crate) fn new(window: &'a mut Window, app: &'a mut App) -> Self {
        Self { window, app }
    }

    pub fn window(&mut self) -> &mut Window {
        self.window
    }

    pub fn app(&mut self) -> &mut App {
        self.app
    }

    #[doc(hidden)]
    pub fn render_once(&mut self, component: impl RenderOnce) -> wire::Node {
        component.render(self.window, self.app).into_node(self)
    }

    fn click(&mut self, listener: ClickListener) -> u32 {
        slots::click(&self.app.inner.slots, listener)
    }
}

/// A guest container backed by a real GPUI style refinement.
pub struct Div {
    pub(crate) interactivity: Interactivity,
    children: Vec<Box<dyn ErasedElement>>,
}

impl Default for Div {
    fn default() -> Self {
        Self {
            interactivity: Interactivity::default(),
            children: Vec::new(),
        }
    }
}

impl Styled for Div {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl IntoElement for Div {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        let id = self.interactivity.id.map(wire::ElementIdWire::from_gpui);
        let on_click = self.interactivity.on_click.map(|listener| lowering.click(listener));
        let interactivity = wire::Interactivity {
            role: self.interactivity.role,
            aria: self.interactivity.aria,
            focusable: self.interactivity.focusable,
            id: id.clone(),
            group: self.interactivity.group,
            hover: self.interactivity.hover,
            active: self.interactivity.active,
            group_hover: self
                .interactivity
                .group_hover
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            group_active: self
                .interactivity
                .group_active
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            on_click,
        };
        let children = self
            .children
            .into_iter()
            .map(|child| child.lower(lowering))
            .collect();
        wire::Node::Container {
            id,
            style: self.interactivity.base_style,
            interactivity,
            children,
        }
    }
}

/// Construct an empty guest container.
pub fn div() -> Div {
    Div::default()
}

/// Add children to an element recipe.
pub trait ParentElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>);

    fn child(mut self, child: impl IntoElement) -> Self where Self: Sized {
        self.extend(std::iter::once(child.into_any_element()));
        self
    }

    fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self where Self: Sized {
        self.extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl ParentElement for Div {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements.into_iter().map(|element| element.0));
    }
}

impl IntoElement for wire::Node {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, _lowering: &mut Lowering<'_>) -> wire::Node {
        self
    }
}

impl IntoElement for String {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Text {
            id: None,
            style: StyleRefinement::default(),
            content: self,
            heading: None,
            live: None,
        }
    }
}

impl IntoElement for &'static str {
    type Element = String;

    fn into_element(self) -> String {
        self.to_owned()
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        self.to_owned().into_node(lowering)
    }
}

impl IntoElement for SharedString {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        self.to_string().into_node(lowering)
    }
}

/// A one-shot component with the same call shape as GPUI's `RenderOnce`.
pub trait RenderOnce: 'static {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// An image host primitive. Decoding and painting remain host-owned.
pub struct Img {
    source: String,
}

pub fn img(source: impl Into<String>) -> Img {
    Img {
        source: source.into(),
    }
}

impl IntoElement for Img {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Image {
            key: self.source.clone(),
            hash: stable_hash(self.source.as_bytes()),
            data: Some(wire::ImageData::Resource(self.source)),
            label: None,
            fit: None,
            opacity: None,
            width: None,
            height: None,
        }
    }
}

/// An SVG host primitive carrying bytes through the existing bounded cache.
pub struct Svg {
    bytes: Vec<u8>,
}

pub fn svg(bytes: impl Into<Vec<u8>>) -> Svg {
    Svg { bytes: bytes.into() }
}

impl IntoElement for Svg {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, _lowering: &mut Lowering<'_>) -> wire::Node {
        let hash = stable_hash(&self.bytes);
        wire::Node::Svg {
            key: format!("svg:{hash}"),
            inherit_button_ink: false,
            hash,
            bytes: Some(self.bytes),
            label: None,
            color: None,
            hover: None,
            fit: None,
            opacity: None,
            width: None,
            height: None,
        }
    }
}

/// A host-positioned child primitive.
pub struct Anchored {
    child: Box<dyn ErasedElement>,
}

pub fn anchored(child: impl IntoElement) -> Anchored {
    Anchored {
        child: Box::new(child),
    }
}

impl IntoElement for Anchored {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Pin {
            key: "anchored".into(),
            x: 0.0,
            y: 0.0,
            width: None,
            height: None,
            content: Box::new(self.child.lower(lowering)),
        }
    }
}

/// A deferred child primitive. The host controls when it is painted.
pub struct Deferred {
    child: Box<dyn ErasedElement>,
}

pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: Box::new(child),
    }
}

impl IntoElement for Deferred {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Lazy {
            key: "deferred".into(),
            generation: 0,
            content: Box::new(self.child.lower(lowering)),
        }
    }
}

/// A bounded host canvas command list.
pub struct Canvas {
    commands: Vec<wire::CanvasCommand>,
}

pub fn canvas(commands: impl Into<Vec<wire::CanvasCommand>>) -> Canvas {
    Canvas {
        commands: commands.into(),
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
            commands: self.commands,
        }
    }
}

/// A GPUI-shaped uniform list recipe. The host still owns virtualization.
pub struct UniformList<R> {
    id: ElementId,
    count: usize,
    processor: Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<R>>,
}

pub fn uniform_list<R: IntoElement>(
    id: impl Into<ElementId>,
    count: usize,
    processor: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> UniformList<R> {
    UniformList {
        id: id.into(),
        count,
        processor: Box::new(processor),
    }
}

impl<R: IntoElement> IntoElement for UniformList<R> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        let children = (self.processor)(0..self.count, lowering.window, lowering.app)
            .into_iter()
            .map(|child| child.into_node(lowering))
            .collect();
        wire::Node::KeyedColumn {
            key: wire::ElementIdWire::from_gpui(self.id)
                .name()
                .unwrap_or("uniform-list")
                .to_owned(),
            keys: None,
            background: None,
            border: None,
            spacing: None,
            padding: None,
            width: None,
            height: None,
            max_width: None,
            align: None,
            virtual_row: None,
            children,
        }
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl gpui::prelude::FluentBuilder for Div {}
impl gpui::prelude::FluentBuilder for AnyElement {}
impl gpui::prelude::FluentBuilder for Img {}
impl gpui::prelude::FluentBuilder for Svg {}
impl gpui::prelude::FluentBuilder for Deferred {}
impl gpui::prelude::FluentBuilder for Anchored {}
impl gpui::prelude::FluentBuilder for Canvas {}
impl<R> gpui::prelude::FluentBuilder for UniformList<R> {}
