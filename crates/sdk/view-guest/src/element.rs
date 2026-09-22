//! Guest-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm guest. Lowering turns this small recipe into wire
//! data once per frame.

use crate::interactivity::{ClickListener, Interactivity};
use crate::{slots, wire, App, Window};
use gpui::{ElementId, SharedString, StyleRefinement, Styled};
use std::borrow::Cow;
use std::ops::Range;

/// A guest element that can be lowered by the driver.
///
/// This is intentionally a guest-side boundary with the same name as GPUI's
/// native trait. GPUI's real `Element` requires native layout and paint state;
/// a wasm guest has neither, so lowering is the only operation it can perform.
pub trait Element: 'static + IntoElement {
    /// The authored identity that enters the typed ancestry while this element lowers.
    fn id(&self) -> Option<ElementId> {
        None
    }

    #[doc(hidden)]
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node;

    #[doc(hidden)]
    fn into_any(self) -> AnyElement {
        AnyElement(Box::new(self))
    }
}

/// A value that can be converted into a guest element recipe.
pub trait IntoElement: Sized {
    type Element: Element;

    fn into_element(self) -> Self::Element;

    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

trait ElementObject {
    fn id(&self) -> Option<ElementId>;
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node;
}

impl<T: Element> ElementObject for T {
    fn id(&self) -> Option<ElementId> {
        Element::id(self)
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(self, lowering)
    }
}

/// A type-erased guest element, used for conditional children and components.
pub struct AnyElement(Box<dyn ElementObject>);

impl Element for AnyElement {
    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        self.0.lower(lowering)
    }
}

impl IntoElement for AnyElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_any_element(self) -> AnyElement {
        self
    }
}

/// The explicit lowering context for one driver frame.
pub struct Lowering<'a> {
    window: &'a mut Window,
    app: &'a mut App,
    authored_path: Vec<wire::ElementIdWire>,
}

impl<'a> Lowering<'a> {
    pub(crate) fn new(window: &'a mut Window, app: &'a mut App) -> Self {
        Self {
            window,
            app,
            authored_path: Vec::new(),
        }
    }

    pub fn window(&mut self) -> &mut Window {
        self.window
    }

    pub fn app(&mut self) -> &mut App {
        self.app
    }

    #[doc(hidden)]
    pub fn render_once(&mut self, component: impl RenderOnce) -> wire::Node {
        let element = component.render(self.window, self.app).into_element();
        self.lower_element(element)
    }

    pub(crate) fn lower<E: IntoElement>(&mut self, element: E) -> wire::Node {
        self.lower_element(element.into_element())
    }

    pub(crate) fn lower_element<E: Element>(&mut self, element: E) -> wire::Node {
        let id = element
            .id()
            .map(wire::ElementIdWire::from_gpui)
            .transpose()
            .expect("element ID must be portable across the view boundary");
        if let Some(id) = &id {
            self.authored_path.push(id.clone());
        }
        let node = Element::lower(Box::new(element), self);
        if id.is_some() {
            self.authored_path.pop();
        }
        node
    }

    pub(crate) fn current_path(&self) -> &[wire::ElementIdWire] {
        &self.authored_path
    }

    fn click(&mut self, listener: ClickListener) -> u32 {
        slots::click(&self.app.inner.slots, listener)
    }

    pub(crate) fn route<A: 'static>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> u32 {
        slots::route(&self.app.inner.slots, listener)
    }

    pub(crate) fn message_route(
        &mut self,
        listener: impl Fn(&(), &mut Window, &mut App) + 'static,
    ) -> u32 {
        slots::message_route(&self.app.inner.slots, listener)
    }
}

/// A guest container backed by a real GPUI style refinement.
pub struct Div {
    pub(crate) interactivity: Interactivity,
    children: Vec<AnyElement>,
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

impl Element for Div {
    fn id(&self) -> Option<ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            interactivity,
            children,
        } = *self;
        let id = interactivity.id.map(|_| {
            lowering
                .current_path()
                .last()
                .cloned()
                .expect("identified div must lower inside its authored scope")
        });
        let on_click = interactivity
            .on_click
            .map(|listener| lowering.click(listener));
        let wire_interactivity = wire::Interactivity {
            role: interactivity.role,
            aria: interactivity.aria,
            focusable: interactivity.focusable,
            group: interactivity.group,
            hover: interactivity.hover,
            active: interactivity.active,
            group_hover: interactivity
                .group_hover
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            group_active: interactivity
                .group_active
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            on_click,
        };
        let children = children
            .into_iter()
            .map(|child| lowering.lower_element(child))
            .collect();
        wire::Node::Container {
            id,
            style: interactivity.base_style,
            interactivity: wire_interactivity,
            children,
        }
    }
}

impl IntoElement for Div {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Construct an empty guest container.
pub fn div() -> Div {
    Div::default()
}

/// Add children to an element recipe.
pub trait ParentElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>);

    fn child(mut self, child: impl IntoElement) -> Self
    where
        Self: Sized,
    {
        self.extend(std::iter::once(child.into_any_element()));
        self
    }

    fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self
    where
        Self: Sized,
    {
        self.extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl ParentElement for Div {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Element for SharedString {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Text {
            id: None,
            style: StyleRefinement::default(),
            content: self.to_string(),
            heading: None,
            live: None,
        }
    }
}

impl Element for &'static str {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.lower((*self).to_owned())
    }
}

impl IntoElement for String {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

impl IntoElement for &'static str {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for SharedString {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for Cow<'static, str> {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
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

impl Element for Img {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Image {
            key: self.source.clone(),
            hash: stable_hash(self.source.as_bytes()),
            data: Some(wire::ImageData::Resource(self.source.clone())),
            label: None,
            fit: None,
            opacity: None,
            width: None,
            height: None,
        }
    }
}

impl IntoElement for Img {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// An SVG host primitive carrying bytes through the existing bounded cache.
pub struct Svg {
    bytes: Vec<u8>,
}

pub fn svg(bytes: impl Into<Vec<u8>>) -> Svg {
    Svg {
        bytes: bytes.into(),
    }
}

impl Element for Svg {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        let hash = stable_hash(&self.bytes);
        wire::Node::Svg {
            key: format!("svg:{hash}"),
            inherit_button_ink: false,
            hash,
            bytes: Some(self.bytes.clone()),
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

impl IntoElement for Svg {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// A host-positioned child primitive.
pub struct Anchored {
    child: AnyElement,
}

pub fn anchored(child: impl IntoElement) -> Anchored {
    Anchored {
        child: child.into_any_element(),
    }
}

impl Element for Anchored {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Pin {
            key: "anchored".into(),
            x: 0.0,
            y: 0.0,
            width: None,
            height: None,
            content: Box::new(lowering.lower_element(self.child)),
        }
    }
}

impl IntoElement for Anchored {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// A deferred child primitive. The host controls when it is painted.
pub struct Deferred {
    child: AnyElement,
}

pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: child.into_any_element(),
    }
}

impl Element for Deferred {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Lazy {
            key: "deferred".into(),
            generation: 0,
            content: Box::new(lowering.lower_element(self.child)),
        }
    }
}

impl IntoElement for Deferred {
    type Element = Self;

    fn into_element(self) -> Self {
        self
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

impl Element for Canvas {
    fn lower(self: Box<Self>, _lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Canvas {
            key: "canvas".into(),
            width: None,
            height: None,
            commands: self.commands,
        }
    }
}

impl IntoElement for Canvas {
    type Element = Self;

    fn into_element(self) -> Self {
        self
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

impl<R: IntoElement + 'static> Element for UniformList<R> {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            id,
            count,
            processor,
        } = *self;
        let children = (processor)(0..count, lowering.window, lowering.app)
            .into_iter()
            .map(|child| lowering.lower(child))
            .collect();
        wire::Node::KeyedColumn {
            key: wire::ElementIdWire::from_gpui(id)
                .expect("element ID must be portable across the view boundary")
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

impl<R: IntoElement + 'static> IntoElement for UniformList<R> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViewElement;
    use std::borrow::Cow;

    #[derive(crate::IntoElement)]
    struct DerivedComponent;

    impl RenderOnce for DerivedComponent {
        fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
            "derived"
        }
    }

    #[derive(crate::IntoElement)]
    enum DerivedEnum {
        Unit,
    }

    impl RenderOnce for DerivedEnum {
        fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
            "enum"
        }
    }

    #[derive(crate::IntoElement)]
    struct GenericComponent<T>
    where
        T: Clone + 'static,
    {
        value: T,
    }

    impl<T: Clone + 'static> RenderOnce for GenericComponent<T> {
        fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
            let _ = self.value;
            "generic"
        }
    }

    fn assert_element<T: Element>() {}

    fn assert_view_element<T: RenderOnce>(_: ViewElement<T>) {}

    #[test]
    fn authoring_associated_types_follow_gpui() {
        fn string() -> <String as IntoElement>::Element {
            String::from("string").into_element()
        }
        fn text() -> <&'static str as IntoElement>::Element {
            "text".into_element()
        }
        fn shared() -> <SharedString as IntoElement>::Element {
            SharedString::from("shared").into_element()
        }
        fn borrowed() -> <Cow<'static, str> as IntoElement>::Element {
            Cow::Borrowed("borrowed").into_element()
        }

        assert_element::<SharedString>();
        assert_element::<&'static str>();
        assert_element::<Div>();
        assert_eq!(string().to_string(), "string");
        assert_eq!((*text()).to_owned(), "text");
        assert_eq!(shared().to_string(), "shared");
        assert_eq!(borrowed().to_string(), "borrowed");
    }

    #[test]
    fn derive_and_any_element_use_the_internal_lowering_boundary() {
        let _: AnyElement = DerivedComponent.into_any_element();
        let _: AnyElement = div().child(DerivedComponent).into_any_element();
    }

    #[test]
    fn derive_matches_gpui_component_element_shape() {
        assert_view_element(DerivedComponent.into_element());
        assert_view_element(DerivedEnum::Unit.into_element());
        assert_view_element(GenericComponent { value: 7_u8 }.into_element());
    }

    #[test]
    fn derived_components_and_wrappers_have_fluent_builders() {
        use gpui::prelude::FluentBuilder;

        let _: DerivedComponent = DerivedComponent.when(true, |component| component);
        let _: ViewElement<DerivedComponent> = DerivedComponent
            .into_element()
            .when(true, |element| element);
    }
}

#[cfg(test)]
mod ancestry_tests;
