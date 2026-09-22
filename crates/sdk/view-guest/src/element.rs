//! Guest-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm guest. Lowering turns this small recipe into wire
//! data once per frame.

use crate::{App, Window, slots, wire};
use gpui::{ClickEvent, ElementId, SharedString, StyleRefinement, Styled};
use std::ops::Range;

type ClickListener = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The explicit state carried by guest interactivity until frame lowering.
#[derive(Default)]
pub struct InteractiveState {
    pub(crate) id: Option<ElementId>,
    pub(crate) group: Option<SharedString>,
    pub(crate) hover: Option<StyleRefinement>,
    pub(crate) active: Option<StyleRefinement>,
    pub(crate) group_hover: Option<(SharedString, StyleRefinement)>,
    pub(crate) group_active: Option<(SharedString, StyleRefinement)>,
    pub(crate) on_click: Option<ClickListener>,
}

/// A value that can be lowered into the SDK wire tree.
pub trait IntoElement: Sized + 'static {
    type Element;

    fn into_element(self) -> Self::Element;

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

    fn click(&mut self, listener: ClickListener) -> u32 {
        slots::click(&self.app.inner.slots, listener)
    }
}

/// A guest container backed by a real GPUI style refinement.
pub struct Div {
    pub(crate) style: StyleRefinement,
    pub(crate) interactivity: InteractiveState,
    children: Vec<Box<dyn ErasedElement>>,
}

impl Default for Div {
    fn default() -> Self {
        Self {
            style: StyleRefinement::default(),
            interactivity: InteractiveState::default(),
            children: Vec::new(),
        }
    }
}

impl Styled for Div {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
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
            style: self.style,
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
pub trait ParentElement: Sized {
    fn child(self, child: impl IntoElement) -> Self;

    fn children(self, children: impl IntoIterator<Item = impl IntoElement>) -> Self;
}

impl ParentElement for Div {
    fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(Box::new(child));
        self
    }

    fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| Box::new(child) as Box<dyn ErasedElement>));
        self
    }
}

/// Add basic group and identity declarations to an element recipe.
pub trait InteractiveElement: Sized {
    fn interactive_state(&mut self) -> &mut InteractiveState;

    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactive_state().id = Some(id.into());
        Stateful { element: self }
    }

    fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.interactive_state().group = Some(group.into());
        self
    }

    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactive_state().hover = Some(f(StyleRefinement::default()));
        self
    }

    fn group_hover(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactive_state().group_hover =
            Some((group.into(), f(StyleRefinement::default())));
        self
    }
}

impl InteractiveElement for Div {
    fn interactive_state(&mut self) -> &mut InteractiveState {
        &mut self.interactivity
    }
}

/// The stateful wrapper returned by [`InteractiveElement::id`].
pub struct Stateful<E> {
    pub(crate) element: E,
}

impl<E: Styled> Styled for Stateful<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E: IntoElement> IntoElement for Stateful<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_node(self, lowering: &mut Lowering<'_>) -> wire::Node {
        self.element.into_node(lowering)
    }
}

impl<E: ParentElement> ParentElement for Stateful<E> {
    fn child(self, child: impl IntoElement) -> Self {
        Self {
            element: self.element.child(child),
        }
    }

    fn children(self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        Self {
            element: self.element.children(children),
        }
    }
}

impl<E: InteractiveElement> InteractiveElement for Stateful<E> {
    fn interactive_state(&mut self) -> &mut InteractiveState {
        self.element.interactive_state()
    }
}

/// Stateful interaction methods, named to match GPUI's public authoring API.
pub trait StatefulInteractiveElement: InteractiveElement {
    fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactive_state().active = Some(f(StyleRefinement::default()));
        self
    }

    fn group_active(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactive_state().group_active =
            Some((group.into(), f(StyleRefinement::default())));
        self
    }

    fn on_click(mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.interactive_state().on_click = Some(Box::new(listener));
        self
    }
}

impl<T: InteractiveElement> StatefulInteractiveElement for T {}

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
