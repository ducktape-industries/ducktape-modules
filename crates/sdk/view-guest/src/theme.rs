//! The host appearance expressed as a GPUI global and GPUI color tokens.
use gpui::{Global, Hsla, Rgba};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub sidebar_muted: Hsla,
    pub sidebar_raised: Hsla,
    pub sidebar_border: Hsla,
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub accent_foreground: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub link: Hsla,
    pub success: Hsla,
    pub success_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub agent: Hsla,
    pub agent_soft: Hsla,
    pub hover: Hsla,
}
impl Global for Theme {}
impl Theme {
    pub fn light() -> Self { Self::from_palette(false) }
    pub fn dark() -> Self { Self::from_palette(true) }
    fn from_palette(dark: bool) -> Self {
        let palette = design::palette(dark);
        let color = |[r, g, b, a]: design::Color| Hsla::from(Rgba { r, g, b, a });
        Self {
            dark,
            sidebar: color(palette.sidebar),
            sidebar_foreground: color(palette.sidebar_foreground),
            sidebar_muted: color(palette.sidebar_muted),
            sidebar_raised: color(palette.sidebar_raised),
            sidebar_border: color(palette.sidebar_border),
            background: color(palette.background),
            surface: color(palette.surface),
            surface_raised: color(palette.surface_raised),
            border: color(palette.border),
            border_strong: color(palette.border_strong),
            foreground: color(palette.foreground),
            muted: color(palette.muted),
            faint: color(palette.faint),
            accent: color(palette.accent),
            accent_soft: color(palette.accent_soft),
            accent_foreground: color(palette.accent_foreground),
            primary: color(palette.primary),
            primary_foreground: color(palette.primary_foreground),
            link: color(palette.link),
            success: color(palette.success),
            success_soft: color(palette.success_soft),
            warning: color(palette.warning),
            warning_soft: color(palette.warning_soft),
            danger: color(palette.danger),
            danger_soft: color(palette.danger_soft),
            agent: color(palette.agent),
            agent_soft: color(palette.agent_soft),
            hover: color(palette.surface_raised),
        }
    }
}
impl Default for Theme {
    fn default() -> Self { Self::light() }
}
