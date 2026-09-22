use super::*;

// -------------------------------------------------------- bound assertions

/// The nesting depth `sanitize` would count for this node (root is 0, each
/// container-like or one-child node adds one) — the same metric
/// `decode`'s own depth budget counts, so it doubles as "was this tree
/// really over the door" evidence when `decode` refuses one.
pub(super) fn tree_depth(node: &Node) -> usize {
    match node {
        Node::Sensor { child: content, .. }
        | Node::MouseArea { content, .. }
        | Node::ResizeHandle { content, .. }
        | Node::Float { content, .. }
        | Node::Responsive { content, .. }
        | Node::Lazy { content, .. }
        | Node::Scroll { content, .. } => 1 + tree_depth(content),
        Node::Container { children, .. }
        | Node::Anchored { children, .. }
        | Node::Image {
            state_children: children,
            ..
        }
        | Node::Tooltip { children, .. }
        | Node::Overlay { children, .. }
        | Node::When { children, .. } => 1 + children.iter().map(tree_depth).max().unwrap_or(0),
        Node::Button {
            content: ButtonContent::Child(child),
            ..
        } => 1 + tree_depth(child),
        _ => 0,
    }
}

/// A slider or progress number: finite, and nothing more is promised.
pub(super) fn check_finite(value: f32, ctx: &str, field: &str) {
    assert!(value.is_finite(), "{ctx}: {field} {value} is not finite");
}

pub(super) fn check_pixels(value: &Option<f32>, ctx: &str, field: &str) {
    if let Some(value) = value {
        assert!(
            value.is_finite() && (0.0..=PIXEL_BOUND).contains(value),
            "{ctx}: {field} {value} outside 0..={PIXEL_BOUND}"
        );
    }
}

pub(super) fn check_string(text: &str, ctx: &str, field: &str) {
    assert!(
        text.len() <= MAX_STRING_BYTES,
        "{ctx}: {field} is {} bytes, over MAX_STRING_BYTES",
        text.len()
    );
    assert!(
        text.is_char_boundary(text.len()),
        "{ctx}: {field} does not end on a char boundary"
    );
}

/// Walks a sanitized tree asserting every post-condition `sanitize_node`
/// promises: depth within `MAX_DEPTH`, every string within
/// `MAX_STRING_BYTES` and on a char boundary, every key unique across the
/// whole tree, every size/colour/border field inside its own bound, and the
/// picture bytes the tree carries summed into `svg_bytes`.
pub(super) fn check_bounds(
    node: &Node,
    depth: usize,
    keys: &mut HashSet<String>,
    svg_bytes: &mut usize,
    ctx: &str,
) {
    assert!(
        depth <= MAX_DEPTH,
        "{ctx}: a node sits at depth {depth}, over MAX_DEPTH"
    );
    match node.identity() {
        Some(IdentityKeyRef::Legacy(key)) => {
            check_string(key, ctx, "key");
            assert!(
                keys.insert(key.to_string()),
                "{ctx}: legacy key aliases another node"
            );
        }
        Some(IdentityKeyRef::Element(id)) => {
            id.validate_host()
                .expect("sanitized typed identity is portable and bounded");
        }
        None => {}
    }
    let mut sibling_ids = HashSet::new();
    for child in node.children() {
        if let Some(IdentityKeyRef::Element(id)) = child.identity() {
            assert!(
                sibling_ids.insert(id),
                "{ctx}: typed sibling identity aliases state"
            );
        }
    }
    match node {
        Node::Container { children, .. } | Node::UniformList { children, .. } => {
            for child in children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::List {
            path,
            item_count,
            overdraw,
            commands,
            range_start,
            children,
            ..
        } => {
            assert!(path.len() <= view_wire::MAX_DEPTH);
            assert!(*item_count <= view_wire::MAX_LIST_ITEMS);
            assert!(overdraw.is_finite() && (0.0..=4096.0).contains(overdraw));
            assert!(commands.len() <= view_wire::MAX_LIST_COMMANDS);
            assert!(children.len() <= view_wire::MAX_LIST_ROWS);
            assert!(*range_start <= *item_count);
            assert!(children.len() <= item_count.saturating_sub(*range_start));
            for child in children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::Float {
            scale,
            style,
            content,
            ..
        } => {
            assert!(scale.is_finite() && (f32::EPSILON..=PIXEL_BOUND).contains(scale));
            check_native_style(style);
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::Tooltip {
            delay_ms,
            style,
            children,
            ..
        } => {
            assert!(*delay_ms <= 60_000);
            check_native_style(style);
            assert!(children.len() <= 2);
            for child in children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::Sensor {
            anticipate,
            delay,
            child,
            ..
        } => {
            check_pixels(anticipate, ctx, "sensor anticipate");
            if let Some(delay) = delay {
                assert!(
                    delay.is_finite() && *delay >= 0.0,
                    "{ctx}: sensor delay {delay} is not a finite non-negative number"
                );
            }
            check_bounds(child, depth + 1, keys, svg_bytes, ctx);
        }
        Node::Scroll {
            style,
            bar_width,
            bar_margin,
            scroller_width,
            bar_spacing,
            content,
            ..
        } => {
            check_native_style(style);
            for (value, field) in [
                (bar_width, "bar width"),
                (bar_margin, "bar margin"),
                (scroller_width, "scroller width"),
                (bar_spacing, "bar spacing"),
            ] {
                check_pixels(value, ctx, field);
            }
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::MouseArea { label, content, .. } => {
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::ResizeHandle { content, .. } => {
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::Qr { code, .. } => {
            if let Some(payload) = &code.payload {
                assert!(payload.len() <= view_wire::MAX_QR_PAYLOAD_BYTES);
            }
            for color in [code.cell, code.background].into_iter().flatten() {
                for value in [color.h, color.s, color.l, color.a] {
                    assert!(value.is_finite() && (0.0..=1.0).contains(&value));
                }
            }
            if let Some(view_wire::QrSize::Cell(value) | view_wire::QrSize::Total(value)) =
                code.size
            {
                check_pixels(&Some(value), ctx, "QR size");
            }
        }
        Node::RichText {
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            ..
        } => {
            check_string(text, ctx, "rich text");
            let valid = |range: &std::ops::Range<usize>| {
                range.start <= range.end
                    && range.end <= text.len()
                    && text.is_char_boundary(range.start)
                    && text.is_char_boundary(range.end)
            };
            match runs {
                view_wire::RichTextRuns::Highlights(highlights) => {
                    assert!(highlights.iter().all(|(range, _)| valid(range)));
                }
                view_wire::RichTextRuns::Runs(runs) => {
                    assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), text.len());
                }
            }
            assert!(font_family_overrides.iter().all(|(range, _)| valid(range)));
            assert!(clickable_ranges.iter().all(valid));
        }
        Node::Text {
            content, heading, ..
        } => {
            check_string(content, ctx, "text content");
            assert!(
                heading.is_none_or(|level| (1..=6).contains(&level)),
                "{ctx}: heading level {heading:?} outside 1..=6"
            );
        }
        Node::ImageViewer {
            data,
            label,
            options,
            ..
        } => {
            if let Some(data) = data {
                *svg_bytes += data.byte_len();
                assert!(data.valid_rgba(), "{ctx}: invalid viewer RGBA");
            }
            if let Some(label) = label {
                check_string(label, ctx, "viewer label");
            }
            check_pixels(&options.padding, ctx, "viewer padding");
            if let Some((min, max)) = options.scale_bounds {
                assert!(min.is_finite() && max.is_finite() && min > 0.0 && max >= min);
            }
            if let Some(step) = options.scale_step {
                assert!(step.is_finite() && step > 0.0);
            }
        }
        Node::Image {
            data,
            label,
            style,
            state_children,
            ..
        } => {
            if let Some(data) = data {
                *svg_bytes += data.byte_len();
                assert!(data.valid_rgba(), "{ctx}: invalid RGBA");
            }
            if let Some(label) = label {
                check_string(label, ctx, "image label");
            }
            check_native_style(style);
            for child in state_children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::Svg {
            source,
            transformation,
            label,
            style,
            interactivity,
            ..
        } => {
            if let SvgSource::Data { bytes, .. } = source {
                *svg_bytes += bytes.as_ref().map_or(0, Vec::len);
            }
            if let Some(label) = label {
                check_string(label, ctx, "picture label");
            }
            check_native_style(style);
            if let Some(hover) = &interactivity.hover {
                check_native_style(hover);
            }
            for value in transformation
                .scale
                .into_iter()
                .chain(transformation.translate)
                .chain([transformation.rotate])
            {
                assert!(value.is_finite() && (-PIXEL_BOUND..=PIXEL_BOUND).contains(&value));
            }
        }
        Node::Anchored {
            position,
            offset,
            children,
            ..
        } => {
            for value in position.iter().chain(offset).flatten() {
                assert!(value.is_finite() && (-PIXEL_BOUND..=PIXEL_BOUND).contains(value));
            }
            for child in children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::Deferred { priority, content } => {
            assert!(*priority <= 16);
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::Input {
            id,
            options,
            placeholder,
            value,
            ..
        } => {
            assert!(id.validate_host().is_ok(), "{ctx}: invalid input identity");
            check_string(placeholder, ctx, "placeholder");
            check_string(value, ctx, "input value");
            check_string(&options.label, ctx, "input label");
            if let Some(value) = &options.description {
                check_string(value, ctx, "input description");
            }
        }
        Node::Button {
            content,
            label,
            description,
            ..
        } => {
            match content {
                ButtonContent::Label(text) => check_string(text, ctx, "button label"),
                ButtonContent::Child(child) => check_bounds(child, depth + 1, keys, svg_bytes, ctx),
            }
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            if let Some(description) = description {
                check_string(description, ctx, "button description");
            }
        }
        Node::Space { style } | Node::Rule { style, .. } => check_native_style(style),
        Node::Toggle { label, .. } => {
            check_string(label, ctx, "control label");
        }
        Node::Radio { label, .. } => {
            check_string(label, ctx, "control label");
        }
        Node::Slider {
            label,
            value,
            min,
            max,
            step,
            ..
        } => {
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            for (number, field) in [(value, "value"), (min, "min"), (max, "max"), (step, "step")] {
                check_finite(*number, ctx, field);
            }
        }
        Node::ComboBox {
            state_key,
            options,
            selected,
            placeholder,
            label,
            settings,
            ..
        } => {
            check_string(state_key, ctx, "combo state identity");
            check_string(placeholder, ctx, "combo placeholder");
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            assert!(options.len() <= MAX_OPTIONS, "{ctx}: combo option budget");
            for option in options {
                check_string(option, ctx, "combo option");
            }
            if let Some(index) = selected {
                assert!((*index as usize) < options.len());
            }
            check_pixels(
                &settings.icon.as_ref().map(|icon| icon.spacing),
                ctx,
                "combo icon spacing",
            );
        }
        Node::PickList {
            options,
            selected,
            placeholder,
            label,
            ..
        } => {
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            assert!(
                options.len() <= MAX_OPTIONS,
                "{ctx}: {} options, over MAX_OPTIONS",
                options.len()
            );
            for option in options {
                check_string(option, ctx, "option");
            }
            if let Some(index) = selected {
                assert!(
                    (*index as usize) < options.len(),
                    "{ctx}: selected option {index} past {} options",
                    options.len()
                );
            }
            if let Some(placeholder) = placeholder {
                check_string(placeholder, ctx, "placeholder");
            }
        }
        Node::Progress {
            value, min, max, ..
        } => {
            for (number, field) in [(value, "value"), (min, "min"), (max, "max")] {
                check_finite(*number, ctx, field);
            }
        }
        Node::Overlay {
            label, children, ..
        } => {
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            assert!(children.len() <= 2, "{ctx}: overlay child count");
            for child in children {
                check_bounds(child, depth + 1, keys, svg_bytes, ctx);
            }
        }
        Node::Lazy { content, .. } => check_bounds(content, depth + 1, keys, svg_bytes, ctx),
        Node::Responsive { content, .. } => {
            check_bounds(content, depth + 1, keys, svg_bytes, ctx);
        }
        Node::When { condition, .. } => {
            assert!(
                condition.ops.len() <= view_wire::MAX_QUERY_OPS,
                "{ctx}: condition budget"
            );
        }
        Node::Canvas { style, commands } => {
            check_native_style(style);
            assert!(
                commands.len() <= view_wire::MAX_CANVAS_PARTS,
                "{ctx}: canvas command budget"
            );
        }
        Node::Surface { name, args, .. } => {
            check_string(name, ctx, "surface name");
            for value in args {
                match value {
                    view_wire::SurfaceValue::Str(text) => check_string(text, ctx, "surface arg"),
                    view_wire::SurfaceValue::F64(number) => assert!(number.is_finite()),
                    _ => {}
                }
            }
        }
        Node::Editor {
            placeholder,
            label,
            document,
            ..
        } => {
            if let Some(label) = label {
                check_string(label, ctx, "accessible label");
            }
            check_string(placeholder, ctx, "editor placeholder");
            // A document is metadata: sanitize keeps a valid reference whole,
            // and never spends the display budget on the bytes it names.
            assert_eq!(
                document.validate(),
                Ok(()),
                "{ctx}: sanitize kept an invalid editor document reference"
            );
        }
    }
}

/// Every post-condition `sanitize` promises about a whole frame: the tree's
/// node count and every bound `check_bounds` covers, plus every request's
/// `kind`.
/// Every editor document reference in the tree, in one fixed walk order, so
/// the same tree before and after `sanitize` compares element for element.
pub(super) fn document_refs(root: &Node) -> Vec<editor_document::EditorDocumentRef> {
    let mut pending = vec![root];
    let mut references = Vec::new();
    while let Some(node) = pending.pop() {
        if let Node::Editor { document, .. } = node {
            references.push(document.clone());
        }
        pending.extend(node.children());
    }
    references
}

pub(super) fn check_frame(frame: &Frame, ctx: &str) {
    if let Some(root) = &frame.root {
        assert!(
            !has_duplicate_typed_siblings(root),
            "{ctx}: authored identity scope aliases state"
        );
    }

    if let Some(root) = &frame.root {
        assert!(
            root.count() <= MAX_NODES,
            "{ctx}: {} nodes, over MAX_NODES",
            root.count()
        );
        let mut keys = HashSet::new();
        let mut svg_bytes = 0;
        check_bounds(root, 0, &mut keys, &mut svg_bytes, ctx);
        assert!(
            svg_bytes <= MAX_PICTURE_BYTES_PER_FRAME,
            "{ctx}: {svg_bytes} picture bytes, over MAX_PICTURE_BYTES_PER_FRAME"
        );
    }
    for request in &frame.requests {
        check_string(&request.kind, ctx, "request kind");
    }
}

pub(super) fn has_duplicate_typed_siblings(node: &Node) -> bool {
    fn walk<'a>(node: &'a Node, scope: &mut HashSet<&'a ElementIdWire>) -> bool {
        if let Some(IdentityKeyRef::Element(id)) = node.identity() {
            if !scope.insert(id) {
                return true;
            }
            let mut child_scope = HashSet::new();
            node.children()
                .iter()
                .any(|child| walk(child, &mut child_scope))
        } else {
            node.children().iter().any(|child| walk(child, scope))
        }
    }
    walk(node, &mut HashSet::new())
}
