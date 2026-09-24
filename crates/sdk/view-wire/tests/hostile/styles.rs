//! Bounds on a sanitized `StyleRefinement`, field by field.
use gpui::{AbsoluteLength, DefiniteLength, Hsla, Length, StyleRefinement};

fn in_range(value: f32, min: f32, max: f32) {
    assert!(
        value.is_finite() && (min..=max).contains(&value),
        "{value} outside {min}..={max}"
    );
}
fn absolute_in(value: AbsoluteLength, max: f32) {
    match value {
        AbsoluteLength::Pixels(value) => in_range(value.into(), 0., max),
        AbsoluteLength::Rems(value) => in_range(value.0, 0., (max / 32.).min(256.)),
    }
}
fn definite_in(value: DefiniteLength, max: f32, fraction: f32) {
    match value {
        DefiniteLength::Absolute(value) => absolute_in(value, max),
        DefiniteLength::Fraction(value) => in_range(value, 0., fraction),
    }
}
fn color_in(value: Hsla) {
    for value in [value.h, value.s, value.l, value.a] {
        in_range(value, 0., 1.);
    }
}
/// Every field [`gen_native_style`] fills is inside the bound `sanitize`
/// promises for it.
pub(super) fn check_native_style(style: &StyleRefinement) {
    for value in [
        style.inset.top,
        style.inset.right,
        style.inset.bottom,
        style.inset.left,
        style.size.width,
        style.size.height,
        style.min_size.width,
        style.min_size.height,
        style.max_size.width,
        style.max_size.height,
        style.margin.top,
        style.margin.right,
        style.margin.bottom,
        style.margin.left,
        style.flex_basis,
    ]
    .into_iter()
    .flatten()
    {
        if let Length::Definite(value) = value {
            definite_in(value, 8192., 1.);
        }
    }
    for value in [
        style.padding.top,
        style.padding.right,
        style.padding.bottom,
        style.padding.left,
        style.gap.width,
        style.gap.height,
    ]
    .into_iter()
    .flatten()
    {
        definite_in(value, 8192., 1.);
    }
    for value in [
        style.border_widths.top,
        style.border_widths.right,
        style.border_widths.bottom,
        style.border_widths.left,
        style.corner_radii.top_left,
        style.corner_radii.top_right,
        style.corner_radii.bottom_left,
        style.corner_radii.bottom_right,
        style.scrollbar_width,
    ]
    .into_iter()
    .flatten()
    {
        absolute_in(value, 8192.);
    }
    for (value, min, max) in [
        (style.flex_grow, 0., 1024.),
        (style.flex_shrink, 0., 1024.),
        (style.aspect_ratio, 1. / 1024., 1024.),
        (style.opacity, 0., 1.),
    ] {
        if let Some(value) = value {
            in_range(value, min, max);
        }
    }
    if let Some(gpui::Fill::Color(background)) = &style.background {
        color_in(
            background
                .as_solid()
                .expect("only validated solid backgrounds"),
        );
    }
    let shadows = style.box_shadow.as_deref().unwrap_or_default();
    assert!(shadows.len() <= 4);
    for shadow in shadows {
        color_in(shadow.color);
        in_range(shadow.offset.x.into(), -128., 128.);
        in_range(shadow.offset.y.into(), -128., 128.);
        in_range(shadow.blur_radius.into(), 0., 128.);
        in_range(shadow.spread_radius.into(), 0., 128.);
    }
    for template in [style.grid_cols, style.grid_rows].into_iter().flatten() {
        assert!((1..=64).contains(&template.repeat));
    }
    if let Some(location) = &style.grid_location {
        for placement in [
            &location.row.start,
            &location.row.end,
            &location.column.start,
            &location.column.end,
        ] {
            match placement {
                gpui::GridPlacement::Line(value) => assert!((-64..=64).contains(value)),
                gpui::GridPlacement::Span(value) => assert!((1..=64).contains(value)),
                gpui::GridPlacement::Auto => {}
            }
        }
    }
    for color in [
        style.border_color,
        style.text.color,
        style.text.background_color,
    ]
    .into_iter()
    .flatten()
    {
        color_in(color);
    }
    if let Some(size) = style.text.font_size {
        absolute_in(size, 512.);
    }
    if let Some(height) = style.text.line_height {
        definite_in(height, 512., 8.);
    }
    if let Some(weight) = style.text.font_weight {
        in_range(weight.0, 1., 1000.);
    }
    if let Some(clamp) = style.text.line_clamp {
        assert!((1..=1024).contains(&clamp));
    }
    if let Some(underline) = &style.text.underline {
        in_range(underline.thickness.into(), 0., 32.);
        if let Some(color) = underline.color {
            color_in(color);
        }
    }
    if let Some(strike) = &style.text.strikethrough {
        in_range(strike.thickness.into(), 0., 32.);
        if let Some(color) = strike.color {
            color_in(color);
        }
    }
}
