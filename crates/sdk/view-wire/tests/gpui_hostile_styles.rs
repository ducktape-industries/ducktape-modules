//! Deterministic hostile GPUI refinements through the actual frame/patch boundary.
use gpui::{AbsoluteLength, DefiniteLength, Hsla, Length, StyleRefinement, px, rems};
use view_wire::{Frame, GroupRefinement, Interactivity, Node};

struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
    fn number(&mut self) -> f32 {
        match self.next() >> 60 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => f32::NEG_INFINITY,
            3 => f32::MAX,
            4 => f32::MIN,
            _ => f32::from_bits(self.next() as u32),
        }
    }
    fn absolute(&mut self) -> AbsoluteLength {
        if self.next() & 1 == 0 { px(self.number()).into() }
        else { rems(self.number()).into() }
    }
    fn definite(&mut self) -> DefiniteLength {
        if self.next() & 1 == 0 { self.absolute().into() }
        else { DefiniteLength::Fraction(self.number()) }
    }
    fn color(&mut self) -> Hsla {
        Hsla { h: self.number(), s: self.number(), l: self.number(), a: self.number() }
    }
    fn style(&mut self) -> StyleRefinement {
        let mut style = StyleRefinement::default();
        for value in [
            &mut style.inset.top, &mut style.inset.right, &mut style.inset.bottom,
            &mut style.inset.left, &mut style.size.width, &mut style.size.height,
            &mut style.min_size.width, &mut style.min_size.height,
            &mut style.max_size.width, &mut style.max_size.height,
            &mut style.margin.top, &mut style.margin.right, &mut style.margin.bottom,
            &mut style.margin.left, &mut style.flex_basis,
        ] { *value = Some(self.definite().into()); }
        for value in [
            &mut style.padding.top, &mut style.padding.right,
            &mut style.padding.bottom, &mut style.padding.left,
            &mut style.gap.width, &mut style.gap.height,
        ] { *value = Some(self.definite()); }
        for value in [
            &mut style.border_widths.top, &mut style.border_widths.right,
            &mut style.border_widths.bottom, &mut style.border_widths.left,
            &mut style.corner_radii.top_left, &mut style.corner_radii.top_right,
            &mut style.corner_radii.bottom_left, &mut style.corner_radii.bottom_right,
            &mut style.scrollbar_width,
        ] { *value = Some(self.absolute()); }
        style.flex_grow = Some(self.number());
        style.flex_shrink = Some(self.number());
        style.aspect_ratio = Some(self.number());
        style.opacity = Some(self.number());
        style.border_color = Some(self.color());
        style.background = Some(self.color().into());
        let count = self.next() as usize % 20;
        style.box_shadow = Some((0..count).map(|_| gpui::BoxShadow {
            color: self.color(), offset: gpui::point(px(self.number()), px(self.number())),
            blur_radius: px(self.number()), spread_radius: px(self.number()), inset: false,
        }).collect());
        style.grid_cols = Some(gpui::GridTemplate { repeat: self.next() as u16, ..Default::default() });
        style.grid_rows = Some(gpui::GridTemplate { repeat: self.next() as u16, ..Default::default() });
        style.grid_location = Some(gpui::GridLocation {
            row: gpui::GridPlacement::Line(self.next() as i16)..gpui::GridPlacement::Span(self.next() as u16),
            column: gpui::GridPlacement::Span(self.next() as u16)..gpui::GridPlacement::Line(self.next() as i16),
        });
        style.text.color = Some(self.color());
        style.text.background_color = Some(self.color());
        style.text.font_size = Some(self.absolute());
        style.text.line_height = Some(self.definite());
        style.text.font_weight = Some(gpui::FontWeight(self.number()));
        style.text.line_clamp = Some(self.next() as usize);
        style.text.underline = Some(gpui::UnderlineStyle {
            thickness: px(self.number()), color: Some(self.color()), wavy: true,
        });
        style.text.strikethrough = Some(gpui::StrikethroughStyle {
            thickness: px(self.number()), color: Some(self.color()),
        });
        style
    }
}

fn number(value: f32, min: f32, max: f32) {
    assert!(value.is_finite() && (min..=max).contains(&value), "{value} outside {min}..={max}");
}
fn absolute(value: AbsoluteLength, max: f32) {
    match value {
        AbsoluteLength::Pixels(value) => number(value.into(), 0., max),
        AbsoluteLength::Rems(value) => number(value.0, 0., (max / 32.).min(256.)),
    }
}
fn definite(value: DefiniteLength, max: f32, fraction: f32) {
    match value {
        DefiniteLength::Absolute(value) => absolute(value, max),
        DefiniteLength::Fraction(value) => number(value, 0., fraction),
    }
}
fn color(value: Hsla) {
    for value in [value.h, value.s, value.l, value.a] { number(value, 0., 1.); }
}
fn bounded(style: &StyleRefinement) {
    for value in [
        style.inset.top, style.inset.right, style.inset.bottom, style.inset.left,
        style.size.width, style.size.height, style.min_size.width, style.min_size.height,
        style.max_size.width, style.max_size.height, style.margin.top, style.margin.right,
        style.margin.bottom, style.margin.left, style.flex_basis,
    ].into_iter().flatten() {
        if let Length::Definite(value) = value { definite(value, 8192., 1.); }
    }
    for value in [style.padding.top, style.padding.right, style.padding.bottom,
        style.padding.left, style.gap.width, style.gap.height].into_iter().flatten() {
        definite(value, 8192., 1.);
    }
    for value in [style.border_widths.top, style.border_widths.right,
        style.border_widths.bottom, style.border_widths.left, style.corner_radii.top_left,
        style.corner_radii.top_right, style.corner_radii.bottom_left,
        style.corner_radii.bottom_right, style.scrollbar_width].into_iter().flatten() {
        absolute(value, 8192.);
    }
    number(style.flex_grow.unwrap(), 0., 1024.);
    number(style.flex_shrink.unwrap(), 0., 1024.);
    number(style.aspect_ratio.unwrap(), 1. / 1024., 1024.);
    number(style.opacity.unwrap(), 0., 1.);
    color(style.border_color.unwrap());
    if let Some(gpui::Fill::Color(background)) = &style.background {
        color(background.as_solid().expect("only validated solid backgrounds"));
    }
    let shadows = style.box_shadow.as_ref().unwrap();
    assert!(shadows.len() <= 4);
    for shadow in shadows {
        color(shadow.color);
        number(shadow.offset.x.into(), -128., 128.);
        number(shadow.offset.y.into(), -128., 128.);
        number(shadow.blur_radius.into(), 0., 128.);
        number(shadow.spread_radius.into(), 0., 128.);
    }
    for template in [style.grid_cols, style.grid_rows].into_iter().flatten() {
        assert!((1..=64).contains(&template.repeat));
    }
    let location = style.grid_location.as_ref().unwrap();
    for placement in [&location.row.start, &location.row.end, &location.column.start, &location.column.end] {
        match placement {
            gpui::GridPlacement::Line(value) => assert!((-64..=64).contains(value)),
            gpui::GridPlacement::Span(value) => assert!((1..=64).contains(value)),
            gpui::GridPlacement::Auto => {},
        }
    }
    color(style.text.color.unwrap());
    color(style.text.background_color.unwrap());
    absolute(style.text.font_size.unwrap(), 512.);
    definite(style.text.line_height.unwrap(), 512., 8.);
    number(style.text.font_weight.unwrap().0, 1., 1000.);
    assert!((1..=1024).contains(&style.text.line_clamp.unwrap()));
    let underline = style.text.underline.unwrap();
    number(underline.thickness.into(), 0., 32.);
    color(underline.color.unwrap());
    let strike = style.text.strikethrough.unwrap();
    number(strike.thickness.into(), 0., 32.);
    color(strike.color.unwrap());
}

fn node(random: &mut Random) -> Node {
    Node::Container {
        id: Some(view_wire::ElementIdWire::Integer(42)),
        style: random.style(),
        interactivity: Interactivity {
            hover: Some(random.style()), active: Some(random.style()),
            group_hover: Some(GroupRefinement { group: "row".into(), style: random.style() }),
            group_active: Some(GroupRefinement { group: "row".into(), style: random.style() }),
            ..Default::default()
        },
        children: vec![Node::Text {
            id: None, style: random.style(), content: "kept".into(), heading: None, live: None,
        }],
    }
}
fn check(node: &Node) {
    let Node::Container { style, interactivity, children, .. } = node else { panic!("container") };
    for style in [style, interactivity.hover.as_ref().unwrap(), interactivity.active.as_ref().unwrap(),
        &interactivity.group_hover.as_ref().unwrap().style, &interactivity.group_active.as_ref().unwrap().style] {
        bounded(style);
    }
    let Node::Text { style, content, .. } = &children[0] else { panic!("text") };
    assert_eq!(content, "kept");
    bounded(style);
}

#[test]
fn random_styles_remain_bounded_after_named_messagepack_and_incremental_patches() {
    for seed in 0..256 {
        let mut random = Random(seed);
        let frame = Frame { root: Some(node(&mut random)), ..Default::default() };
        let mut frame: Frame = view_wire::decode(&view_wire::encode(&frame)).unwrap();
        view_wire::sanitize(&mut frame).unwrap();
        let mut old = frame.root.clone().unwrap();
        check(&old);
        let once = frame.clone();
        view_wire::sanitize(&mut frame).unwrap();
        assert_eq!(frame, once, "sanitization must be idempotent, seed {seed}");
        let mut new = node(&mut random);
        let patch = view_wire::diff(&mut old, &mut new);
        let patch = view_wire::decode(&view_wire::encode(&patch)).unwrap();
        view_wire::apply(&mut old, patch).unwrap();
        check(&old);
    }
}
