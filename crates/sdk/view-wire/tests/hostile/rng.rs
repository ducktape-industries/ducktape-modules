use super::*;

// ---------------------------------------------------------------- splitmix64

/// A tiny deterministic PRNG so the property tests need no new dependency.
/// splitmix64: https://prng.di.unimi.it/splitmix64.c
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed)
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
        id: ElementIdWire::Name(gen_key(rng).into()),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
        style: gpui::StyleRefinement::default(),
        placeholder: gen_string(rng),
        label: rng.next_bool().then(|| gen_string(rng)),
    }
}

pub(super) fn gen_rule(rng: &mut Rng) -> Node {
    Node::Rule {
        id: ElementIdWire::Name(gen_key(rng).into()),
        axis: gen_axis(rng),
        style: gen_native_style(rng),
    }
}

pub(super) fn gen_text(rng: &mut Rng) -> Node {
    Node::Text(view_wire::TextNode {
        id: None,
        style: gpui::StyleRefinement::default(),
        content: gen_string(rng),
        // 0 and 7 are outside 1..=6, for the sanitizer to drop.
        heading: rng.next_bool().then(|| rng.next_range(8) as u8),
        live: rng
            .next_bool()
            .then(|| *rng.choose(&[Live::Polite, Live::Assertive])),
    })
}

/// A picture whose bytes cross about half the time, and about one time in
/// sixteen run past `MAX_PICTURE_BYTES_PER_FRAME` on their own.
pub(super) fn gen_native_style(rng: &mut Rng) -> gpui::StyleRefinement {
    use gpui::Styled;
    gpui::StyleRefinement::default()
        .w(gpui::px(gen_f32(rng)))
        .h(gpui::px(gen_f32(rng)))
        .opacity(gen_f32(rng))
        .text_color(gpui::Hsla {
            h: gen_f32(rng),
            s: gen_f32(rng),
            l: gen_f32(rng),
            a: gen_f32(rng),
        })
}

pub(super) fn check_native_style(style: &gpui::StyleRefinement) {
    for length in [&style.size.width, &style.size.height]
        .into_iter()
        .flatten()
    {
        if let gpui::Length::Definite(gpui::DefiniteLength::Absolute(
            gpui::AbsoluteLength::Pixels(value),
        )) = length
        {
            let value = f32::from(*value);
            assert!(value.is_finite() && (0.0..=PIXEL_BOUND).contains(&value));
        }
    }
    if let Some(opacity) = style.opacity {
        assert!(opacity.is_finite() && (0.0..=1.0).contains(&opacity));
    }
    if let Some(color) = style.text.color {
        for value in [color.h, color.s, color.l, color.a] {
            assert!(value.is_finite() && (0.0..=1.0).contains(&value));
        }
    }
}

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
                id: ElementIdWire::Name(gen_key(rng).into()),
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
            id: Some(ElementIdWire::Name(gen_key(rng).into())),
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
        id: Some(ElementIdWire::Name(gen_key(rng).into())),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
        kind: *rng.choose(&[ToggleKind::Checkbox, ToggleKind::Switch]),
        label: gen_string(rng),
        checked: rng.next_bool(),
        on_toggle: rng.next_bool().then(|| rng.next_u64() as u32),
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_radio(rng: &mut Rng) -> Node {
    Node::Radio {
        id: ElementIdWire::Name(gen_key(rng).into()),
        label: gen_string(rng),
        selected: rng.next_bool(),
        on_select: rng.next_u64() as u32,
        style: gpui::StyleRefinement::default(),
    }
}

pub(super) fn gen_slider(rng: &mut Rng) -> Node {
    Node::Slider {
        id: ElementIdWire::Name(gen_key(rng).into()),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
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
        id: ElementIdWire::Name(gen_key(rng).into()),
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
/// A current wire node holding a child list, around `children`.
pub(super) fn gen_container(_rng: &mut Rng, children: Vec<Node>) -> Node {
    Node::Container(view_wire::ContainerNode {
        id: None,
        style: gpui::StyleRefinement::default(),
        interactivity: Interactivity::default(),
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
                id: ElementIdWire::Name(gen_key(rng).into()),
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
