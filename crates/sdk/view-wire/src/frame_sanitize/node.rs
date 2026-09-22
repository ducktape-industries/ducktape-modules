use super::*;

pub(super) fn sanitize_node(
    node: &mut Node,
    depth: usize,
    budgets: &mut Budgets,
    taken: &mut Taken,
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
    let typed_id = match node.identity() {
        Some(IdentityKeyRef::Element(id)) => Some(id.clone()),
        _ => None,
    };
    let typed_scope_started = claim_typed_scope(node, identity_scopes)?;
    if let Some(id) = typed_id {
        authored_path.push(id);
    }
    if let Node::Container { interactivity, .. }
    | Node::UniformList { interactivity, .. }
    | Node::Image { interactivity, .. }
    | Node::Svg { interactivity, .. } = node
    {
        sanitize_interactivity(interactivity);
        if let Some(tooltip) = &mut interactivity.tooltip {
            tooltip.delay_ms = tooltip.delay_ms.min(60_000);
            if budgets.nodes == 0 {
                tooltip.content = None;
            } else if let Some(content) = &mut tooltip.content {
                sanitize_node(
                    content,
                    depth + 1,
                    budgets,
                    taken,
                    &mut vec![std::collections::HashSet::new()],
                    &mut Vec::new(),
                )?;
            }
        }
    }
    match node {
        Node::Container { id, style, .. } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
        }
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
            id.validate_host()?;
            if path.is_empty()
                || path.len() > 64
                || path.last() != Some(id)
                || path != authored_path
            {
                return Err("uniform-list authored path is invalid");
            }
            for ancestor in path.iter() {
                ancestor.validate_host()?;
            }
            *count = (*count).min(MAX_UNIFORM_LIST_COUNT);
            *measure_index = (*measure_index).min(count.saturating_sub(1));
            if let Some(request) = scroll_request {
                request.offset = request.offset.min(MAX_UNIFORM_LIST_COUNT);
            }
            let mut kept_indices = Vec::with_capacity(indices.len().min(MAX_UNIFORM_LIST_ROWS));
            let mut kept_children = Vec::with_capacity(children.len().min(MAX_UNIFORM_LIST_ROWS));
            for (index, child) in indices.drain(..).zip(children.drain(..)) {
                if (index as usize) < *count && kept_indices.len() < MAX_UNIFORM_LIST_ROWS {
                    kept_indices.push(index);
                    kept_children.push(child);
                }
            }
            *indices = kept_indices;
            *children = kept_children;
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
                        *start = (*start).min(*item_count);
                        *end = (*end).clamp(*start, *item_count);
                    }
                    ListCommand::ScrollTo(offset) => {
                        offset.item_ix = offset.item_ix.min(*item_count);
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
            *range_start = (*range_start).min(*item_count);
            children.truncate(MAX_LIST_ROWS.min(item_count.saturating_sub(*range_start)));
        }
        Node::Sensor {
            id,
            style,
            reset,
            anticipate,
            delay,
            ..
        } => {
            id.validate_host()?;
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
        Node::MouseArea { id, label, .. } => {
            id.validate_host()?;
            if let Some(label) = label {
                truncate_string(label);
            }
        }
        Node::ResizeHandle { id, style, .. } => {
            id.validate_host()?;
            style_sanitize::sanitize(style);
        }
        Node::Responsive { id, .. } => id.validate_host()?,
        Node::Lazy { key, .. } => claim(key, taken),
        Node::Float {
            key,
            x,
            y,
            scale,
            style,
            ..
        } => {
            claim(key, taken);
            *x = finite(*x).clamp(-MAX_PIXELS, MAX_PIXELS);
            *y = finite(*y).clamp(-MAX_PIXELS, MAX_PIXELS);
            *scale = finite(*scale).clamp(f32::EPSILON, MAX_PIXELS);
            style_sanitize::sanitize(style);
        }
        Node::Tooltip {
            key,
            delay_ms,
            style,
            children,
            ..
        } => {
            claim(key, taken);
            style_sanitize::sanitize(style);
            *delay_ms = (*delay_ms).min(60_000);
            children.truncate(2);
        }
        Node::Overlay {
            id,
            label,
            style,
            children,
            ..
        } => {
            id.validate_host()?;
            if let Some(label) = label {
                truncate_string(label);
            }
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
        Node::When { key, condition, .. } => {
            claim(key, taken);
            condition.sanitize();
        }
        Node::Scroll {
            id,
            bar_width,
            bar_margin,
            scroller_width,
            bar_spacing,
            style,
            ..
        } => {
            id.validate_host()?;
            for number in [bar_width, bar_margin, scroller_width, bar_spacing] {
                bound_optional(number);
            }
            style_sanitize::sanitize(style);
        }
        Node::Qr { key, code, style } => {
            claim(key, taken);
            code.sanitize(budgets);
            style_sanitize::sanitize(style);
        }
        Node::RichText {
            id,
            style,
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
            rich_text::sanitize(text, runs, font_family_overrides, clickable_ranges, budgets);
        }
        Node::Text {
            id,
            style,
            content,
            heading,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
            spend_text(content, budgets);
            if heading.is_some_and(|level| !(1..=6).contains(&level)) {
                *heading = None;
            }
        }
        Node::ImageViewer {
            id,
            data,
            label,
            options,
            style,
            ..
        } => {
            id.validate_host()?;
            ImageData::sanitize(data, budgets);
            if let Some(label) = label {
                truncate_string(label);
            }
            options.sanitize();
            style_sanitize::sanitize(style);
        }
        Node::Image {
            id,
            data,
            label,
            style,
            loading,
            fallback,
            state_children,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
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
            id,
            source,
            transformation,
            label,
            style,
            interactivity,
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
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
            id,
            placeholder,
            value,
            options,
            style,
            ..
        } => {
            id.validate_host()?;
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
            id,
            style,
            placeholder,
            label,
            ..
        } => {
            id.validate_host()?;
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
            key,
            content,
            label,
            description,
            style,
            ..
        } => {
            claim(key, taken);
            if let ButtonContent::Label(label) = content {
                spend_text(label, budgets);
            }
            if let Some(label) = label {
                truncate_string(label);
            }
            if let Some(description) = description {
                spend_text(description, budgets);
            }
            style_sanitize::sanitize(style);
        }
        Node::Space { style } => style_sanitize::sanitize(style),
        Node::Rule { key, style, .. } => {
            claim(key, taken);
            style_sanitize::sanitize(style);
        }
        Node::Toggle {
            key, label, style, ..
        } => {
            claim(key, taken);
            spend_text(label, budgets);
            style_sanitize::sanitize(style);
        }
        Node::Radio {
            key, label, style, ..
        } => {
            claim(key, taken);
            spend_text(label, budgets);
            style_sanitize::sanitize(style);
        }
        Node::Slider {
            id,
            label,
            value,
            min,
            max,
            step,
            style,
            ..
        } => {
            id.validate_host()?;
            if let Some(label) = label {
                truncate_string(label);
            }
            for number in [value, min, max, step] {
                *number = finite(*number);
            }
            style_sanitize::sanitize(style);
        }
        Node::ComboBox {
            id,
            state_key,
            options,
            selected,
            placeholder,
            label,
            settings,
            style,
            ..
        } => {
            id.validate_host()?;
            spend_text(state_key, budgets);
            settings.sanitize(budgets);
            style_sanitize::sanitize(style);
            options.truncate(MAX_OPTIONS);
            for option in options.iter_mut() {
                spend_text(option, budgets);
            }
            spend_text(placeholder, budgets);
            if let Some(label) = label {
                truncate_string(label);
            }
            if selected.is_some_and(|index| index as usize >= options.len()) {
                *selected = None;
            }
        }
        Node::PickList {
            settings,
            id,
            options,
            selected,
            placeholder,
            label,
            style,
            ..
        } => {
            id.validate_host()?;
            if let Some(label) = label {
                truncate_string(label);
            }
            settings.sanitize(budgets);
            style_sanitize::sanitize(style);
            options.truncate(MAX_OPTIONS);
            for option in options.iter_mut() {
                spend_text(option, budgets);
            }
            if let Some(placeholder) = placeholder {
                spend_text(placeholder, budgets);
            }
            if selected.is_some_and(|index| index as usize >= options.len()) {
                *selected = None;
            }
        }
        Node::Progress {
            key,
            value,
            min,
            max,
            style,
            ..
        } => {
            claim(key, taken);
            for number in [value, min, max] {
                *number = finite(*number);
            }
            style_sanitize::sanitize(style);
        }
        Node::Surface {
            id,
            name,
            args,
            style,
            ..
        } => {
            id.validate_host()?;
            spend_text(name, budgets);
            style_sanitize::sanitize(style);
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
    }
    // Children past the budget are dropped, not stood in for: a layout of
    // ten thousand rows becomes its first rows, which is what a host can
    // lay out, rather than ten thousand empty nodes it still has to walk.
    if let Node::Container { children, .. }
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
            sanitize_node(
                child,
                depth + 1,
                budgets,
                taken,
                identity_scopes,
                authored_path,
            )?;
            kept += 1;
        }
        children.truncate(kept);
        if let Node::Image {
            loading,
            fallback,
            state_children,
            ..
        } = node
        {
            if state_children.len() < usize::from(*loading) + usize::from(*fallback) {
                *loading = false;
                *fallback = false;
                state_children.clear();
            }
        }
        finish_typed_scope(identity_scopes, typed_scope_started);
        if typed_scope_started {
            authored_path.pop();
        }
        return Ok(());
    }
    for child in node.children_mut() {
        if budgets.nodes == 0 {
            *child = Node::empty();
            continue;
        }
        sanitize_node(
            child,
            depth + 1,
            budgets,
            taken,
            identity_scopes,
            authored_path,
        )?;
    }
    finish_typed_scope(identity_scopes, typed_scope_started);
    if typed_scope_started {
        authored_path.pop();
    }
    Ok(())
}
