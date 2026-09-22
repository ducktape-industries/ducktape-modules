use crate::*;

/// A tree deeper than this is cut off: a guest cannot make the host's
/// layout recurse without bound.
pub const MAX_DEPTH: usize = 64;
/// More nodes than this and the host stops reading: the widget tree of a
/// screen, not of a spreadsheet.
pub const MAX_NODES: usize = 8_192;
/// The longest string a single node may carry (text, placeholder, key).
pub const MAX_STRING_BYTES: usize = 64 << 10;
/// The most SHAPED text one frame may carry in total — every [`Node::Text`]
/// content, input or editor value and placeholder, and plain button label
/// together.
///
/// The per-string and per-node caps do not bound this: 128 strings of
/// [`MAX_STRING_BYTES`] are a legal 8 MiB frame, and the host reshapes all
/// of it on the window thread every time the guest changes a character.
/// Measured on a desk machine, that frame took 6.2 s and allocated 3.3 GiB;
/// held to one full-length string's worth of text it takes 69 ms and 26 MiB.
/// A screen shows a couple of kilobytes, so this leaves a guest that means
/// well thirty times what it needs while taking two orders of magnitude off
/// what a hostile one can spend.
///
/// It bounds bytes and nothing else. Two other costs live outside it and are
/// the host's to attack: [`MAX_NODES`] nodes cost around a hundred
/// milliseconds to lay out however short their text, and a byte of Hangul,
/// Han or emoji costs some twenty times a byte of ASCII to shape.
pub const MAX_TEXT_BYTES_PER_FRAME: usize = MAX_STRING_BYTES;
/// The most picture bytes one frame may carry in total, over every
/// [`Node::Svg`] or [`Node::Image`] that brings its payload. A picture that does not fit in
/// what is left is dropped whole, not cut: half an SVG is not an SVG, and
/// the host draws an unknown hash as empty space. A guest sends each
/// picture once, so this bounds what a frame can make the host parse, not
/// what an app can show over its life; an icon is a few kilobytes.
pub const MAX_PICTURE_BYTES_PER_FRAME: usize = 1 << 20;
/// The most options one [`Node::PickList`] may offer: a menu, not a table.
/// Each option is shaped text and spends the frame's text budget too.
pub const MAX_OPTIONS: usize = 256;

/// A uniform list may describe a large logical list without allocating rows.
pub const MAX_UNIFORM_LIST_COUNT: usize = 65_536;
/// A frame and one host range request carry at most this many uniform rows.
pub const MAX_UNIFORM_LIST_ROWS: usize = 256;

/// Maximum positional values supplied to one host surface.
pub const MAX_SURFACE_ARGS: usize = 256;
/// Text and spacing sizes are pixels; nothing on a screen needs more.
pub(crate) const MAX_PIXELS: f32 = 8192.0;
/// A text size, which is not a length: every glyph at it is rasterized and
/// cached, so a screenful of 8192 px text is an atlas no screen asked for.
pub(crate) const MAX_TEXT_PIXELS: f32 = 512.0;

/// Pulls a frame from an untrusted module into what the host is willing to
/// lay out: the tree is truncated past [`MAX_DEPTH`] and [`MAX_NODES`],
/// strings past [`MAX_STRING_BYTES`], shaped text past
/// [`MAX_TEXT_BYTES_PER_FRAME`] in total, picture bytes past
/// [`MAX_PICTURE_BYTES_PER_FRAME`] in total, text sizes to [`MAX_TEXT_PIXELS`],
/// every other size, colour and spacing clamped to a finite range, and typed
/// identity collisions refused. A frame from a well-behaved guest passes
/// through unchanged.
///
/// A frame that arrived as bytes has passed [`decode`] first, which refuses
/// one nested deeper than this walk goes. A frame that carries `patches`
/// instead of a tree is bounded by [`apply`], since every bound is on the
/// tree the patches make and only the host holds it.
pub fn sanitize(frame: &mut Frame) -> Result<SanitizeReport, &'static str> {
    let mut budgets = Budgets::frame();
    let mut report = if let Some(root) = &mut frame.root {
        sanitize_tree_with(root, &mut budgets)?
    } else {
        SanitizeReport::default()
    };
    frame.tooltip_responses.truncate(MAX_PATCHES);
    let mut responses = Vec::with_capacity(frame.tooltip_responses.len());
    for mut response in frame.tooltip_responses.drain(..) {
        if let Some(content) = &mut response.content {
            if budgets.nodes == 0 {
                continue;
            }
            report.merge(sanitize_tree_with(content, &mut budgets)?);
        }
        responses.push(response);
    }
    frame.tooltip_responses = responses;
    frame.upstream_sanitization.merge(report);
    for request in &mut frame.requests {
        truncate_string(&mut request.kind);
    }
    Ok(report)
}

// Sanitization may shorten display text, but never an authoritative document.
pub(crate) fn text_amounts(root: &Node) -> Result<(usize, usize), &'static str> {
    let mut pending = vec![root];
    let mut surface_values = Vec::new();
    let mut references = Vec::new();
    let mut display = 0usize;
    while let Some(node) = pending.pop() {
        let mut add = |text: &str| display = display.saturating_add(text.len());
        match node {
            Node::Text(crate::TextNode { content, .. }) => add(content),
            Node::RichText { text, .. } => add(text),
            Node::Input {
                value,
                placeholder,
                options,
                ..
            } => {
                add(value);
                add(placeholder);
                add(&options.label);
                if let Some(description) = &options.description {
                    add(description);
                }
            }
            Node::Editor {
                placeholder,
                label,
                options,
                ..
            } => {
                add(placeholder);
                if let Some(label) = label {
                    add(label);
                }
                if let Some(rich) = &options.rich {
                    for item in &rich.toolbar {
                        add(&item.label);
                    }
                }
            }
            Node::Image { label, .. }
            | Node::ImageViewer { label, .. }
            | Node::Svg { label, .. }
            | Node::MouseArea { label, .. }
            | Node::Slider { label, .. }
            | Node::Overlay { label, .. } => {
                if let Some(label) = label {
                    add(label);
                }
            }
            // Unknown surfaces display their name in the native placeholder.
            Node::Surface { name, args, .. } => {
                add(name);
                surface_values.extend(args);
            }
            Node::Button {
                content,
                label,
                description,
                ..
            } => {
                if let ButtonContent::Label(text) = content {
                    add(text);
                }
                if let Some(label) = label {
                    add(label);
                }
                if let Some(description) = description {
                    add(description);
                }
            }
            Node::Toggle { label, .. } | Node::Radio { label, .. } => add(label),
            Node::ComboBox {
                options,
                placeholder,
                label,
                ..
            } => {
                for option in options {
                    add(option);
                }
                add(placeholder);
                if let Some(label) = label {
                    add(label);
                }
            }
            Node::PickList {
                options,
                placeholder,
                label,
                ..
            } => {
                for option in options {
                    add(option);
                }
                if let Some(placeholder) = placeholder {
                    add(placeholder);
                }
                if let Some(label) = label {
                    add(label);
                }
            }
            _ => {}
        }
        if let Node::Editor {
            document, options, ..
        } = node
        {
            if let Some(rich) = &options.rich {
                rich.document.validate()?;
                if rich.toolbar.len() > editor_presentation::MAX_EDITOR_MENU_ITEMS {
                    return Err("rich toolbar limit");
                }
            }
            references.push(document);
        }
        pending.extend(node.children());
    }
    // Surface strings share the display budget (for example a code preview).
    // Record/type names are routing metadata, not the textual payload itself.
    while let Some(value) = surface_values.pop() {
        match value {
            SurfaceValue::Str(text) => display = display.saturating_add(text.len()),
            SurfaceValue::List(items) => surface_values.extend(items),
            SurfaceValue::Option(Some(item)) => surface_values.push(item.as_ref()),
            SurfaceValue::Record { fields, .. } => {
                surface_values.extend(fields.iter().map(|(_, value)| value));
            }
            SurfaceValue::Unit
            | SurfaceValue::Bool(_)
            | SurfaceValue::I64(_)
            | SurfaceValue::F64(_)
            | SurfaceValue::Option(None) => {}
        }
    }
    editor_document::validate_editor_document_refs(references.iter().copied())
        .map_err(|_| "invalid editor document references or budget")?;
    Ok((references.len(), display))
}

pub(crate) fn sanitize_tree(root: &mut Node) -> Result<SanitizeReport, &'static str> {
    sanitize_tree_with(root, &mut Budgets::frame())
}

fn sanitize_tree_with(
    root: &mut Node,
    budgets: &mut Budgets,
) -> Result<SanitizeReport, &'static str> {
    let (documents, before) = text_amounts(root)?;
    let mut identity_scopes = vec![std::collections::HashSet::new()];
    let mut authored_path = Vec::new();
    sanitize_node(root, 0, budgets, &mut identity_scopes, &mut authored_path)?;

    let (after_documents, after) = text_amounts(root)?;
    if after_documents != documents {
        return Err("frame budget would remove an editor document projection");
    }
    Ok(SanitizeReport {
        display_text_truncated: after < before,
    })
}

/// Typed IDs are unique among the children of the first containing element
/// with an ID. An id-less wrapper is transparent to that GPUI scope; an
/// identified node starts a fresh scope for its descendants.
type IdentityScopes = Vec<std::collections::HashSet<IdentityKey>>;

fn claim_typed_scope(node: &Node, scopes: &mut IdentityScopes) -> Result<bool, &'static str> {
    let Some(IdentityKeyRef::Element(id)) = node.identity() else {
        return Ok(false);
    };
    let scope = scopes
        .last_mut()
        .expect("the root identity scope is always present");
    if !scope.insert(IdentityKey::Element(id.clone())) {
        return Err("duplicate typed element identity among siblings");
    }
    scopes.push(std::collections::HashSet::new());
    Ok(true)
}

fn finish_typed_scope(scopes: &mut IdentityScopes, started: bool) {
    if started {
        scopes.pop();
    }
}

/// What is left of a frame's per-frame budgets while its tree is walked.
pub(crate) struct Budgets {
    pub(crate) nodes: usize,
    pub(crate) qr_codes: usize,
    pub(crate) canvas_parts: usize,
    pub(crate) surface_values: usize,
    pub(crate) text: usize,
    pub(crate) pictures: usize,
    pub(crate) list_items: usize,
}

impl Budgets {
    pub(crate) fn frame() -> Self {
        Self {
            nodes: MAX_NODES,
            text: MAX_TEXT_BYTES_PER_FRAME,
            pictures: MAX_PICTURE_BYTES_PER_FRAME,
            list_items: MAX_LIST_ITEMS,
            surface_values: MAX_SURFACE_VALUES,
            canvas_parts: MAX_CANVAS_PARTS,
            qr_codes: MAX_QR_CODES,
        }
    }
}

/// Truncates one shaped string to what is left of the frame's text budget
/// and spends what survives. Nodes are walked in tree order, so a frame past
/// the budget keeps its head and loses its tail.
pub(crate) fn spend_text(text: &mut String, budgets: &mut Budgets) {
    truncate_to(text, budgets.text.min(MAX_STRING_BYTES));
    budgets.text -= text.len();
}

/// Spends a picture's bytes from the frame's picture budget, or drops them
/// whole when they do not fit.
fn spend_svg(bytes: &mut Option<Vec<u8>>, budgets: &mut Budgets) {
    match bytes {
        Some(picture) if picture.len() <= budgets.pictures => budgets.pictures -= picture.len(),
        _ => *bytes = None,
    }
}

mod node;
use node::sanitize_node;

mod numbers;
pub use numbers::truncate_string;
use numbers::truncate_to;
pub(crate) use numbers::{bound_optional, bounded, finite, signed_bounded};

mod interactivity;
use interactivity::sanitize_interactivity;
