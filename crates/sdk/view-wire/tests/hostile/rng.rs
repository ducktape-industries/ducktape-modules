use super::*;

// ---------------------------------------------------------------- splitmix64

/// A tiny deterministic PRNG so the property tests need no new dependency.
/// splitmix64: https://prng.di.unimi.it/splitmix64.c
///
/// The flag makes [`gen_id`] draw a host-local id now and then.
pub(super) struct Rng(u64, bool);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed, false)
    }

    /// A generator whose ids are sometimes ones the host must refuse.
    pub(super) fn poisoning_ids(seed: u64) -> Self {
        Self(seed, true)
    }

    pub(super) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(super) fn next_range(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() as usize) % bound
    }

    pub(super) fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// A fraction in `0.0..1.0`, from the PRNG's top 24 bits.
    pub(super) fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 40) as f64) / ((1u64 << 24) as f64)
    }

    /// A value in `0..=max`, biased toward small values by raising a
    /// uniform fraction to `exponent` before scaling: the higher the
    /// exponent, the more the mass sits near zero. Keeps most generated
    /// trees and strings cheap while still drawing the occasional value
    /// near `max` to exercise the wire's ceilings.
    pub(super) fn skewed(&mut self, max: usize, exponent: i32) -> usize {
        if max == 0 {
            return 0;
        }
        let biased = self.next_unit().powi(exponent);
        ((biased * max as f64) as usize).min(max)
    }

    pub(super) fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_range(items.len())]
    }
}

// ------------------------------------------------------------- tree generator

/// A hostile f32: sometimes a normal-looking value, sometimes one of the
/// exact values `sanitize` exists to handle (NaN, both infinities, the
/// float extremes, and out-of-range negatives).
pub(super) fn gen_f32(rng: &mut Rng) -> f32 {
    match rng.next_range(20) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => f32::MAX,
        4 => f32::MIN,
        5 => -(rng.skewed(1_000_000, 2) as f32),
        _ => rng.skewed(20_000, 2) as f32 - 5_000.0,
    }
}

pub(super) fn gen_opt_f32(rng: &mut Rng) -> Option<f32> {
    rng.next_bool().then(|| gen_f32(rng))
}

/// A key drawn from a small fixed pool: with only five options across a
/// whole tree, collisions are the common case rather than the exception.
/// A node's typed id; from a [`Rng::poisoning_ids`] generator, one in
/// eight is a focus handle, which never crosses the wire.
pub(super) fn gen_id(rng: &mut Rng) -> ElementIdWire {
    if rng.1 && rng.next_range(8) == 0 {
        return ElementIdWire::FocusHandle(rng.next_u64());
    }
    ElementIdWire::Name(gen_key(rng).into())
}

pub(super) fn gen_key(rng: &mut Rng) -> String {
    const POOL: [&str; 5] = ["App/a", "App/b", "dup", "x", "same-key"];
    (*rng.choose(&POOL)).to_string()
}

/// A string built from single-, two-, three- and four-byte UTF-8
/// characters. Most calls stay small so a tree of thousands of leaves
/// stays cheap to build; roughly one in three hundred goes hostile and
/// targets up to `3 * MAX_STRING_BYTES`, which is what actually exercises
/// `truncate`'s char-boundary walk.
pub(super) fn gen_string(rng: &mut Rng) -> String {
    const POOL: [char; 6] = ['a', 'Z', 'é', '한', '😀', '\n'];
    let (cap, exponent) = match rng.next_range(300) {
        0 => (3 * MAX_STRING_BYTES, 8),
        _ => (48, 2),
    };
    let target = rng.skewed(cap, exponent);
    let mut s = String::new();
    while s.len() < target {
        s.push(*rng.choose(&POOL));
    }
    s
}

pub(super) fn gen_opt_role(rng: &mut Rng) -> Option<Role> {
    rng.next_bool().then(|| {
        *rng.choose(&[
            Role::Button,
            Role::Link,
            Role::Tab,
            Role::MenuItem,
            Role::Row,
            Role::Checkbox,
            Role::Switch,
        ])
    })
}

pub(super) fn gen_axis(rng: &mut Rng) -> Axis {
    if rng.next_bool() {
        Axis::Column
    } else {
        Axis::Row
    }
}

pub(super) fn gen_anchor(rng: &mut Rng) -> ScrollAnchor {
    *rng.choose(&[ScrollAnchor::Start, ScrollAnchor::End, ScrollAnchor::Keep])
}

pub(super) fn gen_button_label(rng: &mut Rng) -> Node {
    Node::Button {
        checked: rng.next_bool().then(|| rng.next_bool()),
        expanded: rng.next_bool().then(|| rng.next_bool()),
        selected: rng.next_bool().then(|| rng.next_bool()),
        role: gen_opt_role(rng),
        description: rng.next_bool().then(|| gen_string(rng)),
        id: gen_id(rng),
        content: ButtonContent::Label(gen_string(rng)),
        label: rng.next_bool().then(|| gen_string(rng)),
        on_press: rng.next_bool().then(|| rng.next_u64() as u32),
        style: gen_native_style(rng),
    }
}

pub(super) fn gen_input(rng: &mut Rng) -> Node {
    Node::Input {
        options: InputOptions {
            label: gen_string(rng),
            description: Some(gen_string(rng)),
            disabled: rng.next_bool(),
        },
        id: gen_id(rng),
        placeholder: gen_string(rng),
        value: gen_string(rng),
        on_input: rng.next_u64() as u32,
        on_submit: rng.next_bool().then(|| rng.next_u64() as u32),
        secure: rng.next_bool(),
        style: gpui::StyleRefinement::default(),
    }
}

/// One logical document per identifier, so every reference the tree makes to
/// the same document is exactly the one `validate_editor_document_refs`
/// requires. Byte lengths stay small: a projection is charged per binding, and
/// the fuzz is about tree shape, not the aggregate byte ceilings the focused
/// `editor_document` tests already pin.
pub(super) fn gen_document(rng: &mut Rng) -> editor_document::EditorDocumentRef {
    const POOL: [&str; 5] = ["app:draft", "app:notes", "dup", "x", "app:same"];
    let index = rng.next_range(POOL.len());
    let byte_len = (index * 37) as u32;
    editor_document::EditorDocumentRef {
        document: POOL[index].to_string(),
        reset: index as u64,
        text_revision: 2 * index as u64,
        revision: 3 * index as u64,
        cursor: EditorCursor {
            position: EditorPosition {
                line: 0,
                column: byte_len,
            },
            selection: None,
        },
        byte_len,
    }
}

pub(super) fn gen_editor(rng: &mut Rng) -> Node {
    Node::Editor {
        document: gen_document(rng),
        on_document: rng.next_u64() as u32,
        editable: rng.next_bool(),
        options: Box::new(EditorOptions {
            rich: None,
            presentation: None,
            binding: None,
        }),
        id: gen_id(rng),
        style: gpui::StyleRefinement::default(),
        placeholder: gen_string(rng),
        label: rng.next_bool().then(|| gen_string(rng)),
    }
}

pub(super) fn gen_rule(rng: &mut Rng) -> Node {
    Node::Rule {
        id: gen_id(rng),
        axis: gen_axis(rng),
        style: gen_native_style(rng),
    }
}

pub(super) fn gen_text(rng: &mut Rng) -> Node {
    Node::Text(view_wire::TextNode {
        id: None,
        style: if rng.next_range(8) == 0 {
            gen_native_style(rng)
        } else {
            gpui::StyleRefinement::default()
        },
        content: gen_string(rng),
        // 0 and 7 are outside 1..=6, for the sanitizer to drop.
        heading: rng.next_bool().then(|| rng.next_range(8) as u8),
        live: rng
            .next_bool()
            .then(|| *rng.choose(&[Live::Polite, Live::Assertive])),
    })
}

/// A whole hostile refinement: every field `sanitize` bounds, drawn from
/// [`gen_f32`], so a tree exercises each clamp on every styled node.
pub(super) fn gen_native_style(rng: &mut Rng) -> gpui::StyleRefinement {
    use gpui::{AbsoluteLength, DefiniteLength, px, rems};
    let number = gen_f32;
    let absolute = |rng: &mut Rng| -> AbsoluteLength {
        if rng.next_bool() {
            px(number(rng)).into()
        } else {
            rems(number(rng)).into()
        }
    };
    let definite = |rng: &mut Rng| -> DefiniteLength {
        if rng.next_bool() {
            absolute(rng).into()
        } else {
            DefiniteLength::Fraction(number(rng))
        }
    };
    let mut style = gpui::StyleRefinement::default();
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
    ] {
        *value = Some(definite(rng).into());
    }
    for value in [
        &mut style.padding.top,
        &mut style.padding.right,
        &mut style.padding.bottom,
        &mut style.padding.left,
        &mut style.gap.width,
        &mut style.gap.height,
    ] {
        *value = Some(definite(rng));
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
    ] {
        *value = Some(absolute(rng));
    }
    style.flex_grow = Some(gen_f32(rng));
    style.flex_shrink = Some(gen_f32(rng));
    style.aspect_ratio = Some(gen_f32(rng));
    style.opacity = Some(gen_f32(rng));
    style.border_color = Some(gen_color(rng));
    style.background = Some(gen_color(rng).into());
    style.box_shadow = Some(
        (0..rng.next_range(20))
            .map(|_| gpui::BoxShadow {
                color: gen_color(rng),
                offset: gpui::point(px(gen_f32(rng)), px(gen_f32(rng))),
                blur_radius: px(gen_f32(rng)),
                spread_radius: px(gen_f32(rng)),
                inset: false,
            })
            .collect(),
    );
    style.grid_cols = Some(gpui::GridTemplate {
        repeat: rng.next_u64() as u16,
        ..Default::default()
    });
    style.grid_rows = Some(gpui::GridTemplate {
        repeat: rng.next_u64() as u16,
        ..Default::default()
    });
    style.grid_location = Some(gpui::GridLocation {
        row: gpui::GridPlacement::Line(rng.next_u64() as i16)
            ..gpui::GridPlacement::Span(rng.next_u64() as u16),
        column: gpui::GridPlacement::Span(rng.next_u64() as u16)
            ..gpui::GridPlacement::Line(rng.next_u64() as i16),
    });
    style.text.color = Some(gen_color(rng));
    style.text.background_color = Some(gen_color(rng));
    style.text.font_size = Some(absolute(rng));
    style.text.line_height = Some(definite(rng));
    style.text.font_weight = Some(gpui::FontWeight(gen_f32(rng)));
    style.text.line_clamp = Some(rng.next_u64() as usize);
    style.text.underline = Some(gpui::UnderlineStyle {
        thickness: px(gen_f32(rng)),
        color: Some(gen_color(rng)),
        wavy: true,
    });
    style.text.strikethrough = Some(gpui::StrikethroughStyle {
        thickness: px(gen_f32(rng)),
        color: Some(gen_color(rng)),
    });
    style
}

pub(super) fn gen_color(rng: &mut Rng) -> gpui::Hsla {
    gpui::Hsla {
        h: gen_f32(rng),
        s: gen_f32(rng),
        l: gen_f32(rng),
        a: gen_f32(rng),
    }
}

/// A picture whose bytes cross about half the time, and about one time in
/// sixteen run past `MAX_PICTURE_BYTES_PER_FRAME` on their own.
pub(super) fn gen_svg(rng: &mut Rng) -> Node {
    let bytes = rng.next_bool().then(|| {
        let len = match rng.next_range(16) {
            0 => MAX_PICTURE_BYTES_PER_FRAME + 1 + rng.next_range(64),
            _ => rng.skewed(4096, 2),
        };
        vec![b'<'; len]
    });
    if rng.next_bool() {
        let data = bytes.map(|bytes| {
            if rng.next_bool() {
                ImageData::Encoded(bytes)
            } else {
                ImageData::Rgba {
                    width: rng.next_u64() as u32,
                    height: rng.next_u64() as u32,
                    pixels: bytes,
                }
            }
        });
        if rng.next_bool() {
            return Node::ImageViewer {
                id: gen_id(rng),
                hash: rng.next_u64(),
                data,
                label: rng.next_bool().then(|| gen_string(rng)),
                fit: None,
                options: ViewerOptions {
                    padding: gen_opt_f32(rng),
                    scale_bounds: rng.next_bool().then(|| (gen_f32(rng), gen_f32(rng))),
                    scale_step: gen_opt_f32(rng),
                },
                style: gpui::StyleRefinement::default(),
            };
        }
        return Node::Image {
            id: Some(gen_id(rng)),
            hash: rng.next_u64(),
            data,
            label: rng.next_bool().then(|| gen_string(rng)),
            image_style: ImageStyle {
                grayscale: rng.next_bool(),
                object_fit: ImageObjectFit::Contain,
            },
            loading: false,
            fallback: false,
            state_children: Vec::new(),
            style: gen_native_style(rng),
            interactivity: Interactivity::default(),
        };
    }
    Node::Svg {
        id: Some(gen_id(rng)),
        source: SvgSource::Data {
            hash: rng.next_u64(),
            bytes,
        },
        transformation: SvgTransformation {
            scale: [gen_f32(rng), gen_f32(rng)],
            translate: [gen_f32(rng), gen_f32(rng)],
            rotate: gen_f32(rng),
        },
        label: rng.next_bool().then(|| gen_string(rng)),
        style: gen_native_style(rng),
        interactivity: Interactivity {
            hover: Some(gen_native_style(rng)),
            ..Default::default()
        },
    }
}

pub(super) fn gen_toggle(rng: &mut Rng) -> Node {
    Node::Toggle {
        id: gen_id(rng),
        kind: *rng.choose(&[ToggleKind::Checkbox, ToggleKind::Switch]),
        label: gen_string(rng),
        checked: rng.next_bool(),
        on_toggle: rng.next_bool().then(|| rng.next_u64() as u32),
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_radio(rng: &mut Rng) -> Node {
    Node::Radio {
        id: gen_id(rng),
        label: gen_string(rng),
        selected: rng.next_bool(),
        on_select: rng.next_u64() as u32,
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_slider(rng: &mut Rng) -> Node {
    Node::Slider {
        id: gen_id(rng),
        label: rng.next_bool().then(|| gen_string(rng)),
        value: gen_f32(rng),
        min: gen_f32(rng),
        max: gen_f32(rng),
        step: gen_f32(rng),
        on_change: rng.next_u64() as u32,
        on_release: rng.next_bool().then(|| rng.next_u64() as u32),
        axis: gen_axis(rng),
        style: gpui::StyleRefinement::default(),
    }
}

/// A pick list whose option count crosses `MAX_OPTIONS` about one time in
/// eight, and whose selection points anywhere, including past the list.
pub(super) fn gen_pick_list(rng: &mut Rng) -> Node {
    let count = match rng.next_range(8) {
        0 => MAX_OPTIONS + 1 + rng.next_range(64),
        _ => rng.skewed(16, 2),
    };
    Node::PickList {
        settings: Default::default(),
        id: gen_id(rng),
        options: (0..count).map(|_| gen_string(rng)).collect(),
        selected: rng
            .next_bool()
            .then(|| rng.next_range(2 * MAX_OPTIONS) as u32),
        placeholder: rng.next_bool().then(|| gen_string(rng)),
        label: rng.next_bool().then(|| gen_string(rng)),
        on_select: rng.next_u64() as u32,
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_progress(rng: &mut Rng) -> Node {
    Node::Progress {
        id: gen_id(rng),
        value: gen_f32(rng),
        min: gen_f32(rng),
        max: gen_f32(rng),
        axis: gen_axis(rng),
        style: gpui::StyleRefinement::default(),
    }
}

/// A leaf with no children, for filling out a wide container: every leaf
/// variant except `Space` carries a string, a colour or a number worth
/// pulling into range.
pub(super) fn gen_surface(rng: &mut Rng) -> Node {
    Node::Surface {
        id: gen_id(rng),
        name: gen_string(rng),
        args: vec![
            view_wire::SurfaceValue::Str(gen_string(rng)),
            view_wire::SurfaceValue::F64(f64::NAN),
        ],
        on_event: Some(0),
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_leaf(rng: &mut Rng) -> Node {
    match rng.next_range(12) {
        0 => gen_text(rng),
        10 => gen_svg(rng),
        11 => gen_editor(rng),
        1 => Node::Space {
            style: gen_native_style(rng),
        },
        2 => gen_input(rng),
        3 => gen_rule(rng),
        4 => gen_toggle(rng),
        5 => gen_radio(rng),
        6 => gen_slider(rng),
        7 => gen_pick_list(rng),
        8 => gen_progress(rng),
        9 => gen_surface(rng),
        _ => gen_button_label(rng),
    }
}

/// Builds one random tree of exactly `depth` levels of nesting with `width`
/// extra siblings injected at one random level, entirely with an
/// iterative loop rather than recursion — the wire's own stress test
/// (`deep_chain_bytes` in `lib.rs`) builds a deep chain the same way,
/// because a recursive builder would blow its own stack before `decode`
/// ever got a chance to refuse anything.
/// A current wire node holding a child list, around `children`. One in
/// eight carries a hostile base style and every conditional refinement, so
/// the bounds are exercised on the node kind views style most.
pub(super) fn gen_container(rng: &mut Rng, children: Vec<Node>) -> Node {
    let styled = rng.next_range(8) == 0;
    let refinement = |rng: &mut Rng| styled.then(|| gen_native_style(rng));
    let group = |rng: &mut Rng| {
        refinement(rng).map(|style| GroupRefinement {
            group: "row".into(),
            style,
        })
    };
    Node::Container(view_wire::ContainerNode {
        id: None,
        style: refinement(rng).unwrap_or_default(),
        interactivity: Interactivity {
            hover: refinement(rng),
            active: refinement(rng),
            group_hover: group(rng),
            group_active: group(rng),
            ..Default::default()
        },
        children,
    })
}

pub(super) fn gen_list(rng: &mut Rng, children: Vec<Node>) -> Node {
    if rng.next_range(3) == 0 {
        return gen_container(rng, children);
    }
    match rng.next_range(5) {
        0 => {
            return gen_container(rng, children);
        }
        1 => {
            return Node::Overlay {
                id: gen_id(rng),
                label: rng.next_bool().then(|| gen_string(rng)),
                on_dismiss: Some(rng.next_u64() as u32),
                children,
                style: gpui::StyleRefinement::default(),
            };
        }
        _ => {}
    }
    gen_container(rng, children)
}
