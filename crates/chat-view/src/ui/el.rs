//! A fluent builder over `wire::Node`, the way GPUI styles a `div()`: the
//! node is built once, then each call sets one property. Controls take their
//! handler id from `Cx::on`; a `None` handler is a disabled control.
use ducktape_view_guest::wire::{self, AlignX, AlignY, ButtonPreset, Length, Node, kit};

pub struct El(pub Node);

impl El {
    pub fn row(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self(kit::row(key, children))
    }
    pub fn centered_row(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self(kit::centered_row(key, children))
    }
    pub fn column(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self(kit::column(key, children))
    }
    pub fn container(key: impl Into<String>, child: Node) -> Self {
        Self(kit::container(key, child))
    }
    pub fn mouse_area(key: impl Into<String>, content: Node) -> Self {
        Self(Node::MouseArea {
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
        })
    }

    pub fn node(self) -> Node {
        self.0
    }

    pub fn w(self, width: Length) -> Self {
        Self(kit::sized(self.0, Some(width), None))
    }
    pub fn h(self, height: Length) -> Self {
        Self(kit::sized(self.0, None, Some(height)))
    }
    pub fn fill_w(self) -> Self {
        self.w(Length::Fill)
    }
    pub fn fill(self) -> Self {
        self.w(Length::Fill).h(Length::Fill)
    }
    pub fn gap(self, gap: f32) -> Self {
        Self(kit::spaced(self.0, gap))
    }
    pub fn pad(self, edges: wire::Edges) -> Self {
        Self(kit::padded(self.0, edges))
    }
    pub fn pad_all(self, all: f32) -> Self {
        self.pad(wire::Edges::all(all))
    }
    pub fn pad_xy(self, x: f32, y: f32) -> Self {
        self.pad(wire::Edges {
            top: y,
            right: x,
            bottom: y,
            left: x,
        })
    }
    pub fn align_x(mut self, align: AlignX) -> Self {
        match &mut self.0 {
            Node::Container { align_x, .. } => *align_x = Some(align),
            Node::Linear { .. } => self.0 = kit::aligned(self.0, align),
            _ => {}
        }
        self
    }
    pub fn align_y(mut self, align: AlignY) -> Self {
        if let Node::Container { align_y, .. } = &mut self.0 {
            *align_y = Some(align);
        }
        self
    }
    pub fn clip(mut self) -> Self {
        if let Node::Container { clip, .. } = &mut self.0 {
            *clip = true;
        }
        self
    }
    pub fn bg(mut self, color: design::Color) -> Self {
        let paint = kit::rgba(color);
        match &mut self.0 {
            Node::Container { background, .. } => {
                *background = Some(wire::Background::Color(paint))
            }
            Node::Linear { background, .. } => *background = Some(paint),
            _ => {}
        }
        self
    }
    pub fn border(mut self, color: Option<design::Color>, width: Option<f32>, radius: f32) -> Self {
        let border = wire::Border {
            color: color.map(kit::rgba),
            width,
            radius: Some([radius; 4]),
        };
        match &mut self.0 {
            Node::Container { border: slot, .. } | Node::Linear { border: slot, .. } => {
                *slot = Some(border)
            }
            _ => {}
        }
        self
    }
    pub fn rounded(self, radius: f32) -> Self {
        self.border(None, None, radius)
    }
    pub fn on_press_at(mut self, handler: u32) -> Self {
        if let Node::MouseArea { on_press_at, .. } = &mut self.0 {
            *on_press_at = Some(handler);
        }
        self
    }
    pub fn on_right_press(mut self, handler: u32) -> Self {
        if let Node::MouseArea { on_right_press, .. } = &mut self.0 {
            *on_right_press = Some(handler);
        }
        self
    }
    pub fn on_press(mut self, handler: u32) -> Self {
        if let Node::MouseArea { on_press, .. } = &mut self.0 {
            *on_press = Some(handler);
        }
        self
    }
    /// A row the screen reader announces, selected while chosen.
    pub fn row_role(mut self, label: String, selected: bool) -> Self {
        if let Node::MouseArea {
            role,
            label: slot,
            selected: chosen,
            ..
        } = &mut self.0
        {
            *role = Some(wire::Role::Row);
            *slot = Some(label);
            *chosen = Some(selected);
        }
        self
    }
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
    let mut children = vec![
        El(kit::nowrap(kit::label(format!("{key}/label"), name)))
            .fill_w()
            .node(),
    ];
    children.extend(control);
    El::centered_row(key, children)
        .gap(kit::spacing::XXS as f32)
        .fill_w()
        .h(Length::Fixed(kit::height::CONTROL as f32))
        .pad(wire::Edges {
            top: 0.,
            right: kit::spacing::XXS as f32,
            bottom: 0.,
            left: kit::spacing::SM as f32,
        })
        .node()
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
    El::container(
        key,
        kit::space(Some(Length::Fixed(8.)), Some(Length::Fixed(8.))),
    )
    .bg(kit::palette().accent)
    .rounded(kit::radius::PILL as f32)
    .w(Length::Shrink)
    .node()
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
