//! GPUI-shaped interaction recipes lowered into driver-owned frame routes.

use crate::{wire, AnyElement, AnyView, App, Div, Element, IntoElement, Lowering, ParentElement, Window};
use gpui::{
    ClickEvent, ElementId, FileDropEvent, MouseButton, SharedString, StyleRefinement, Styled,
    WindowControlArea,
};
use std::time::Duration;

pub(crate) type ClickListener = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub(crate) type MouseDownListener =
    Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static>;
pub(crate) type MouseUpListener = Box<dyn Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static>;
pub(crate) type MousePressureListener =
    Box<dyn Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static>;
pub(crate) type MouseMoveListener =
    Box<dyn Fn(&gpui::MouseMoveEvent, &mut Window, &mut App) + 'static>;
pub(crate) type MouseExitListener =
    Box<dyn Fn(&gpui::MouseExitEvent, &mut Window, &mut App) + 'static>;
pub(crate) type ScrollWheelListener =
    Box<dyn Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static>;
pub(crate) type PinchListener = Box<dyn Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static>;
pub(crate) type KeyDownListener = Box<dyn Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static>;
pub(crate) type KeyUpListener = Box<dyn Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static>;
pub(crate) type ModifiersChangedListener =
    Box<dyn Fn(&gpui::ModifiersChangedEvent, &mut Window, &mut App) + 'static>;

struct MouseDownBinding {
    button: Option<MouseButton>,
    listener: MouseDownListener,
}
struct MouseUpBinding {
    button: Option<MouseButton>,
    listener: MouseUpListener,
}

/// A guest-owned focus identity. Native GPUI focus handles cannot cross the
/// wasm boundary, so the host creates the corresponding native handle while
/// retaining this typed GPUI element identity.
#[derive(Clone, Debug)]
pub struct FocusHandle {
    id: u64,
}

impl FocusHandle {
    pub(crate) fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn focus(&self, window: &mut Window, _cx: &mut App) {
        window.dispatch(wire::WidgetCommand::FocusHandle { handle: self.id });
    }
}

struct TooltipBuilder {
    build: Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>,
    hoverable: bool,
}

/// The explicit state carried by guest interactivity until frame lowering.
#[derive(Default)]
pub struct Interactivity {
    pub(crate) role: Option<gpui::Role>,
    pub(crate) aria: wire::Aria,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: Option<bool>,
    pub(crate) tab_index: Option<i32>,
    pub(crate) tab_group: bool,
    pub(crate) focus: Option<StyleRefinement>,
    pub(crate) in_focus: Option<StyleRefinement>,
    pub(crate) focus_visible: Option<StyleRefinement>,
    pub(crate) key_context: Option<SharedString>,
    pub(crate) focus_handle: Option<FocusHandle>,
    pub(crate) group: Option<SharedString>,
    pub(crate) hover: Option<StyleRefinement>,
    pub(crate) active: Option<StyleRefinement>,
    pub(crate) group_hover: Option<(SharedString, StyleRefinement)>,
    pub(crate) group_active: Option<(SharedString, StyleRefinement)>,
    pub(crate) on_click: Option<ClickListener>,
    pub(crate) on_aux_click: Option<ClickListener>,
    mouse_down: Vec<MouseDownBinding>,
    capture_mouse_down: Vec<MouseDownListener>,
    mouse_down_out: Vec<MouseDownListener>,
    mouse_up: Vec<MouseUpBinding>,
    capture_mouse_up: Vec<MouseUpListener>,
    mouse_up_out: Vec<MouseUpBinding>,
    mouse_pressure: Vec<MousePressureListener>,
    capture_mouse_pressure: Vec<MousePressureListener>,
    mouse_move: Vec<MouseMoveListener>,
    mouse_exit: Vec<MouseExitListener>,
    scroll_wheel: Vec<ScrollWheelListener>,
    pinch: Vec<PinchListener>,
    capture_pinch: Vec<PinchListener>,
    key_down: Vec<KeyDownListener>,
    capture_key_down: Vec<KeyDownListener>,
    key_up: Vec<KeyUpListener>,
    capture_key_up: Vec<KeyUpListener>,
    modifiers_changed: Vec<ModifiersChangedListener>,
    on_hover: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    hover_listener_mode: gpui::HoverListenerMode,
    on_file_drop_exit: Vec<Box<dyn Fn(&FileDropEvent, &mut Window, &mut App) + 'static>>,
    tooltip: Option<TooltipBuilder>,
    tooltip_show_delay: Option<Duration>,
    occlude: bool,
    block_mouse_except_scroll: bool,
    window_control_area: Option<WindowControlArea>,
    pub(crate) base_style: StyleRefinement,
    pub(crate) id: Option<ElementId>,
}

/// Add GPUI's stateless and stateful interactivity declarations to an element.
pub trait InteractiveElement: Sized {
    fn interactivity(&mut self) -> &mut Interactivity;

    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactivity().id = Some(id.into());
        Stateful { element: self }
    }

    fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().focus_handle = Some(focus_handle.clone());
        self
    }

    fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.interactivity().tab_stop = Some(tab_stop);
        self
    }

    fn tab_index(mut self, index: isize) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().tab_index = i32::try_from(index).ok();
        self.interactivity().tab_stop = Some(true);
        self
    }

    fn tab_group(mut self) -> Self {
        self.interactivity().tab_group = true;
        if self.interactivity().tab_index.is_none() {
            self.interactivity().tab_index = Some(0);
        }
        self
    }

    fn key_context<C, E>(mut self, key_context: C) -> Self
    where
        C: TryInto<gpui::KeyContext, Error = E>,
        E: std::fmt::Display,
    {
        if let Ok(key_context) = key_context.try_into() {
            self.interactivity().key_context = Some(format!("{key_context:?}").into());
        }
        self
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

    fn on_mouse_down(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_down.push(MouseDownBinding {
            button: Some(button),
            listener: Box::new(listener),
        });
        self
    }

    fn capture_any_mouse_down(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .capture_mouse_down
            .push(Box::new(listener));
        self
    }

    fn on_any_mouse_down(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_down.push(MouseDownBinding {
            button: None,
            listener: Box::new(listener),
        });
        self
    }

    fn on_mouse_down_out(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_down_out.push(Box::new(listener));
        self
    }

    fn on_mouse_up(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_up.push(MouseUpBinding {
            button: Some(button),
            listener: Box::new(listener),
        });
        self
    }

    fn capture_any_mouse_up(
        mut self,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .capture_mouse_up
            .push(Box::new(listener));
        self
    }

    fn on_any_mouse_up(
        mut self,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_up.push(MouseUpBinding {
            button: None,
            listener: Box::new(listener),
        });
        self
    }

    fn on_mouse_up_out(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_up_out.push(MouseUpBinding {
            button: Some(button),
            listener: Box::new(listener),
        });
        self
    }

    fn on_mouse_pressure(
        mut self,
        listener: impl Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_pressure.push(Box::new(listener));
        self
    }

    fn capture_mouse_pressure(
        mut self,
        listener: impl Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .capture_mouse_pressure
            .push(Box::new(listener));
        self
    }

    fn on_mouse_move(
        mut self,
        listener: impl Fn(&gpui::MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_move.push(Box::new(listener));
        self
    }

    fn on_mouse_exit(
        mut self,
        listener: impl Fn(&gpui::MouseExitEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().mouse_exit.push(Box::new(listener));
        self
    }

    fn on_scroll_wheel(
        mut self,
        listener: impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().scroll_wheel.push(Box::new(listener));
        self
    }

    fn on_pinch(
        mut self,
        listener: impl Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().pinch.push(Box::new(listener));
        self
    }

    fn capture_pinch(
        mut self,
        listener: impl Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_pinch.push(Box::new(listener));
        self
    }

    fn on_key_down(
        mut self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().key_down.push(Box::new(listener));
        self
    }

    fn capture_key_down(
        mut self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .capture_key_down
            .push(Box::new(listener));
        self
    }

    fn on_key_up(
        mut self,
        listener: impl Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().key_up.push(Box::new(listener));
        self
    }

    fn capture_key_up(
        mut self,
        listener: impl Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_key_up.push(Box::new(listener));
        self
    }

    fn on_modifiers_changed(
        mut self,
        listener: impl Fn(&gpui::ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .modifiers_changed
            .push(Box::new(listener));
        self
    }

    fn on_file_drop_exit(
        mut self,
        listener: impl Fn(&FileDropEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .on_file_drop_exit
            .push(Box::new(listener));
        self
    }

    fn occlude(mut self) -> Self {
        self.interactivity().occlude = true;
        self
    }

    fn window_control_area(mut self, area: WindowControlArea) -> Self {
        self.interactivity().window_control_area = Some(area);
        self
    }

    fn block_mouse_except_scroll(mut self) -> Self {
        self.interactivity().block_mouse_except_scroll = true;
        self
    }

    fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().focus = Some(f(StyleRefinement::default()));
        self
    }

    fn in_focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().in_focus = Some(f(StyleRefinement::default()));
        self
    }

    fn focus_visible(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().focus_visible = Some(f(StyleRefinement::default()));
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

impl<E: Element> Element for Stateful<E> {
    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(Box::new(self.element), lowering)
    }
}

impl<E: Element> IntoElement for Stateful<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
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

/// Stateful interaction methods with GPUI's public names and signatures where
/// the value can be represented by the guest wire contract.
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
    fn aria_active_descendant(mut self) -> Self {
        self.interactivity().aria.active_descendant = true;
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
    fn restrict_scroll_to_axis(mut self) -> Self {
        self.interactivity().base_style.restrict_scroll_to_axis = Some(true);
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
    fn on_aux_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_aux_click = Some(Box::new(listener));
        self
    }
    fn on_hover(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity().on_hover = Some(Box::new(listener));
        self
    }
    fn hover_listener_mode(mut self, mode: gpui::HoverListenerMode) -> Self {
        self.interactivity().hover_listener_mode = mode;
        self
    }
    fn tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.interactivity().tooltip = Some(TooltipBuilder {
            build: Box::new(build_tooltip),
            hoverable: false,
        });
        self
    }
    fn hoverable_tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.interactivity().tooltip = Some(TooltipBuilder {
            build: Box::new(build_tooltip),
            hoverable: true,
        });
        self
    }
    fn tooltip_show_delay(mut self, delay: Duration) -> Self {
        self.interactivity().tooltip_show_delay = Some(delay);
        self
    }
}

impl<T: InteractiveElement> StatefulInteractiveElement for Stateful<T> {}

impl Interactivity {
    pub(crate) fn into_wire(self, lowering: &mut Lowering<'_>) -> (Option<wire::ElementIdWire>, wire::Interactivity) {
        let id = self.id.map(wire::ElementIdWire::from_gpui).transpose().expect("element ID must be portable across the view boundary");
        let tooltip = self.tooltip.map(|tooltip| {
            let request = lowering.tooltip(tooltip.build);
            wire::Tooltip {
                request,
                content: None,
                hoverable: tooltip.hoverable,
                delay_ms: self
                    .tooltip_show_delay
                    .unwrap_or(Duration::from_millis(500))
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            }
        });
        let wire = wire::Interactivity {
            role: self.role,
            aria: self.aria,
            focusable: self.focusable,
            tab_stop: self.tab_stop,
            tab_index: self.tab_index,
            tab_group: self.tab_group,
            focus: self.focus,
            in_focus: self.in_focus,
            focus_visible: self.focus_visible,
            key_context: self.key_context,
            focus_handle: self.focus_handle.map(|handle| handle.id),
            occlude: self.occlude,
            block_mouse_except_scroll: self.block_mouse_except_scroll,
            window_control_area: self.window_control_area.map(|area| match area {
                WindowControlArea::Drag => wire::WindowControlArea::Drag,
                WindowControlArea::Close => wire::WindowControlArea::Close,
                WindowControlArea::Max => wire::WindowControlArea::Max,
                WindowControlArea::Min => wire::WindowControlArea::Min,
            }),
            hover_listener_mode: match self.hover_listener_mode {
                gpui::HoverListenerMode::InputModalityAware => {
                    wire::HoverListenerMode::InputModalityAware
                }
                gpui::HoverListenerMode::InputModalityIndependent => {
                    wire::HoverListenerMode::InputModalityIndependent
                }
            },
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
            on_aux_click: self.on_aux_click.map(|listener| lowering.click(listener)),
            on_mouse_down: route_mouse_down(self.mouse_down, lowering),
            capture_mouse_down: route_plain(self.capture_mouse_down, lowering),
            on_mouse_down_out: route_plain(self.mouse_down_out, lowering),
            on_mouse_up: route_mouse_up(self.mouse_up, lowering),
            capture_mouse_up: route_plain(self.capture_mouse_up, lowering),
            on_mouse_up_out: route_mouse_up(self.mouse_up_out, lowering),
            on_mouse_pressure: route_plain(self.mouse_pressure, lowering),
            capture_mouse_pressure: route_plain(self.capture_mouse_pressure, lowering),
            on_mouse_move: route_plain(self.mouse_move, lowering),
            on_mouse_exit: route_plain(self.mouse_exit, lowering),
            on_scroll_wheel: route_plain(self.scroll_wheel, lowering),
            on_pinch: route_plain(self.pinch, lowering),
            capture_pinch: route_plain(self.capture_pinch, lowering),
            on_key_down: route_plain(self.key_down, lowering),
            capture_key_down: route_plain(self.capture_key_down, lowering),
            on_key_up: route_plain(self.key_up, lowering),
            capture_key_up: route_plain(self.capture_key_up, lowering),
            on_modifiers_changed: route_plain(self.modifiers_changed, lowering),
            on_hover: self.on_hover.map(|listener| lowering.route(listener)),
            on_file_drop_exit: route_plain(self.on_file_drop_exit, lowering),
            tooltip,
        };
        (id, wire)
    }
}

fn route_plain<E: 'static>(
    listeners: Vec<Box<dyn Fn(&E, &mut Window, &mut App) + 'static>>,
    lowering: &mut Lowering<'_>,
) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &E, window, app| {
            for listener in &listeners {
                listener(event, window, app);
            }
        })
    })
}

fn route_mouse_down(listeners: Vec<MouseDownBinding>, lowering: &mut Lowering<'_>) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &gpui::MouseDownEvent, window, app| {
            for binding in &listeners {
                if binding.button.is_none_or(|button| button == event.button) {
                    (binding.listener)(event, window, app);
                }
            }
        })
    })
}

fn route_mouse_up(listeners: Vec<MouseUpBinding>, lowering: &mut Lowering<'_>) -> Option<u32> {
    (!listeners.is_empty()).then(|| {
        lowering.route(move |event: &gpui::MouseUpEvent, window, app| {
            for binding in &listeners {
                if binding.button.is_none_or(|button| button == event.button) {
                    (binding.listener)(event, window, app);
                }
            }
        })
    })
}

impl<E> gpui::prelude::FluentBuilder for Stateful<E> {}
