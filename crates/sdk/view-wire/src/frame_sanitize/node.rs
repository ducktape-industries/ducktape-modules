use super::*;

/// One node and, within what is left of the budgets, everything under it.
/// The walk per node: its typed id claimed in its scope and checked, its
/// interactivity (and a tooltip's content) bounded, its own fields bounded
/// by [`sanitize_fields`], then its children.
pub(super) fn sanitize_node(
    node: &mut Node,
    depth: usize,
    budgets: &mut Budgets,
    identity_scopes: &mut IdentityScopes,
    authored_path: &mut Vec<ElementIdWire>,
) -> Result<(), &'static str> {
    // The caller guarantees one node of budget; a node too deep spends it
    // on the empty node that stands in for it.
    budgets.nodes -= 1;
    if depth >= MAX_DEPTH {
        *node = Node::empty();
        return Ok(());
    }
    let typed_id = node.identity().cloned();
    let typed_scope_started = claim_typed_scope(node, identity_scopes)?;
    if let Some(id) = &typed_id {
        authored_path.push(id.clone());
    }
    if let Node::Container(crate::ContainerNode { interactivity, .. })
    | Node::UniformList { interactivity, .. }
    | Node::Image { interactivity, .. }
    | Node::Svg { interactivity, .. } = node
    {
        sanitize_interactivity(interactivity);
        if let Some(tooltip) = &mut interactivity.tooltip {
            tooltip.delay_ms = tooltip.delay_ms.min(60_000);
            sanitize_tooltip_content(&mut tooltip.content, depth, budgets)?;
        }
    }
    // Every variant that carries an id (`Node::identity`) has it checked.
    if let Some(id) = &typed_id {
        id.validate_host()?;
    }
    sanitize_fields(node, depth, budgets, authored_path)?;
    sanitize_children(node, depth, budgets, identity_scopes, authored_path)?;
    finish_typed_scope(identity_scopes, typed_scope_started);
    if typed_scope_started {
        authored_path.pop();
    }
    Ok(())
}

/// A tooltip's content is a tree of its own, in a fresh identity scope, on
/// the frame's node budget; with none left it is dropped.
fn sanitize_tooltip_content(
    content: &mut Option<Box<Node>>,
    depth: usize,
    budgets: &mut Budgets,
) -> Result<(), &'static str> {
    if budgets.nodes == 0 {
        *content = None;
    } else if let Some(content) = content {
        sanitize_node(
            content,
            depth + 1,
            budgets,
            &mut vec![std::collections::HashSet::new()],
            &mut Vec::new(),
        )?;
    }
    Ok(())
}

/// A node's own fields, one arm per kind; its id is already checked.
fn sanitize_fields(
    node: &mut Node,
    depth: usize,
    budgets: &mut Budgets,
    authored_path: &[ElementIdWire],
) -> Result<(), &'static str> {
    match node {
        Node::Container(crate::ContainerNode { style, .. })
        | Node::ResizeHandle { style, .. }
        | Node::Rule { style, .. }
        | Node::Space { style } => style_sanitize::sanitize(style),
        Node::Responsive { .. } | Node::Lazy { .. } => {}
        Node::UniformList {
            id,
            path,
            style,
            count,
            measure_index,
            scroll_request,
            indices,
            children,
            ..
        } => {
            style_sanitize::sanitize(style);
            uniform_list_path(id, path, authored_path)?;
            *count = (*count).min(MAX_UNIFORM_LIST_COUNT);
            *measure_index = (*measure_index).min(count.saturating_sub(1));
            if let Some(request) = scroll_request {
                request.offset = request.offset.min(MAX_UNIFORM_LIST_COUNT);
            }
            uniform_list_rows(*count, indices, children);
        }
        Node::List {
            path,
            item_count,
            overdraw,
            style,
            commands,
            range_start,
            children,
            ..
        } => {
            if path != authored_path {
                return Err("list authored path is invalid");
            }
            for id in path.iter() {
                id.validate_host()?;
            }
            *item_count = (*item_count).min(budgets.list_items);
            budgets.list_items -= *item_count;
            *overdraw = bounded(*overdraw).min(4096.0);
            style_sanitize::sanitize(style);
            list_commands(commands, *item_count);
            *range_start = (*range_start).min(*item_count);
            children.truncate(MAX_LIST_ROWS.min(item_count.saturating_sub(*range_start)));
        }
        Node::Sensor {
            style,
            reset,
            anticipate,
            delay,
            ..
        } => {
            style_sanitize::sanitize(style);
            if let Some(value) = reset
                && !value.bound(0, budgets, false)
            {
                *reset = None;
            }
            bound_optional(anticipate);
            if let Some(delay) = delay {
                *delay = finite(*delay).max(0.0);
            }
        }
        Node::MouseArea { label, .. } => label.iter_mut().for_each(truncate_string),
        Node::Float {
            x, y, scale, style, ..
        } => {
            *x = finite(*x).clamp(-MAX_PIXELS, MAX_PIXELS);
            *y = finite(*y).clamp(-MAX_PIXELS, MAX_PIXELS);
            *scale = finite(*scale).clamp(f32::EPSILON, MAX_PIXELS);
            style_sanitize::sanitize(style);
        }
        Node::Tooltip {
            delay_ms,
            style,
            children,
            ..
        } => {
            style_sanitize::sanitize(style);
            *delay_ms = (*delay_ms).min(60_000);
            children.truncate(2);
        }
        Node::Overlay {
            label,
            style,
            children,
            ..
        } => {
            label.iter_mut().for_each(truncate_string);
            style_sanitize::sanitize(style);
            children.truncate(2);
        }
        Node::Canvas { style, commands } => {
            style_sanitize::sanitize(style);
            canvas::sanitize(commands, budgets);
        }
        Node::Anchored {
            fit,
            position,
            offset,
            ..
        } => {
            for point in [position, offset].into_iter().flatten() {
                for value in point {
                    *value = signed_bounded(*value);
                }
            }
            if let AnchoredFitMode::SnapToWindowWithMargin(edges) = fit {
                for edge in edges {
                    *edge = bounded(*edge);
                }
            }
        }
        Node::Deferred { priority, .. } => *priority = (*priority).min(16),
        Node::When { condition, .. } => condition.sanitize(),
        Node::Scroll {
            bar_width,
            bar_margin,
            scroller_width,
            bar_spacing,
            style,
            ..
        } => {
            for number in [bar_width, bar_margin, scroller_width, bar_spacing] {
                bound_optional(number);
            }
            style_sanitize::sanitize(style);
        }
        Node::Qr { code, style, .. } => {
            code.sanitize(budgets);
            style_sanitize::sanitize(style);
        }
        Node::RichText {
            style,
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            tooltip,
            ..
        } => {
            style_sanitize::sanitize(style);
            rich_text::sanitize(text, runs, font_family_overrides, clickable_ranges, budgets);
            if let Some(tooltip) = tooltip {
                sanitize_tooltip_content(&mut tooltip.content, depth, budgets)?;
            }
        }
        Node::Text(crate::TextNode {
            style,
            content,
            heading,
            ..
        }) => {
            style_sanitize::sanitize(style);
            spend_text(content, budgets);
            if heading.is_some_and(|level| !(1..=6).contains(&level)) {
                *heading = None;
            }
        }
        Node::ImageViewer {
            data,
            label,
            options,
            style,
            ..
        } => {
            ImageData::sanitize(data, budgets);
            label.iter_mut().for_each(truncate_string);
            options.sanitize();
            style_sanitize::sanitize(style);
        }
        Node::Image {
            data,
            label,
            style,
            loading,
            fallback,
            state_children,
            ..
        } => {
            ImageData::sanitize(data, budgets);
            style_sanitize::sanitize(style);
            if let Some(label) = label {
                spend_text(label, budgets);
            }
            let expected = usize::from(*loading) + usize::from(*fallback);
            state_children.truncate(expected);
            if state_children.len() < expected {
                *loading = false;
                *fallback = false;
                state_children.clear();
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
            match source {
                SvgSource::Data { bytes, .. } => spend_svg(bytes, budgets),
                SvgSource::Asset(path) | SvgSource::External(path) => truncate_string(path),
                SvgSource::None => {}
            }
            for value in &mut transformation.scale {
                *value = signed_bounded(*value);
            }
            for value in &mut transformation.translate {
                *value = signed_bounded(*value);
            }
            transformation.rotate = signed_bounded(transformation.rotate);
            style_sanitize::sanitize(style);
            sanitize_interactivity(interactivity);
            if let Some(label) = label {
                spend_text(label, budgets);
            }
        }
        Node::Input {
            placeholder,
            value,
            options,
            style,
            ..
        } => {
            spend_text(placeholder, budgets);
            spend_text(value, budgets);
            spend_text(&mut options.label, budgets);
            if let Some(description) = &mut options.description {
                spend_text(description, budgets);
            }
            style_sanitize::sanitize(style);
        }
        Node::Editor {
            options,
            style,
            placeholder,
            label,
            ..
        } => {
            style_sanitize::sanitize(style);
            if let Some(presentation) = &mut options.presentation {
                presentation.sanitize(budgets);
            }
            if let Some(rich) = &mut options.rich {
                for item in &mut rich.toolbar {
                    spend_text(&mut item.label, budgets);
                }
            }
            spend_text(placeholder, budgets);
            if let Some(label) = label {
                spend_text(label, budgets);
            }
        }
        Node::Button {
            content,
            label,
            description,
            style,
            ..
        } => {
            if let ButtonContent::Label(label) = content {
                spend_text(label, budgets);
            }
            label.iter_mut().for_each(truncate_string);
            if let Some(description) = description {
                spend_text(description, budgets);
            }
            style_sanitize::sanitize(style);
        }
        Node::Toggle { label, style, .. } | Node::Radio { label, style, .. } => {
            spend_text(label, budgets);
            style_sanitize::sanitize(style);
        }
        Node::Slider {
            label,
            value,
            min,
            max,
            step,
            style,
            ..
        } => {
            label.iter_mut().for_each(truncate_string);
            for number in [value, min, max, step] {
                *number = finite(*number);
            }
            style_sanitize::sanitize(style);
        }
        Node::ComboBox {
            state_key,
            options,
            selected,
            placeholder,
            label,
            settings,
            style,
            ..
        } => {
            spend_text(state_key, budgets);
            settings.sanitize(budgets);
            style_sanitize::sanitize(style);
            menu_options(options, selected, budgets);
            spend_text(placeholder, budgets);
            label.iter_mut().for_each(truncate_string);
        }
        Node::PickList {
            settings,
            options,
            selected,
            placeholder,
            label,
            style,
            ..
        } => {
            label.iter_mut().for_each(truncate_string);
            settings.sanitize(budgets);
            style_sanitize::sanitize(style);
            menu_options(options, selected, budgets);
            if let Some(placeholder) = placeholder {
                spend_text(placeholder, budgets);
            }
        }
        Node::Progress {
            value,
            min,
            max,
            style,
            ..
        } => {
            for number in [value, min, max] {
                *number = finite(*number);
            }
            style_sanitize::sanitize(style);
        }
        Node::Surface {
            name, args, style, ..
        } => {
            spend_text(name, budgets);
            style_sanitize::sanitize(style);
            surface_args(args, budgets);
        }
    }
    Ok(())
}

/// A uniform list names the path of ids that authored it, ending in its own,
/// and it must be the path the walk actually took to reach it.
fn uniform_list_path(
    id: &ElementIdWire,
    path: &[ElementIdWire],
    authored_path: &[ElementIdWire],
) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > 64 || path.last() != Some(id) || path != authored_path {
        return Err("uniform-list authored path is invalid");
    }
    for ancestor in path {
        ancestor.validate_host()?;
    }
    Ok(())
}

/// Keeps the rows whose index is inside `count`, at most
/// [`MAX_UNIFORM_LIST_ROWS`] of them.
fn uniform_list_rows(count: usize, indices: &mut Vec<u32>, children: &mut Vec<Node>) {
    let mut kept_indices = Vec::with_capacity(indices.len().min(MAX_UNIFORM_LIST_ROWS));
    let mut kept_children = Vec::with_capacity(children.len().min(MAX_UNIFORM_LIST_ROWS));
    for (index, child) in indices.drain(..).zip(children.drain(..)) {
        if (index as usize) < count && kept_indices.len() < MAX_UNIFORM_LIST_ROWS {
            kept_indices.push(index);
            kept_children.push(child);
        }
    }
    *indices = kept_indices;
    *children = kept_children;
}

/// A list's commands, at most [`MAX_LIST_COMMANDS`], each held inside the
/// list's (already bounded) item count.
fn list_commands(commands: &mut Vec<ListCommand>, item_count: usize) {
    commands.truncate(MAX_LIST_COMMANDS);
    for command in commands {
        match command {
            ListCommand::Reset { count } => *count = (*count).min(MAX_LIST_ITEMS),
            ListCommand::Splice { start, end, count } => {
                *start = (*start).min(MAX_LIST_ITEMS);
                *end = (*end).clamp(*start, MAX_LIST_ITEMS);
                *count = (*count).min(MAX_LIST_ITEMS);
            }
            ListCommand::Remeasure { start, end } => {
                *start = (*start).min(item_count);
                *end = (*end).clamp(*start, item_count);
            }
            ListCommand::ScrollTo(offset) => {
                offset.item_ix = offset.item_ix.min(item_count);
                offset.offset_in_item = bounded(offset.offset_in_item);
            }
            ListCommand::ScrollToRevealItem(index) => {
                *index = (*index).min(item_count.saturating_sub(1));
            }
            ListCommand::ScrollToEnd
            | ListCommand::SetFollowMode { .. }
            | ListCommand::PauseFollowingTail => {}
        }
    }
}

/// A menu's options: at most [`MAX_OPTIONS`], each spent from the frame's
/// text budget, and a selection that points past them dropped.
fn menu_options(options: &mut Vec<String>, selected: &mut Option<u32>, budgets: &mut Budgets) {
    options.truncate(MAX_OPTIONS);
    for option in options.iter_mut() {
        spend_text(option, budgets);
    }
    if selected.is_some_and(|index| index as usize >= options.len()) {
        *selected = None;
    }
}

/// A surface's arguments: at most [`MAX_SURFACE_ARGS`], cut where the frame's
/// surface values run out, and a value past its own bounds made `Unit`.
fn surface_args(args: &mut Vec<SurfaceValue>, budgets: &mut Budgets) {
    args.truncate(MAX_SURFACE_ARGS);
    let mut kept = 0;
    for value in args.iter_mut() {
        if budgets.surface_values == 0 {
            break;
        }
        if !value.bound(0, budgets, false) {
            *value = SurfaceValue::Unit;
        }
        kept += 1;
    }
    args.truncate(kept);
}

/// The children, in tree order, on what is left of the node budget.
fn sanitize_children(
    node: &mut Node,
    depth: usize,
    budgets: &mut Budgets,
    identity_scopes: &mut IdentityScopes,
    authored_path: &mut Vec<ElementIdWire>,
) -> Result<(), &'static str> {
    // Children past the budget are dropped, not stood in for: a layout of
    // ten thousand rows becomes its first rows, which is what a host can
    // lay out, rather than ten thousand empty nodes it still has to walk.
    if let Node::Container(crate::ContainerNode { children, .. })
    | Node::List { children, .. }
    | Node::When { children, .. }
    | Node::Tooltip { children, .. }
    | Node::Overlay { children, .. }
    | Node::Anchored { children, .. }
    | Node::Image {
        state_children: children,
        ..
    } = node
    {
        let mut kept = 0;
        for child in children.iter_mut() {
            if budgets.nodes == 0 {
                break;
            }
            sanitize_node(child, depth + 1, budgets, identity_scopes, authored_path)?;
            kept += 1;
        }
        children.truncate(kept);
        if let Node::Image {
            loading,
            fallback,
            state_children,
            ..
        } = node
            && state_children.len() < usize::from(*loading) + usize::from(*fallback)
        {
            *loading = false;
            *fallback = false;
            state_children.clear();
        }
        return Ok(());
    }
    // A single slot (a wrapper's content) keeps its place as an empty node.
    for child in node.children_mut() {
        if budgets.nodes == 0 {
            *child = Node::empty();
            continue;
        }
        sanitize_node(child, depth + 1, budgets, identity_scopes, authored_path)?;
    }
    Ok(())
}
