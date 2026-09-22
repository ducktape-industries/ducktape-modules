//! GPUI-shaped interaction recipes, lowered into driver-owned frame routes.
use crate::{AnyElement, App, Div, Element, IntoElement, Lowering, ParentElement, Window, wire};
use gpui::{ClickEvent, ElementId, SharedString, StyleRefinement, Styled};

pub(crate) type ClickListener = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The explicit state carried by guest interactivity until frame lowering.
#[derive(Default)]
pub struct Interactivity {
    pub(crate) role: Option<gpui::Role>,
    pub(crate) aria: wire::Aria,
    pub(crate) focusable: bool,
    pub base_style: StyleRefinement,
    pub(crate) id: Option<ElementId>,
    pub(crate) group: Option<SharedString>,
    pub(crate) hover: Option<StyleRefinement>,
    pub(crate) active: Option<StyleRefinement>,
    pub(crate) group_hover: Option<(SharedString, StyleRefinement)>,
    pub(crate) group_active: Option<(SharedString, StyleRefinement)>,
    pub(crate) on_click: Option<ClickListener>,
}

impl Interactivity {
    pub(crate) fn into_wire(
        self,
        lowering: &mut Lowering<'_>,
    ) -> (Option<wire::ElementIdWire>, wire::Interactivity) {
        let id = self
            .id
            .map(wire::ElementIdWire::from_gpui)
            .transpose()
            .expect("element ID must be portable across the view boundary");
        let interactivity = wire::Interactivity {
            role: self.role,
            aria: self.aria,
            focusable: self.focusable,
            group: self.group,
            hover: self.hover,
            active: self.active,
            group_hover: self
                .group_hover
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            group_active: self
                .group_active
                .map(|(group, style)| wire::GroupRefinement { group, style }),
            on_click: self.on_click.map(|listener| lowering.click(listener)),
        };
        (id, interactivity)
    }
}

/// Add basic group and identity declarations to an element recipe.
pub trait InteractiveElement: Sized {
    fn interactivity(&mut self) -> &mut Interactivity;

    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactivity().id = Some(id.into());
        Stateful { element: self }
    }

    fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.interactivity().group = Some(group.into());
        self
    }

    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().hover = Some(f(StyleRefinement::default()));
        self
    }

    fn group_hover(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_hover = Some((group.into(), f(StyleRefinement::default())));
        self
    }
}

impl InteractiveElement for Div {
    fn interactivity(&mut self) -> &mut Interactivity {
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

impl<E: Element> IntoElement for Stateful<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Stateful<E> {
    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(Box::new(self.element), lowering)
    }
}

impl<E: ParentElement> ParentElement for Stateful<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements);
    }
}

impl<E: InteractiveElement> InteractiveElement for Stateful<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

/// Stateful interaction methods, named to match GPUI's public authoring API.
pub trait StatefulInteractiveElement: InteractiveElement {
    fn role(mut self, role: gpui::Role) -> Self {
        self.interactivity().role = Some(role);
        self
    }
    fn focusable(mut self) -> Self {
        self.interactivity().focusable = true;
        self
    }
    fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.interactivity().aria.author_id = Some(id.into());
        self
    }
    fn aria_label(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.label = Some(value.into());
        self
    }
    fn aria_description(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.description = Some(value.into());
        self
    }
    fn aria_keyshortcuts(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.keyshortcuts = Some(value.into());
        self
    }
    fn aria_value(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.value = Some(value.into());
        self
    }
    fn aria_placeholder(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().aria.placeholder = Some(value.into());
        self
    }
    fn aria_selected(mut self, value: bool) -> Self {
        self.interactivity().aria.selected = Some(value);
        self
    }
    fn aria_expanded(mut self, value: bool) -> Self {
        self.interactivity().aria.expanded = Some(value);
        self
    }
    fn aria_disabled(mut self, value: bool) -> Self {
        self.interactivity().aria.disabled = Some(value);
        self
    }
    fn aria_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.numeric_value = Some(value);
        self
    }
    fn aria_numeric_value_step(mut self, value: f64) -> Self {
        self.interactivity().aria.numeric_value_step = Some(value);
        self
    }
    fn aria_min_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.min_numeric_value = Some(value);
        self
    }
    fn aria_max_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().aria.max_numeric_value = Some(value);
        self
    }
    fn aria_level(mut self, value: usize) -> Self {
        self.interactivity().aria.level = Some(value);
        self
    }
    fn aria_position_in_set(mut self, value: usize) -> Self {
        self.interactivity().aria.position_in_set = Some(value);
        self
    }
    fn aria_size_of_set(mut self, value: usize) -> Self {
        self.interactivity().aria.size_of_set = Some(value);
        self
    }
    fn aria_row_index(mut self, value: usize) -> Self {
        self.interactivity().aria.row_index = Some(value);
        self
    }
    fn aria_column_index(mut self, value: usize) -> Self {
        self.interactivity().aria.column_index = Some(value);
        self
    }
    fn aria_row_count(mut self, value: usize) -> Self {
        self.interactivity().aria.row_count = Some(value);
        self
    }
    fn aria_column_count(mut self, value: usize) -> Self {
        self.interactivity().aria.column_count = Some(value);
        self
    }
    fn aria_toggled(mut self, value: gpui::Toggled) -> Self {
        self.interactivity().aria.toggled = Some(value);
        self
    }
    fn aria_orientation(mut self, value: gpui::Orientation) -> Self {
        self.interactivity().aria.orientation = Some(value);
        self
    }

    fn overflow_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(gpui::Overflow::Scroll);
        self.interactivity().base_style.overflow.y = Some(gpui::Overflow::Scroll);
        self
    }
    fn overflow_x_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(gpui::Overflow::Scroll);
        self
    }
    fn overflow_y_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.y = Some(gpui::Overflow::Scroll);
        self
    }
    fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().active = Some(f(StyleRefinement::default()));
        self
    }

    fn group_active(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_active = Some((group.into(), f(StyleRefinement::default())));
        self
    }

    fn on_click(mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity().on_click = Some(Box::new(listener));
        self
    }
}

impl<T: InteractiveElement> StatefulInteractiveElement for Stateful<T> {}

impl<E> gpui::prelude::FluentBuilder for Stateful<E> {}
