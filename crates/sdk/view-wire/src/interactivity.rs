//! Lossless payloads for GPUI element listeners.
//!
//! These values describe the platform event data that a guest callback can
//! observe. They intentionally do not contain native hitboxes, focus handles,
//! windows, or action objects: those stay on the host side of the wire.

use crate::{click, keyboard, mouse};
use gpui::{Bounds, Pixels, Point};
use serde::{Deserialize, Serialize};

pub const MAX_KEY_CONTEXT_ENTRIES: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyContext {
    pub entries: Vec<KeyContextEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyContextEntry {
    pub key: gpui::SharedString,
    pub value: Option<gpui::SharedString>,
}

impl KeyContext {
    pub fn from_gpui(context: &gpui::KeyContext) -> Self {
        Self {
            entries: context
                .primary()
                .into_iter()
                .chain(context.secondary())
                .map(|entry| KeyContextEntry {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                })
                .collect(),
        }
    }

    pub fn to_gpui(&self) -> gpui::KeyContext {
        let mut context = gpui::KeyContext::default();
        for entry in &self.entries {
            if let Some(value) = &entry.value {
                context.set(entry.key.clone(), value.clone());
            } else {
                context.add(entry.key.clone());
            }
        }
        context
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchPhase {
    Capture,
    Bubble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseDown {
    pub button: click::MouseButton,
    pub position: Point<Pixels>,
    pub modifiers: keyboard::Modifiers,
    pub click_count: u32,
    pub first_mouse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseUp {
    pub button: click::MouseButton,
    pub position: Point<Pixels>,
    pub modifiers: keyboard::Modifiers,
    pub click_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseMove {
    pub position: Point<Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: keyboard::Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseExit {
    pub position: Point<Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: keyboard::Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MousePressure {
    pub pressure: f32,
    pub stage: PressureStage,
    pub position: Point<Pixels>,
    pub modifiers: keyboard::Modifiers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PressureStage {
    #[default]
    Zero,
    Normal,
    Force,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollWheel {
    pub position: Point<Pixels>,
    pub delta: mouse::ScrollDelta,
    pub modifiers: keyboard::Modifiers,
    pub touch_phase: TouchPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pinch {
    pub position: Point<Pixels>,
    pub delta: f32,
    pub modifiers: keyboard::Modifiers,
    pub phase: TouchPhase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDown {
    pub state: keyboard::KeyState,
    pub repeat: bool,
    pub prefer_character_input: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyUp {
    pub state: keyboard::KeyState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifiersChanged {
    pub modifiers: keyboard::Modifiers,
    pub capslock: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoverListenerMode {
    #[default]
    InputModalityAware,
    InputModalityIndependent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tooltip {
    /// Current-frame route used to construct the tooltip after native hover.
    pub request: u32,
    /// Host-filled response cache for the currently displayed frame.
    pub content: Option<Box<crate::Node>>,
    pub hoverable: bool,
    pub delay_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TooltipResponse {
    pub request: u32,
    /// Echoes the rich-text character index; ordinary tooltips use `None`.
    pub character_index: Option<u32>,
    /// `None` is an explicit result from an index-sensitive builder.
    pub content: Option<Box<crate::Node>>,
}

/// One bounded response cache for a native `InteractiveText` tooltip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextTooltip {
    pub request: u32,
    pub character_index: Option<u32>,
    pub content: Option<Box<crate::Node>>,
}

pub fn point(x: f32, y: f32) -> Point<Pixels> {
    gpui::point(gpui::px(x), gpui::px(y))
}

pub fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds {
        origin: point(x, y),
        size: gpui::size(gpui::px(width), gpui::px(height)),
    }
}

fn button(value: click::MouseButton) -> gpui::MouseButton {
    match value {
        click::MouseButton::Left => gpui::MouseButton::Left,
        click::MouseButton::Right => gpui::MouseButton::Right,
        click::MouseButton::Middle => gpui::MouseButton::Middle,
        click::MouseButton::Back => gpui::MouseButton::Navigate(gpui::NavigationDirection::Back),
        click::MouseButton::Forward => {
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward)
        }
    }
}

fn modifiers(value: keyboard::Modifiers) -> gpui::Modifiers {
    gpui::Modifiers {
        shift: value.shift,
        control: value.control,
        alt: value.alt,
        platform: value.logo,
        function: value.function,
    }
}

fn key_name(value: &keyboard::Key) -> String {
    match value {
        keyboard::Key::Character(value) => value.clone(),
        keyboard::Key::Unidentified => "unidentified".into(),
        keyboard::Key::Named(value) => match value {
            keyboard::Named::Enter => "enter",
            keyboard::Named::Tab => "tab",
            keyboard::Named::Space => "space",
            keyboard::Named::Escape => "escape",
            keyboard::Named::Backspace => "backspace",
            keyboard::Named::Delete => "delete",
            keyboard::Named::Insert => "insert",
            keyboard::Named::ArrowUp => "up",
            keyboard::Named::ArrowDown => "down",
            keyboard::Named::ArrowLeft => "left",
            keyboard::Named::ArrowRight => "right",
            keyboard::Named::PageUp => "pageup",
            keyboard::Named::PageDown => "pagedown",
            keyboard::Named::Control => "control",
            keyboard::Named::Alt => "alt",
            keyboard::Named::Shift => "shift",
            keyboard::Named::Super => "platform",
            keyboard::Named::Fn => "function",
            value => return format!("{value:?}").to_ascii_lowercase(),
        }
        .into(),
    }
}

fn keystroke(value: &keyboard::KeyState) -> gpui::Keystroke {
    gpui::Keystroke {
        modifiers: modifiers(value.modifiers),
        key: key_name(&value.key),
        key_char: match &value.modified_key {
            keyboard::Key::Character(value) => Some(value.clone()),
            _ => None,
        },
    }
}

fn touch_phase(value: TouchPhase) -> gpui::TouchPhase {
    match value {
        TouchPhase::Started => gpui::TouchPhase::Started,
        TouchPhase::Moved => gpui::TouchPhase::Moved,
        TouchPhase::Ended => gpui::TouchPhase::Ended,
        TouchPhase::Cancelled => gpui::TouchPhase::Cancelled,
    }
}

fn wire_key(value: &str) -> keyboard::Key {
    use keyboard::Named;
    match value {
        "enter" => keyboard::Key::Named(Named::Enter),
        "tab" => keyboard::Key::Named(Named::Tab),
        "space" => keyboard::Key::Named(Named::Space),
        "escape" => keyboard::Key::Named(Named::Escape),
        "backspace" => keyboard::Key::Named(Named::Backspace),
        "delete" => keyboard::Key::Named(Named::Delete),
        "insert" => keyboard::Key::Named(Named::Insert),
        "up" => keyboard::Key::Named(Named::ArrowUp),
        "down" => keyboard::Key::Named(Named::ArrowDown),
        "left" => keyboard::Key::Named(Named::ArrowLeft),
        "right" => keyboard::Key::Named(Named::ArrowRight),
        "pageup" => keyboard::Key::Named(Named::PageUp),
        "pagedown" => keyboard::Key::Named(Named::PageDown),
        _ => keyboard::Key::Character(value.to_owned()),
    }
}

fn wire_modifiers(value: gpui::Modifiers) -> keyboard::Modifiers {
    keyboard::Modifiers {
        shift: value.shift,
        control: value.control,
        alt: value.alt,
        logo: value.platform,
        function: value.function,
    }
}

fn wire_key_state(value: &gpui::Keystroke) -> keyboard::KeyState {
    let key = wire_key(&value.key);
    let modified_key = value
        .key_char
        .as_deref()
        .map(|key| keyboard::Key::Character(key.to_owned()))
        .unwrap_or_else(|| key.clone());
    keyboard::KeyState {
        key,
        modified_key,
        physical_key: keyboard::Physical::Unidentified(keyboard::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: wire_modifiers(value.modifiers),
    }
}

impl From<&gpui::MouseDownEvent> for MouseDown {
    fn from(value: &gpui::MouseDownEvent) -> Self {
        Self {
            button: value.button.into(),
            position: value.position,
            modifiers: wire_modifiers(value.modifiers),
            click_count: value.click_count.min(u32::MAX as usize) as u32,
            first_mouse: value.first_mouse,
        }
    }
}

impl From<&gpui::MouseUpEvent> for MouseUp {
    fn from(value: &gpui::MouseUpEvent) -> Self {
        Self {
            button: value.button.into(),
            position: value.position,
            modifiers: wire_modifiers(value.modifiers),
            click_count: value.click_count.min(u32::MAX as usize) as u32,
        }
    }
}

impl From<&gpui::MouseMoveEvent> for MouseMove {
    fn from(value: &gpui::MouseMoveEvent) -> Self {
        Self {
            position: value.position,
            pressed_button: value.pressed_button.map(Into::into),
            modifiers: wire_modifiers(value.modifiers),
        }
    }
}

impl From<&gpui::MouseExitEvent> for MouseExit {
    fn from(value: &gpui::MouseExitEvent) -> Self {
        Self {
            position: value.position,
            pressed_button: value.pressed_button.map(Into::into),
            modifiers: wire_modifiers(value.modifiers),
        }
    }
}

impl From<&gpui::MousePressureEvent> for MousePressure {
    fn from(value: &gpui::MousePressureEvent) -> Self {
        Self {
            pressure: value.pressure,
            stage: match value.stage {
                gpui::PressureStage::Zero => PressureStage::Zero,
                gpui::PressureStage::Normal => PressureStage::Normal,
                gpui::PressureStage::Force => PressureStage::Force,
            },
            position: value.position,
            modifiers: wire_modifiers(value.modifiers),
        }
    }
}

impl From<&gpui::ScrollWheelEvent> for ScrollWheel {
    fn from(value: &gpui::ScrollWheelEvent) -> Self {
        let delta = match value.delta {
            gpui::ScrollDelta::Pixels(point) => mouse::ScrollDelta::Pixels {
                x: point.x.as_f32(),
                y: point.y.as_f32(),
            },
            gpui::ScrollDelta::Lines(point) => mouse::ScrollDelta::Lines {
                x: point.x,
                y: point.y,
            },
        };
        Self {
            position: value.position,
            delta,
            modifiers: wire_modifiers(value.modifiers),
            touch_phase: match value.touch_phase {
                gpui::TouchPhase::Started => TouchPhase::Started,
                gpui::TouchPhase::Moved => TouchPhase::Moved,
                gpui::TouchPhase::Ended => TouchPhase::Ended,
                gpui::TouchPhase::Cancelled => TouchPhase::Cancelled,
            },
        }
    }
}

impl From<&gpui::PinchEvent> for Pinch {
    fn from(value: &gpui::PinchEvent) -> Self {
        Self {
            position: value.position,
            delta: value.delta,
            modifiers: wire_modifiers(value.modifiers),
            phase: match value.phase {
                gpui::TouchPhase::Started => TouchPhase::Started,
                gpui::TouchPhase::Moved => TouchPhase::Moved,
                gpui::TouchPhase::Ended => TouchPhase::Ended,
                gpui::TouchPhase::Cancelled => TouchPhase::Cancelled,
            },
        }
    }
}

impl From<&gpui::KeyDownEvent> for KeyDown {
    fn from(value: &gpui::KeyDownEvent) -> Self {
        Self {
            state: wire_key_state(&value.keystroke),
            repeat: value.is_held,
            prefer_character_input: value.prefer_character_input,
        }
    }
}

impl From<&gpui::KeyUpEvent> for KeyUp {
    fn from(value: &gpui::KeyUpEvent) -> Self {
        Self {
            state: wire_key_state(&value.keystroke),
        }
    }
}

impl From<&gpui::ModifiersChangedEvent> for ModifiersChanged {
    fn from(value: &gpui::ModifiersChangedEvent) -> Self {
        Self {
            modifiers: wire_modifiers(value.modifiers),
            capslock: value.capslock.on,
        }
    }
}

impl MouseDown {
    pub fn into_gpui(self) -> gpui::MouseDownEvent {
        gpui::MouseDownEvent {
            button: button(self.button),
            position: self.position,
            modifiers: modifiers(self.modifiers),
            click_count: self.click_count as usize,
            first_mouse: self.first_mouse,
        }
    }
}

impl MouseUp {
    pub fn into_gpui(self) -> gpui::MouseUpEvent {
        gpui::MouseUpEvent {
            button: button(self.button),
            position: self.position,
            modifiers: modifiers(self.modifiers),
            click_count: self.click_count as usize,
        }
    }
}

impl MouseMove {
    pub fn into_gpui(self) -> gpui::MouseMoveEvent {
        gpui::MouseMoveEvent {
            position: self.position,
            pressed_button: self.pressed_button.map(button),
            modifiers: modifiers(self.modifiers),
        }
    }
}

impl MouseExit {
    pub fn into_gpui(self) -> gpui::MouseExitEvent {
        gpui::MouseExitEvent {
            position: self.position,
            pressed_button: self.pressed_button.map(button),
            modifiers: modifiers(self.modifiers),
        }
    }
}

impl MousePressure {
    pub fn into_gpui(self) -> gpui::MousePressureEvent {
        gpui::MousePressureEvent {
            pressure: self.pressure,
            stage: match self.stage {
                PressureStage::Zero => gpui::PressureStage::Zero,
                PressureStage::Normal => gpui::PressureStage::Normal,
                PressureStage::Force => gpui::PressureStage::Force,
            },
            position: self.position,
            modifiers: modifiers(self.modifiers),
        }
    }
}

impl ScrollWheel {
    pub fn into_gpui(self) -> gpui::ScrollWheelEvent {
        gpui::ScrollWheelEvent {
            position: self.position,
            delta: match self.delta {
                mouse::ScrollDelta::Pixels { x, y } => gpui::ScrollDelta::Pixels(point(x, y)),
                mouse::ScrollDelta::Lines { x, y } => gpui::ScrollDelta::Lines(gpui::point(x, y)),
            },
            modifiers: modifiers(self.modifiers),
            touch_phase: touch_phase(self.touch_phase),
        }
    }
}

impl Pinch {
    pub fn into_gpui(self) -> gpui::PinchEvent {
        gpui::PinchEvent {
            position: self.position,
            delta: self.delta,
            modifiers: modifiers(self.modifiers),
            phase: touch_phase(self.phase),
        }
    }
}

impl KeyDown {
    pub fn into_gpui(self) -> gpui::KeyDownEvent {
        gpui::KeyDownEvent {
            keystroke: keystroke(&self.state),
            is_held: self.repeat,
            prefer_character_input: self.prefer_character_input,
        }
    }
}

impl KeyUp {
    pub fn into_gpui(self) -> gpui::KeyUpEvent {
        gpui::KeyUpEvent {
            keystroke: keystroke(&self.state),
        }
    }
}

impl ModifiersChanged {
    pub fn into_gpui(self) -> gpui::ModifiersChangedEvent {
        gpui::ModifiersChangedEvent {
            modifiers: modifiers(self.modifiers),
            capslock: gpui::Capslock { on: self.capslock },
        }
    }
}
