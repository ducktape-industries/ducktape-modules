//! Guest-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm guest. Lowering turns this small recipe into wire
//! data once per frame.

use crate::interactivity::{ClickListener, Interactivity};
use crate::{App, Window, slots, wire};
use gpui::{
    ElementId, ListHorizontalSizingBehavior, ListSizingBehavior, Overflow, ScrollStrategy,
    SharedString, StyleRefinement, Styled,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

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

    pub(crate) fn parts(&mut self) -> (&mut Window, &mut App) {
        (self.window, self.app)
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

    pub(crate) fn click(&mut self, listener: ClickListener) -> u32 {
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

    pub(crate) fn picture(&mut self, bytes: impl AsRef<[u8]>) -> (u64, Option<Vec<u8>>) {
        slots::picture(&self.app.inner.slots, bytes)
    }

    pub(crate) fn tooltip(
        &mut self,
        build: Box<dyn Fn(&mut Window, &mut App) -> crate::AnyView + 'static>,
    ) -> u32 {
        slots::tooltip(&self.app.inner.slots, build)
    }

    pub(crate) fn rich_text_tooltip(
        &mut self,
        build: Box<dyn Fn(usize, &mut Window, &mut App) -> Option<crate::AnyView> + 'static>,
    ) -> u32 {
        slots::rich_text_tooltip(&self.app.inner.slots, build)
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
        let id = interactivity.id.as_ref().map(|_| {
            lowering
                .current_path()
                .last()
                .cloned()
                .expect("identified div must lower inside its authored scope")
        });
        let style = interactivity.base_style.clone();
        let (_, wire_interactivity) = interactivity.into_wire(lowering);
        let children = children
            .into_iter()
            .map(|child| lowering.lower_element(child))
            .collect();
        wire::Node::Container {
            id,
            style,
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

/// A single-line host text input. GPUI core has no text-input element, so this
/// recipe carries a typed identity and lowers to the host's native field.
pub struct Input {
    id: ElementId,
    value: String,
    placeholder: String,
    options: wire::InputOptions,
    secure: bool,
    style: StyleRefinement,
    on_input: Option<Box<dyn Fn(&String, &mut Window, &mut App)>>,
    on_submit: Option<Box<dyn Fn(&(), &mut Window, &mut App)>>,
}

impl Input {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            value: String::new(),
            placeholder: String::new(),
            options: wire::InputOptions::default(),
            secure: false,
            style: StyleRefinement::default(),
            on_input: None,
            on_submit: None,
        }
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.options.label = label.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.options.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.options.disabled = disabled;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    pub fn on_input(mut self, listener: impl Fn(&String, &mut Window, &mut App) + 'static) -> Self {
        self.on_input = Some(Box::new(listener));
        self
    }

    pub fn on_submit(mut self, listener: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Box::new(listener));
        self
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Element for Input {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let this = *self;
        let id = wire::ElementIdWire::from_gpui(this.id)
            .expect("input element ID must be portable across the view boundary");
        let on_input = this.on_input.map(|listener| lowering.route(listener));
        let on_submit = this
            .on_submit
            .map(|listener| lowering.message_route(listener));
        wire::Node::Input {
            options: this.options,
            id,
            placeholder: this.placeholder,
            value: this.value,
            on_input: on_input.unwrap_or(u32::MAX),
            on_submit,
            secure: this.secure,
            style: this.style,
        }
    }
}

impl IntoElement for Input {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
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

mod uniform_list;
pub(crate) use uniform_list::UniformListScrollState;
pub use uniform_list::{uniform_list, UniformList, UniformListScrollHandle};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod ancestry_tests;
