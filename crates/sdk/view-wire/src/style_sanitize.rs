//! Bounds applied before a guest refinement reaches native layout or painting.
//! The host still clips the entire view slot: local clipping cannot contain a
//! deferred or anchored element on its own.
use gpui::{
    AbsoluteLength, DefiniteLength, Fill, GridPlacement, Hsla, Length, StyleRefinement, px,
};

const MAX_PIXELS: f32 = 8192.;
const MAX_FONT_PIXELS: f32 = 512.;
const MAX_REMS: f32 = 256.;
const MAX_GRID: u16 = 64;
const MAX_SHADOWS: usize = 4;

fn finite(value: &mut f32, min: f32, max: f32) {
    *value = if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    };
}
fn absolute(value: &mut AbsoluteLength, max: f32) {
    match value {
        AbsoluteLength::Pixels(value) => {
            let mut number = f32::from(*value);
            finite(&mut number, 0., max);
            *value = px(number);
        }
        AbsoluteLength::Rems(value) => finite(&mut value.0, 0., (max / 32.).min(MAX_REMS)),
    }
}
fn definite(value: &mut DefiniteLength, max: f32) {
    match value {
        DefiniteLength::Absolute(value) => absolute(value, max),
        DefiniteLength::Fraction(value) => finite(value, 0., 1.),
    }
}
fn length(value: &mut Length) {
    if let Length::Definite(value) = value {
        definite(value, MAX_PIXELS);
    }
}
pub(crate) fn sanitize_hsla(value: &mut Hsla) {
    for number in [&mut value.h, &mut value.s, &mut value.l, &mut value.a] {
        finite(number, 0., 1.);
    }
}
fn truncate(value: &mut String, max: usize) {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
fn grid(value: &mut GridPlacement) {
    match value {
        GridPlacement::Line(value) => *value = (*value).clamp(-(MAX_GRID as i16), MAX_GRID as i16),
        GridPlacement::Span(value) => *value = (*value).clamp(1, MAX_GRID),
        GridPlacement::Auto => {}
    }
}

/// Apply once to each base and conditional refinement during the tree's
/// existing sanitize walk. Absolute positioning remains local to the slot;
/// finite, nonnegative offsets/margins and hard host clipping bound its reach.
/// This pinned GPUI revision has no z-index field in StyleRefinement. Deferred
/// painting must be bounded separately by the host's slot content mask.
pub(crate) fn sanitize(style: &mut StyleRefinement) {
    for value in [
        &mut style.inset.top,
        &mut style.inset.right,
        &mut style.inset.bottom,
        &mut style.inset.left,
        &mut style.size.width,
        &mut style.size.height,
        &mut style.min_size.width,
        &mut style.min_size.height,
        &mut style.max_size.width,
        &mut style.max_size.height,
        &mut style.margin.top,
        &mut style.margin.right,
        &mut style.margin.bottom,
        &mut style.margin.left,
        &mut style.flex_basis,
    ]
    .into_iter()
    .flatten()
    {
        length(value);
    }
    for value in [
        &mut style.padding.top,
        &mut style.padding.right,
        &mut style.padding.bottom,
        &mut style.padding.left,
        &mut style.gap.width,
        &mut style.gap.height,
    ]
    .into_iter()
    .flatten()
    {
        definite(value, MAX_PIXELS);
    }
    for value in [
        &mut style.border_widths.top,
        &mut style.border_widths.right,
        &mut style.border_widths.bottom,
        &mut style.border_widths.left,
        &mut style.corner_radii.top_left,
        &mut style.corner_radii.top_right,
        &mut style.corner_radii.bottom_left,
        &mut style.corner_radii.bottom_right,
        &mut style.scrollbar_width,
    ]
    .into_iter()
    .flatten()
    {
        absolute(value, MAX_PIXELS);
    }
    for value in [&mut style.flex_grow, &mut style.flex_shrink]
        .into_iter()
        .flatten()
    {
        finite(value, 0., 1024.);
    }
    if let Some(value) = &mut style.aspect_ratio {
        finite(value, 1. / 1024., 1024.);
    }
    if let Some(value) = &mut style.opacity {
        finite(value, 0., 1.);
    }
    if let Some(value) = &mut style.border_color {
        sanitize_hsla(value);
    }
    // Background's pattern/gradient payload is private in pinned GPUI. Only
    // solids expose the numeric values needed for validation; reject opaque
    // pattern parameters rather than forwarding unchecked shader inputs.
    style.background = style.background.take().and_then(|Fill::Color(background)| {
        background.as_solid().map(|mut value| {
            sanitize_hsla(&mut value);
            Fill::from(value)
        })
    });
    if let Some(shadows) = &mut style.box_shadow {
        shadows.truncate(MAX_SHADOWS);
        for shadow in shadows {
            sanitize_hsla(&mut shadow.color);
            for offset in [&mut shadow.offset.x, &mut shadow.offset.y] {
                let mut number = f32::from(*offset);
                finite(&mut number, -128., 128.);
                *offset = px(number);
            }
            for radius in [&mut shadow.blur_radius, &mut shadow.spread_radius] {
                let mut number = f32::from(*radius);
                finite(&mut number, 0., 128.);
                *radius = px(number);
            }
        }
    }
    for template in [&mut style.grid_cols, &mut style.grid_rows]
        .into_iter()
        .flatten()
    {
        template.repeat = template.repeat.clamp(1, MAX_GRID);
    }
    if let Some(location) = &mut style.grid_location {
        for value in [
            &mut location.row.start,
            &mut location.row.end,
            &mut location.column.start,
            &mut location.column.end,
        ] {
            grid(value);
        }
    }
    let text = &mut style.text;
    if let Some(value) = &mut text.color {
        sanitize_hsla(value);
    }
    if let Some(value) = &mut text.background_color {
        sanitize_hsla(value);
    }
    if let Some(value) = &mut text.font_size {
        absolute(value, MAX_FONT_PIXELS);
    }
    if let Some(value) = &mut text.line_height {
        match value {
            DefiniteLength::Absolute(value) => absolute(value, MAX_FONT_PIXELS),
            DefiniteLength::Fraction(value) => finite(value, 0., 8.),
        }
    }
    if let Some(value) = &mut text.font_weight {
        finite(&mut value.0, 1., 1000.);
    }
    if let Some(value) = &mut text.line_clamp {
        *value = (*value).clamp(1, 1024);
    }
    if let Some(value) = &mut text.font_family {
        let mut name = value.to_string();
        truncate(&mut name, 256);
        *value = name.into();
    }
    if let Some(fallbacks) = &mut text.font_fallbacks {
        let fonts = std::sync::Arc::make_mut(&mut fallbacks.0);
        fonts.truncate(16);
        for name in fonts {
            truncate(name, 256);
        }
    }
    if let Some(features) = &mut text.font_features {
        let features = std::sync::Arc::make_mut(&mut features.0);
        features.truncate(64);
        features.retain(|(tag, _)| tag.len() == 4 && tag.is_ascii());
    }
    if let Some(overflow) = &mut text.text_overflow {
        let (gpui::TextOverflow::Truncate(value)
        | gpui::TextOverflow::TruncateStart(value)
        | gpui::TextOverflow::TruncateMiddle(value)) = overflow;
        let mut string = value.to_string();
        truncate(&mut string, 32);
        *value = string.into();
    }
    if let Some(value) = &mut text.underline {
        let mut thickness = f32::from(value.thickness);
        finite(&mut thickness, 0., 32.);
        value.thickness = px(thickness);
        if let Some(value) = &mut value.color {
            sanitize_hsla(value);
        }
    }
    if let Some(value) = &mut text.strikethrough {
        let mut thickness = f32::from(value.thickness);
        finite(&mut thickness, 0., 32.);
        value.thickness = px(thickness);
        if let Some(value) = &mut value.color {
            sanitize_hsla(value);
        }
    }
    #[cfg(debug_assertions)]
    {
        style.debug = None;
        style.debug_below = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Overflow, Position, Styled, relative, rems, rgb};

    #[test]
    fn absolute_position_and_negative_margins_cannot_move_before_slot_origin() {
        let mut style = StyleRefinement::default()
            .absolute()
            .left(px(-50.))
            .m(px(-20.));
        sanitize(&mut style);
        assert_eq!(style.position, Some(Position::Absolute));
        assert_eq!(style.inset.left, Some(px(0.).into()));
        assert_eq!(style.margin.top, Some(px(0.).into()));
    }
    #[test]
    fn bounds_pixels_rems_fractions_and_nonfinite_dimensions() {
        let mut style = StyleRefinement::default()
            .w(px(f32::INFINITY))
            .h(rems(10000.))
            .min_w(relative(1000.));
        style.max_size.height = Some(px(1e20).into());
        sanitize(&mut style);
        assert_eq!(style.size.width, Some(px(0.).into()));
        assert_eq!(style.size.height, Some(rems(MAX_REMS).into()));
        assert_eq!(style.min_size.width, Some(relative(1.).into()));
        assert_eq!(style.max_size.height, Some(px(MAX_PIXELS).into()));
    }
    #[test]
    fn opacity_and_color_are_finite_and_bounded() {
        let mut style = StyleRefinement::default().opacity(f32::NAN);
        style.border_color = Some(Hsla {
            h: -1.,
            s: 2.,
            l: f32::INFINITY,
            a: 9.,
        });
        sanitize(&mut style);
        assert_eq!(style.opacity, Some(0.));
        assert_eq!(
            style.border_color,
            Some(Hsla {
                h: 0.,
                s: 1.,
                l: 0.,
                a: 1.
            })
        );
    }
    #[test]
    fn visible_overflow_is_local_and_requires_host_slot_clip() {
        let mut style = StyleRefinement::default();
        style.overflow.x = Some(Overflow::Visible);
        sanitize(&mut style);
        assert_eq!(style.overflow.x, Some(Overflow::Visible));
    }
    #[test]
    fn bounds_shadow_count_and_gpu_radius() {
        let mut style = StyleRefinement::default();
        style.box_shadow = Some(vec![
            gpui::BoxShadow {
                color: rgb(0).into(),
                offset: gpui::point(px(f32::NAN), px(1000.)),
                blur_radius: px(1e9),
                spread_radius: px(-100.),
                inset: false,
            };
            100
        ]);
        sanitize(&mut style);
        let shadows = style.box_shadow.unwrap();
        assert_eq!(shadows.len(), MAX_SHADOWS);
        assert_eq!(shadows[0].blur_radius, px(128.));
        assert_eq!(shadows[0].offset.x, px(-128.));
        assert_eq!(shadows[0].spread_radius, px(0.));
    }
    #[test]
    fn caps_grid_expansion_and_font_rasterization() {
        let mut style = StyleRefinement::default().text_size(px(1e9));
        style.grid_cols = Some(gpui::GridTemplate {
            repeat: u16::MAX,
            ..Default::default()
        });
        style.grid_location = Some(gpui::GridLocation {
            row: GridPlacement::Span(u16::MAX)..GridPlacement::Line(i16::MIN),
            column: GridPlacement::Auto..GridPlacement::Auto,
        });
        sanitize(&mut style);
        assert_eq!(style.text.font_size, Some(px(MAX_FONT_PIXELS).into()));
        assert_eq!(style.grid_cols.unwrap().repeat, MAX_GRID);
        assert_eq!(
            style.grid_location.unwrap().row.start,
            GridPlacement::Span(MAX_GRID)
        );
    }
    #[test]
    fn preserves_normal_line_spacing_and_is_idempotent() {
        let mut style = StyleRefinement::default()
            .text_size(px(16.))
            .line_height(relative(1.5));
        sanitize(&mut style);
        assert_eq!(style.text.line_height, Some(relative(1.5)));
        let once = style.clone();
        sanitize(&mut style);
        assert_eq!(style, once);
    }
    #[test]
    fn bounds_font_lookup_lists_and_custom_ellipsis_at_utf8_boundaries() {
        let mut style = StyleRefinement::default().font_family("λ".repeat(200));
        style.text.font_fallbacks =
            Some(gpui::FontFallbacks::from_fonts(vec!["λ".repeat(200); 30]));
        style.text.font_features = Some(gpui::FontFeatures(std::sync::Arc::new(vec![
            (
                "liga".into(),
                1
            );
            100
        ])));
        style.text.text_overflow = Some(gpui::TextOverflow::Truncate("λ".repeat(100).into()));
        sanitize(&mut style);
        assert_eq!(style.text.font_family.unwrap().len(), 256);
        let fonts = style.text.font_fallbacks.unwrap();
        assert_eq!(fonts.fallback_list().len(), 16);
        assert!(fonts.fallback_list().iter().all(|name| name.len() == 256));
        assert_eq!(style.text.font_features.unwrap().tag_value_list().len(), 64);
        assert_eq!(
            style.text.text_overflow,
            Some(gpui::TextOverflow::Truncate("λ".repeat(16).into()))
        );
    }

    #[test]
    fn opaque_pattern_inputs_are_rejected_but_solid_colors_survive() {
        let mut style = StyleRefinement::default().bg(gpui::linear_gradient(
            0.,
            gpui::linear_color_stop(rgb(0), 0.),
            gpui::linear_color_stop(rgb(0xffffff), 1.),
        ));
        sanitize(&mut style);
        assert!(style.background.is_none());
        let mut solid = StyleRefinement::default().bg(rgb(0x123456));
        let before = solid.background.clone();
        sanitize(&mut solid);
        assert_eq!(solid.background, before);
    }
}
