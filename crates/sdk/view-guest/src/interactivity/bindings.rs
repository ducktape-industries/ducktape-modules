use super::{App, ClickEvent, MouseButton, Window};

pub(crate) type ClickListener = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub(super) type EventListener<E> = Box<dyn Fn(&E, &mut Window, &mut App) + 'static>;
pub(super) type MouseDownListener =
    Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static>;
pub(super) type MouseUpListener = Box<dyn Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static>;
pub(super) type MousePressureListener =
    Box<dyn Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static>;
pub(super) type MouseMoveListener =
    Box<dyn Fn(&gpui::MouseMoveEvent, &mut Window, &mut App) + 'static>;
pub(super) type MouseExitListener =
    Box<dyn Fn(&gpui::MouseExitEvent, &mut Window, &mut App) + 'static>;
pub(super) type ScrollWheelListener =
    Box<dyn Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static>;
pub(super) type PinchListener = Box<dyn Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static>;
pub(super) type KeyDownListener = Box<dyn Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static>;
pub(super) type KeyUpListener = Box<dyn Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static>;
pub(super) type ModifiersChangedListener =
    Box<dyn Fn(&gpui::ModifiersChangedEvent, &mut Window, &mut App) + 'static>;

pub(super) struct MouseDownBinding {
    pub(super) button: Option<MouseButton>,
    pub(super) listener: MouseDownListener,
}
pub(super) struct MouseUpBinding {
    pub(super) button: Option<MouseButton>,
    pub(super) listener: MouseUpListener,
}
