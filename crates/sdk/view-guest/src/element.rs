//! Guest-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm guest. Lowering turns this small recipe into wire
//! data once per frame.

use crate::interactivity::{ClickListener, Interactivity};
use crate::{slots, wire, App, Window};
use gpui::{ElementId, ListHorizontalSizingBehavior, ListSizingBehavior, Overflow, ScrollStrategy, SharedString, StyleRefinement, Styled};
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

/// A handle for controlling a guest uniform list across frames.
#[derive(Clone, Default)]
pub struct UniformListScrollHandle(Rc<RefCell<UniformListScrollState>>);

#[derive(Default)]
pub(crate) struct UniformListScrollState {
    pub(crate) request: Option<wire::list::UniformListScrollRequest>,
    pub(crate) y_flipped: bool,
    pub(crate) top_index: usize,
    pub(crate) scrollable: bool,
    pub(crate) scrolled_to_end: Option<bool>,
}

impl UniformListScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn request(&self, index: usize, strategy: ScrollStrategy, offset: usize, strict: bool) {
        let strategy = match strategy {
            ScrollStrategy::Top => wire::list::UniformListScrollStrategy::Top,
            ScrollStrategy::Center => wire::list::UniformListScrollStrategy::Center,
            ScrollStrategy::Bottom => wire::list::UniformListScrollStrategy::Bottom,
            ScrollStrategy::Nearest => wire::list::UniformListScrollStrategy::Nearest,
        };
        self.0.borrow_mut().request = Some(wire::list::UniformListScrollRequest {
            index,
            strategy,
            offset,
            strict,
        });
    }

    pub fn scroll_to_item(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, false);
    }

    pub fn scroll_to_item_strict(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, true);
    }

    pub fn scroll_to_item_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, false);
    }

    pub fn scroll_to_item_strict_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, true);
    }

    pub fn y_flipped(&self) -> bool {
        self.0.borrow().y_flipped
    }

    pub fn logical_scroll_top_index(&self) -> usize {
        self.0.borrow().top_index
    }

    pub fn is_scrollable(&self) -> bool {
        self.0.borrow().scrollable
    }

    pub fn is_scrolled_to_end(&self) -> Option<bool> {
        self.0.borrow().scrolled_to_end
    }

    pub fn scroll_to_bottom(&self) {
        self.scroll_to_item(usize::MAX, ScrollStrategy::Bottom);
    }
}

/// A GPUI-shaped uniform list recipe. The host owns layout and virtualization.
pub struct UniformList {
    count: usize,
    processor: Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>,
    pub(crate) interactivity: Interactivity,
    measure_index: usize,
    sizing: wire::list::UniformListSizing,
    horizontal_sizing: wire::list::UniformListHorizontalSizing,
    scroll: Option<UniformListScrollHandle>,
    y_flipped: bool,
}

pub fn uniform_list<R: IntoElement>(
    id: impl Into<ElementId>,
    count: usize,
    processor: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> UniformList {
    let mut style = StyleRefinement::default();
    style.overflow.y = Some(Overflow::Scroll);
    UniformList {
        count,
        processor: Box::new(move |range, window, app| {
            processor(range, window, app)
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect()
        }),
        interactivity: Interactivity {
            id: Some(id.into()),
            base_style: style,
            ..Interactivity::default()
        },
        measure_index: 0,
        sizing: wire::list::UniformListSizing::Auto,
        horizontal_sizing: wire::list::UniformListHorizontalSizing::FitList,
        scroll: None,
        y_flipped: false,
    }
}

impl UniformList {
    pub fn with_width_from_item(mut self, item_index: Option<usize>) -> Self {
        self.measure_index = item_index.unwrap_or(0);
        self
    }

    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing = match behavior {
            ListSizingBehavior::Infer => wire::list::UniformListSizing::Infer,
            ListSizingBehavior::Auto => wire::list::UniformListSizing::Auto,
        };
        self
    }

    pub fn with_horizontal_sizing_behavior(
        mut self,
        behavior: ListHorizontalSizingBehavior,
    ) -> Self {
        self.horizontal_sizing = match behavior {
            ListHorizontalSizingBehavior::FitList => {
                self.interactivity.base_style.overflow.x = None;
                wire::list::UniformListHorizontalSizing::FitList
            }
            ListHorizontalSizingBehavior::Unconstrained => {
                self.interactivity.base_style.overflow.x = Some(Overflow::Scroll);
                wire::list::UniformListHorizontalSizing::Unconstrained
            }
        };
        self
    }

    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    pub fn y_flipped(mut self, y_flipped: bool) -> Self {
        self.y_flipped = y_flipped;
        self
    }
}

impl Styled for UniformList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl crate::InteractiveElement for UniformList {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl Element for UniformList {
    fn id(&self) -> Option<ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let count = self.count.min(wire::MAX_UNIFORM_LIST_COUNT);
        let path = lowering.current_path().to_vec();
        let id = path.last().cloned().expect("uniform list lowers inside its authored scope");
        let measure_index = self.measure_index.min(count.saturating_sub(1));
        let scroll = self.scroll.as_ref().map(|handle| &handle.0);
        let (route, ranges) = lowering
            .app
            .uniform_list_route(&path, count, measure_index, scroll);
        let mut indices = Vec::new();
        let mut children = Vec::new();
        for range in ranges {
            let range = range.start.min(count)..range.end.min(count);
            if range.is_empty() {
                continue;
            }
            let rendered = (self.processor)(range.clone(), lowering.window, lowering.app);
            for (index, child) in range.zip(rendered).take(wire::MAX_UNIFORM_LIST_ROWS) {
                indices.push(index as u32);
                children.push(lowering.lower_element(child));
            }
        }
        let scroll_request = self
            .scroll
            .as_ref()
            .and_then(|handle| handle.0.borrow_mut().request.take());
        if let Some(handle) = &self.scroll {
            handle.0.borrow_mut().y_flipped = self.y_flipped;
        }
        wire::Node::UniformList {
            id,
            path,
            route,
            style: self.interactivity.base_style,
            interactivity: wire::Interactivity {
                role: self.interactivity.role,
                aria: self.interactivity.aria,
                focusable: self.interactivity.focusable,
                group: self.interactivity.group,
                hover: self.interactivity.hover,
                active: self.interactivity.active,
                group_hover: self.interactivity.group_hover.map(|(group, style)| wire::GroupRefinement { group, style }),
                group_active: self.interactivity.group_active.map(|(group, style)| wire::GroupRefinement { group, style }),
                on_click: self.interactivity.on_click.map(|listener| lowering.click(listener)),
            },
            count,
            measure_index,
            sizing: self.sizing,
            horizontal_sizing: self.horizontal_sizing,
            y_flipped: self.y_flipped,
            scroll_request,
            indices,
            children,
        }
    }
}

impl IntoElement for UniformList {
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
impl gpui::prelude::FluentBuilder for Input {}
impl gpui::prelude::FluentBuilder for AnyElement {}
impl gpui::prelude::FluentBuilder for Img {}
impl gpui::prelude::FluentBuilder for Svg {}
impl gpui::prelude::FluentBuilder for Deferred {}
impl gpui::prelude::FluentBuilder for Anchored {}
impl gpui::prelude::FluentBuilder for Canvas {}
impl gpui::prelude::FluentBuilder for UniformList {}

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
