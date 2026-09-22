//! Theme globals shared by guest authoring and host theme notifications.

use gpui::{Global, Hsla};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: Hsla,
    pub surface: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub border: Hsla,
}

impl Global for Theme {}

impl Theme {
    pub const fn light() -> Self {
        Self {
            dark: false,
            background: Hsla { h: 0.0, s: 0.0, l: 0.98, a: 1.0 },
            surface: Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 },
            text: Hsla { h: 0.0, s: 0.0, l: 0.1, a: 1.0 },
            muted: Hsla { h: 0.0, s: 0.0, l: 0.45, a: 1.0 },
            accent: Hsla { h: 0.62, s: 0.8, l: 0.5, a: 1.0 },
            border: Hsla { h: 0.0, s: 0.0, l: 0.82, a: 1.0 },
        }
    }

    pub const fn dark() -> Self {
        Self {
            dark: true,
            background: Hsla { h: 0.0, s: 0.0, l: 0.1, a: 1.0 },
            surface: Hsla { h: 0.0, s: 0.0, l: 0.15, a: 1.0 },
            text: Hsla { h: 0.0, s: 0.0, l: 0.94, a: 1.0 },
            muted: Hsla { h: 0.0, s: 0.0, l: 0.62, a: 1.0 },
            accent: Hsla { h: 0.62, s: 0.85, l: 0.68, a: 1.0 },
            border: Hsla { h: 0.0, s: 0.0, l: 0.3, a: 1.0 },
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}
