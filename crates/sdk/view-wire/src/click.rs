//! Lossless payloads for GPUI click callbacks. Routes are separate from ordinary
//! message IDs, and the callback receives the actual event, not a default one.
use gpui::{Bounds, Modifiers, Pixels, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Click {
    Mouse {
        down: ButtonEvent,
        up: ButtonEvent,
        first_mouse: bool,
    },
    Keyboard {
        button: KeyboardButton,
        bounds: Bounds<Pixels>,
    },
    Touch {
        position: Point<Pixels>,
        tap_count: usize,
        long_press: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ButtonEvent {
    pub button: MouseButton,
    pub position: Point<Pixels>,
    pub modifiers: Modifiers,
    pub click_count: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardButton {
    Enter,
    Space,
}

impl From<gpui::MouseButton> for MouseButton {
    fn from(value: gpui::MouseButton) -> Self {
        match value {
            gpui::MouseButton::Left => Self::Left,
            gpui::MouseButton::Right => Self::Right,
            gpui::MouseButton::Middle => Self::Middle,
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Back) => Self::Back,
            gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward) => Self::Forward,
        }
    }
}
impl From<MouseButton> for gpui::MouseButton {
    fn from(value: MouseButton) -> Self {
        match value {
            MouseButton::Left => Self::Left,
            MouseButton::Right => Self::Right,
            MouseButton::Middle => Self::Middle,
            MouseButton::Back => Self::Navigate(gpui::NavigationDirection::Back),
            MouseButton::Forward => Self::Navigate(gpui::NavigationDirection::Forward),
        }
    }
}
impl From<&gpui::ClickEvent> for Click {
    fn from(value: &gpui::ClickEvent) -> Self {
        match value {
            gpui::ClickEvent::Mouse(value) => Self::Mouse {
                down: ButtonEvent {
                    button: value.down.button.into(),
                    position: value.down.position,
                    modifiers: value.down.modifiers,
                    click_count: value.down.click_count,
                },
                up: ButtonEvent {
                    button: value.up.button.into(),
                    position: value.up.position,
                    modifiers: value.up.modifiers,
                    click_count: value.up.click_count,
                },
                first_mouse: value.down.first_mouse,
            },
            gpui::ClickEvent::Keyboard(value) => Self::Keyboard {
                button: match value.button {
                    gpui::KeyboardButton::Enter => KeyboardButton::Enter,
                    gpui::KeyboardButton::Space => KeyboardButton::Space,
                },
                bounds: value.bounds,
            },
            gpui::ClickEvent::Touch(value) => Self::Touch {
                position: value.position,
                tap_count: value.tap_count,
                long_press: value.long_press,
            },
        }
    }
}
impl From<Click> for gpui::ClickEvent {
    fn from(value: Click) -> Self {
        match value {
            Click::Mouse {
                down,
                up,
                first_mouse,
            } => Self::Mouse(gpui::MouseClickEvent {
                down: gpui::MouseDownEvent {
                    button: down.button.into(),
                    position: down.position,
                    modifiers: down.modifiers,
                    click_count: down.click_count,
                    first_mouse,
                },
                up: gpui::MouseUpEvent {
                    button: up.button.into(),
                    position: up.position,
                    modifiers: up.modifiers,
                    click_count: up.click_count,
                },
            }),
            Click::Keyboard { button, bounds } => Self::Keyboard(gpui::KeyboardClickEvent {
                button: match button {
                    KeyboardButton::Enter => gpui::KeyboardButton::Enter,
                    KeyboardButton::Space => gpui::KeyboardButton::Space,
                },
                bounds,
            }),
            Click::Touch {
                position,
                tap_count,
                long_press,
            } => Self::Touch(gpui::TouchClickEvent {
                position,
                tap_count,
                long_press,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn round_trip(event: gpui::ClickEvent) -> gpui::ClickEvent {
        let wire = Click::from(&event);
        let bytes = rmp_serde::to_vec_named(&wire).unwrap();
        let decoded: Click = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded, wire);
        let restored = gpui::ClickEvent::from(decoded);
        assert_eq!(Click::from(&restored), wire);
        restored
    }

    #[test]
    fn mouse_callback_preserves_positions_buttons_and_modifier_changes() {
        for button in gpui::MouseButton::all() {
            let restored = round_trip(gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
                down: gpui::MouseDownEvent {
                    button,
                    position: point(px(-12.5), px(34.25)),
                    modifiers: Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    click_count: 2,
                    first_mouse: true,
                },
                up: gpui::MouseUpEvent {
                    button,
                    position: point(px(12.5), px(37.25)),
                    modifiers: Modifiers {
                        control: true,
                        platform: true,
                        ..Default::default()
                    },
                    click_count: 2,
                },
            }));
            assert!(restored.modifiers().control);
            assert!(!restored.modifiers().shift);
        }
    }
    #[test]
    fn keyboard_activation_preserves_button_and_bounds() {
        for button in [gpui::KeyboardButton::Enter, gpui::KeyboardButton::Space] {
            round_trip(gpui::ClickEvent::Keyboard(gpui::KeyboardClickEvent {
                button,
                bounds: Bounds {
                    origin: point(px(13.), px(21.)),
                    size: size(px(50.), px(30.)),
                },
            }));
        }
    }
    #[test]
    fn touch_activation_preserves_long_press_and_tap_count() {
        round_trip(gpui::ClickEvent::Touch(gpui::TouchClickEvent {
            position: point(px(81.25), px(12.75)),
            tap_count: 3,
            long_press: true,
        }));
    }
}
