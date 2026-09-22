//! Helpers for a guest's own tests: build events the host would send, and
//! read the tree a frame carries.

use crate::wire::{ButtonContent, Event, Frame, Node};

/// Every text the tree shows, depth first: text nodes, button labels, and
/// the value or placeholder of an input or editor.
pub(crate) fn texts(frame: &Frame) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = &frame.root {
        collect_texts(root, &mut out);
    }
    out
}

fn collect_texts(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Container(crate::wire::ContainerNode { children, .. }) => {
            children.iter().for_each(|child| collect_texts(child, out))
        }
        Node::Sensor { child: content, .. }
        | Node::Float { content, .. }
        | Node::Deferred { content, .. }
        | Node::Responsive { content, .. }
        | Node::Lazy { content, .. }
        | Node::ResizeHandle { content, .. }
        | Node::MouseArea { content, .. }
        | Node::Scroll { content, .. } => collect_texts(content, out),
        Node::Tooltip { children, .. }
        | Node::Overlay { children, .. }
        | Node::UniformList { children, .. }
        | Node::Anchored { children, .. }
        | Node::List { children, .. }
        | Node::When { children, .. } => {
            children.iter().for_each(|child| collect_texts(child, out))
        }
        Node::RichText { text, .. } => out.push(text.clone()),
        Node::Text(crate::wire::TextNode { content, .. }) => out.push(content.clone()),
        Node::Input {
            value: text,
            placeholder,
            ..
        } => out.push(if text.is_empty() {
            placeholder.clone()
        } else {
            text.clone()
        }),
        // Editor text belongs to its document transfer, not the display tree.
        Node::Editor {
            document,
            placeholder,
            ..
        } => {
            if document.byte_len == 0 {
                out.push(placeholder.clone());
            }
        }
        Node::Button { content, .. } => match content {
            ButtonContent::Label(label) => out.push(label.clone()),
            ButtonContent::Child(child) => collect_texts(child, out),
        },
        Node::Toggle { label, .. } | Node::Radio { label, .. } => out.push(label.clone()),
        Node::ComboBox {
            options,
            selected,
            placeholder,
            ..
        } => out.push(
            selected
                .and_then(|index| options.get(index as usize))
                .cloned()
                .unwrap_or_else(|| placeholder.clone()),
        ),
        Node::PickList {
            options,
            selected,
            placeholder,
            ..
        } => out.push(match selected {
            Some(index) => options[*index as usize].clone(),
            None => placeholder.clone().unwrap_or_default(),
        }),
        Node::Space { .. }
        | Node::Rule { .. }
        | Node::Qr { .. }
        | Node::Svg { .. }
        | Node::Image { .. }
        | Node::ImageViewer { .. }
        | Node::Slider { .. }
        | Node::Progress { .. }
        | Node::Canvas { .. }
        | Node::Surface { .. } => {}
    }
}

/// Panics listing each node assistive technology cannot name or place, by
/// its key path and fault. `ops/build-views.sh` runs every test whose name
/// holds `accessibility` before it builds a component.
pub(crate) fn assert_accessible(tree: &Node) {
    let faults = crate::wire::accessibility_faults(tree);
    assert!(
        faults.is_empty(),
        "{} accessibility fault(s):\n{}",
        faults.len(),
        faults
            .iter()
            .map(|fault| format!("  {:?} at {}", fault.kind, fault.path.join(" > ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

pub(crate) fn has_text(frame: &Frame, content: &str) -> bool {
    texts(frame).iter().any(|text| text == content)
}

pub(crate) fn rich_click(frame: &Frame, key: &str, index: usize) -> Event {
    let Some(Node::RichText {
        on_click: Some(handler),
        clickable_ranges,
        ..
    }) = find(frame, key)
    else {
        panic!("{key} is not interactive rich text");
    };
    assert!(
        index < clickable_ranges.len(),
        "rich text click index out of bounds"
    );
    Event::Select {
        handler: *handler,
        index: u32::try_from(index).expect("rich text click index fits the wire"),
    }
}

/// The node under `key` (`App/content/count`), if the tree has one.
pub(crate) fn find<'a>(frame: &'a Frame, key: &str) -> Option<&'a Node> {
    let root = frame.root.as_ref()?;
    find_by(root, &|node| node.key() == Some(key))
}

/// The first node `matches` accepts, depth first.
fn find_by<'a>(node: &'a Node, matches: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if matches(node) {
        return Some(node);
    }
    match node {
        Node::Container(crate::wire::ContainerNode { children, .. }) => {
            children.iter().find_map(|child| find_by(child, matches))
        }
        Node::Sensor { child: content, .. }
        | Node::Float { content, .. }
        | Node::Deferred { content, .. }
        | Node::Responsive { content, .. }
        | Node::Lazy { content, .. }
        | Node::ResizeHandle { content, .. }
        | Node::MouseArea { content, .. }
        | Node::Scroll { content, .. } => find_by(content, matches),
        Node::Tooltip { children, .. }
        | Node::Overlay { children, .. }
        | Node::UniformList { children, .. }
        | Node::Anchored { children, .. }
        | Node::List { children, .. }
        | Node::When { children, .. } => children.iter().find_map(|child| find_by(child, matches)),

        Node::Button {
            content: ButtonContent::Child(child),
            ..
        } => find_by(child, matches),
        Node::Button { .. }
        | Node::RichText { .. }
        | Node::Text(crate::wire::TextNode { .. })
        | Node::Qr { .. }
        | Node::Svg { .. }
        | Node::Image { .. }
        | Node::ImageViewer { .. }
        | Node::Input { .. }
        | Node::Editor { .. }
        | Node::Space { .. }
        | Node::Rule { .. }
        | Node::Toggle { .. }
        | Node::Radio { .. }
        | Node::Slider { .. }
        | Node::PickList { .. }
        | Node::ComboBox { .. }
        | Node::Progress { .. }
        | Node::Canvas { .. }
        | Node::Surface { .. } => None,
    }
}

/// The button whose key, label or accessible name is `name`.
fn button<'a>(frame: &'a Frame, name: &str) -> Option<&'a Node> {
    let root = frame.root.as_ref()?;
    find_by(root, &|node| match node {
        Node::Container(crate::wire::ContainerNode { interactivity, .. })
            if interactivity.on_click.is_some() =>
        {
            let mut labels = Vec::new();
            collect_texts(node, &mut labels);
            node.key() == Some(name)
                || interactivity.aria.label.as_deref() == Some(name)
                || labels.iter().any(|label| label == name)
        }
        Node::Button {
            id, content, label, ..
        } => {
            id.name() == Some(name)
                || label.as_deref() == Some(name)
                || matches!(content, ButtonContent::Label(label) if label == name)
        }
        _ => false,
    })
}

/// The input whose key or placeholder is `name`.
fn input<'a>(frame: &'a Frame, name: &str) -> Option<&'a Node> {
    let root = frame.root.as_ref()?;
    find_by(root, &|node| match node {
        Node::Input {
            id, placeholder, ..
        } => id.name() == Some(name) || placeholder == name,
        _ => false,
    })
}

/// The events the host sends when the user presses the button with key or
/// label `name`.
pub(crate) fn press(frame: &Frame, name: &str) -> Vec<Event> {
    match button(frame, name) {
        Some(Node::Container(crate::wire::ContainerNode { interactivity, .. })) => {
            vec![Event::Click {
                handler: interactivity.on_click.expect("click route"),
                event: (&gpui::ClickEvent::default()).into(),
            }]
        }
        Some(Node::Button {
            on_press: Some(message),
            ..
        }) => vec![Event::Message(*message)],
        Some(Node::Button { on_press: None, .. }) => panic!("button {name:?} is disabled"),
        _ => panic!("no button {name:?} in {:?}", texts(frame)),
    }
}

/// The events the host sends when the input with key or placeholder `name`
/// now reads `text`.
pub(crate) fn type_into(frame: &Frame, name: &str, text: &str) -> Vec<Event> {
    let Some(Node::Input { on_input, .. }) = input(frame, name) else {
        panic!("no input {name:?} in {:?}", texts(frame));
    };
    vec![Event::Input {
        handler: *on_input,
        text: text.to_string(),
    }]
}

/// The events the host sends when the user submits the input with key or
/// placeholder `name`.
pub(crate) fn submit(frame: &Frame, name: &str) -> Vec<Event> {
    let Some(Node::Input { on_submit, .. }) = input(frame, name) else {
        panic!("no input {name:?} in {:?}", texts(frame));
    };
    let Some(message) = on_submit else {
        panic!("input {name:?} has no submit route");
    };
    vec![Event::Message(*message)]
}

/// The events the host sends when the sensor with key `name` measures its
/// child at `width` by `height`: a first measurement is a show, so the
/// show route hears it, and a sensor with only a resize route hears it
/// there.
pub(crate) fn measure(frame: &Frame, name: &str, width: f32, height: f32) -> Vec<Event> {
    let Some(Node::Sensor {
        on_show, on_resize, ..
    }) = find(frame, name)
    else {
        panic!("no sensor {name:?} in {:?}", keys(frame));
    };
    let Some(handler) = on_show.or(*on_resize) else {
        panic!("sensor {name:?} has no size route");
    };
    vec![Event::Size {
        handler,
        width,
        height,
    }]
}

/// The event the host sends while the named resize handle is grabbed.
pub(crate) fn drag(frame: &Frame, name: &str, dx: f64, dy: f64) -> Vec<Event> {
    let Some(Node::ResizeHandle { on_drag, .. }) = find(frame, name) else {
        panic!("no resize handle {name:?} in {:?}", keys(frame));
    };
    let Some(handler) = on_drag else {
        panic!("resize handle {name:?} has no drag route");
    };
    vec![Event::Drag {
        handler: *handler,
        dx,
        dy,
    }]
}

/// The event the host sends when the modal backdrop dismisses an overlay.
pub(crate) fn dismiss(frame: &Frame, name: &str) -> Vec<Event> {
    let Some(Node::Overlay { on_dismiss, .. }) = find(frame, name) else {
        panic!("no overlay {name:?} in {:?}", keys(frame));
    };
    let Some(message) = on_dismiss else {
        panic!("overlay {name:?} has no dismiss route");
    };
    vec![Event::Message(*message)]
}

/// The event a named host-painted surface returns to its guest listener.
pub(crate) fn surface(frame: &Frame, name: &str, value: crate::wire::SurfaceValue) -> Vec<Event> {
    let Some(Node::Surface { on_event, .. }) = find(frame, name) else {
        panic!("no surface {name:?} in {:?}", keys(frame));
    };
    let Some(handler) = on_event else {
        panic!("surface {name:?} has no event route");
    };
    vec![Event::Surface {
        handler: *handler,
        value,
    }]
}

/// Every node key in the tree, depth first.
pub(crate) fn keys(frame: &Frame) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = &frame.root {
        collect_keys(root, &mut out);
    }
    out
}

fn collect_keys(node: &Node, out: &mut Vec<String>) {
    if let Some(key) = node.key() {
        out.push(key.to_string());
    }
    match node {
        Node::Container(crate::wire::ContainerNode { children, .. }) => {
            children.iter().for_each(|child| collect_keys(child, out))
        }
        Node::Sensor { child: content, .. }
        | Node::Float { content, .. }
        | Node::Deferred { content, .. }
        | Node::Responsive { content, .. }
        | Node::Lazy { content, .. }
        | Node::ResizeHandle { content, .. }
        | Node::MouseArea { content, .. }
        | Node::Scroll { content, .. } => collect_keys(content, out),
        Node::Tooltip { children, .. }
        | Node::Overlay { children, .. }
        | Node::UniformList { children, .. }
        | Node::Anchored { children, .. }
        | Node::List { children, .. }
        | Node::When { children, .. } => children.iter().for_each(|child| collect_keys(child, out)),

        Node::Button {
            content: ButtonContent::Child(child),
            ..
        } => collect_keys(child, out),
        Node::Button { .. }
        | Node::RichText { .. }
        | Node::Text(crate::wire::TextNode { .. })
        | Node::Qr { .. }
        | Node::Svg { .. }
        | Node::Image { .. }
        | Node::ImageViewer { .. }
        | Node::Input { .. }
        | Node::Editor { .. }
        | Node::Space { .. }
        | Node::Rule { .. }
        | Node::Toggle { .. }
        | Node::Radio { .. }
        | Node::Slider { .. }
        | Node::PickList { .. }
        | Node::ComboBox { .. }
        | Node::Progress { .. }
        | Node::Canvas { .. }
        | Node::Surface { .. } => {}
    }
}

mod context;
mod fake_host;
pub use context::TestAppContext;
pub use fake_host::{FakeHost, Feed};
