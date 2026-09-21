//! The controls and the style helpers the panes share: buttons in their
//! presets, fields, section rows, and the small mutations the kit lacks
//! (a background, a border, an alignment on a container, a mouse area).
use ducktape_view_guest::wire::{self, AlignX, AlignY, ButtonPreset, Length, Node, kit};

pub fn mouse_area(key: impl Into<String>, content: Node) -> Node {
    Node::MouseArea {
        key: key.into(),
        role: None,
        label: None,
        expanded: None,
        selected: None,
        checked: None,
        on_press: None,
        on_release: None,
        on_double_click: None,
        on_right_press: None,
        on_right_release: None,
        on_middle_press: None,
        on_middle_release: None,
        on_enter: None,
        on_exit: None,
        on_move: None,
        on_press_at: None,
        on_scroll: None,
        content: Box::new(content),
    }
}

pub fn width(node: Node, width: Length) -> Node {
    kit::sized(node, Some(width), None)
}
pub fn height(node: Node, height: Length) -> Node {
    kit::sized(node, None, Some(height))
}
pub fn fill_width(node: Node) -> Node {
    kit::sized(node, Some(Length::Fill), None)
}
pub fn fill(node: Node) -> Node {
    kit::sized(node, Some(Length::Fill), Some(Length::Fill))
}
pub fn padded_all(node: Node, all: f32) -> Node {
    kit::padded(node, wire::Edges::all(all))
}
pub fn padded_xy(node: Node, x: f32, y: f32) -> Node {
    kit::padded(
        node,
        wire::Edges {
            top: y,
            right: x,
            bottom: y,
            left: x,
        },
    )
}
pub fn aligned_x(mut node: Node, align: AlignX) -> Node {
    match &mut node {
        Node::Container { align_x, .. } => *align_x = Some(align),
        Node::Linear { .. } => return kit::aligned(node, align),
        _ => {}
    }
    node
}
pub fn aligned_y(mut node: Node, align: AlignY) -> Node {
    if let Node::Container { align_y, .. } = &mut node {
        *align_y = Some(align);
    }
    node
}
pub fn clipped(mut node: Node) -> Node {
    if let Node::Container { clip, .. } = &mut node {
        *clip = true;
    }
    node
}
pub fn background(mut node: Node, color: design::Color) -> Node {
    let paint = kit::rgba(color);
    match &mut node {
        Node::Container { background, .. } => *background = Some(wire::Background::Color(paint)),
        Node::Linear { background, .. } => *background = Some(paint),
        _ => {}
    }
    node
}
pub fn bordered(
    mut node: Node,
    color: Option<design::Color>,
    width: Option<f32>,
    radius: f32,
) -> Node {
    let border = wire::Border {
        color: color.map(kit::rgba),
        width,
        radius: Some([radius; 4]),
    };
    match &mut node {
        Node::Container { border: slot, .. } | Node::Linear { border: slot, .. } => {
            *slot = Some(border)
        }
        _ => {}
    }
    node
}
pub fn rounded(node: Node, radius: f32) -> Node {
    bordered(node, None, None, radius)
}
pub fn with_press_at(mut node: Node, handler: u32) -> Node {
    if let Node::MouseArea { on_press_at, .. } = &mut node {
        *on_press_at = Some(handler);
    }
    node
}
pub fn with_right_press(mut node: Node, handler: u32) -> Node {
    if let Node::MouseArea { on_right_press, .. } = &mut node {
        *on_right_press = Some(handler);
    }
    node
}
pub fn with_press(mut node: Node, handler: u32) -> Node {
    if let Node::MouseArea { on_press, .. } = &mut node {
        *on_press = Some(handler);
    }
    node
}
/// A row the screen reader announces, selected while chosen.
pub fn with_row_role(mut node: Node, label: String, selected: bool) -> Node {
    if let Node::MouseArea {
        role,
        label: slot,
        selected: chosen,
        ..
    } = &mut node
    {
        *role = Some(wire::Role::Row);
        *slot = Some(label);
        *chosen = Some(selected);
    }
    node
}

// ---------- controls ----------

pub fn button(
    key: impl Into<String>,
    label: &str,
    on_press: Option<u32>,
    preset: ButtonPreset,
) -> Node {
    kit::button(key, label, on_press, preset)
}

pub fn action(key: impl Into<String>, label: &str, on_press: Option<u32>) -> Node {
    button(key, label, on_press, ButtonPreset::Secondary)
}

pub fn subtle(key: impl Into<String>, label: &str, on_press: Option<u32>) -> Node {
    button(key, label, on_press, ButtonPreset::Subtle)
}

pub fn primary(key: impl Into<String>, label: &str, on_press: Option<u32>) -> Node {
    button(key, label, on_press, ButtonPreset::Primary)
}

/// A compact ghost control: `glyph` is what shows; `label` is what a screen
/// reader and a test press.
pub fn glyph(key: impl Into<String>, glyph: &str, label: &str, on_press: Option<u32>) -> Node {
    let mut button = subtle(key, glyph, on_press);
    if let Node::Button {
        label: accessible,
        padding,
        ..
    } = &mut button
    {
        *accessible = Some(label.into());
        *padding = Some(wire::Edges {
            top: 2.,
            right: kit::spacing::XS as f32,
            bottom: 2.,
            left: kit::spacing::XS as f32,
        });
    }
    button
}

/// A button dead for a reason the reader can hear: the step out of it.
pub fn gated(mut button: Node, allowed: bool, why: &str) -> Node {
    if let Node::Button {
        on_press,
        description,
        ..
    } = &mut button
        && !allowed
    {
        *on_press = None;
        *description = Some(why.into());
    }
    button
}

pub fn with_label(mut button: Node, label: &str) -> Node {
    if let Node::Button { label: slot, .. } = &mut button {
        *slot = Some(label.into());
    }
    button
}

pub fn field(
    key: impl Into<String>,
    label: &str,
    value: &str,
    on_input: u32,
    on_submit: Option<u32>,
    disabled: bool,
) -> Node {
    let mut node = kit::input(key, label, value, on_input, on_submit);
    if let Node::Input { options, .. } = &mut node {
        options.label = label.into();
        options.disabled = disabled;
    }
    node
}

/// A section label over a list: a 28px row, the name quiet, and at most one
/// ghost control beside it.
pub fn section_row(key: &str, name: &str, control: Option<Node>) -> Node {
    let mut children = vec![fill_width(kit::nowrap(kit::label(
        format!("{key}/label"),
        name,
    )))];
    children.extend(control);
    kit::padded(
        height(
            fill_width(kit::spaced(
                kit::centered_row(key, children),
                kit::spacing::XXS as f32,
            )),
            Length::Fixed(kit::height::CONTROL as f32),
        ),
        wire::Edges {
            top: 0.,
            right: kit::spacing::XXS as f32,
            bottom: 0.,
            left: kit::spacing::SM as f32,
        },
    )
}

/// A room in the list pane: a 28px row, the name as its accessible label.
pub fn sidebar_row(mut button: Node, name: &str) -> Node {
    if let Node::Button {
        label,
        height,
        padding,
        ..
    } = &mut button
    {
        *label = Some(name.into());
        *height = Some(Length::Fixed(kit::height::CONTROL as f32));
        *padding = Some(wire::Edges {
            top: 0.,
            right: kit::spacing::SM as f32,
            bottom: 0.,
            left: kit::spacing::SM as f32,
        });
    }
    button
}

/// An 8px accent dot at a row's end: what marks an unread room.
pub fn unread_dot(key: String) -> Node {
    width(
        rounded(
            background(
                kit::container(
                    key,
                    kit::space(Some(Length::Fixed(8.)), Some(Length::Fixed(8.))),
                ),
                kit::palette().accent,
            ),
            kit::radius::PILL as f32,
        ),
        Length::Shrink,
    )
}

/// A text node whose line box is `height` tall: an emoji's full glyph paints
/// inside a button that clips to the line box.
pub fn tall_glyph(key: String, glyph: &str, size: f32, height: f32) -> Node {
    kit::text_options(
        kit::text_size(kit::text(key, glyph), size),
        wire::TextOptions {
            line_height: Some(wire::LineHeight::Absolute(height)),
            ..Default::default()
        },
    )
}
