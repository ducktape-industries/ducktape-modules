use super::*;

pub(super) fn sanitize_interactivity(interactivity: &mut Interactivity) {
    interactivity.window_control_area = None;
    interactivity.aria.sanitize();
    for style in [
        &mut interactivity.focus,
        &mut interactivity.in_focus,
        &mut interactivity.focus_visible,
    ]
    .into_iter()
    .flatten()
    {
        style_sanitize::sanitize(style);
    }
    if let Some(context) = &mut interactivity.key_context {
        context
            .entries
            .truncate(crate::interactivity::MAX_KEY_CONTEXT_ENTRIES);
        for entry in &mut context.entries {
            let mut key = entry.key.to_string();
            truncate_string(&mut key);
            entry.key = key.into();
            if let Some(value) = &mut entry.value {
                let mut bounded = value.to_string();
                truncate_string(&mut bounded);
                *value = bounded.into();
            }
        }
    }
    for style in [&mut interactivity.hover, &mut interactivity.active]
        .into_iter()
        .flatten()
    {
        style_sanitize::sanitize(style);
    }
    for group in [
        &mut interactivity.group_hover,
        &mut interactivity.group_active,
    ]
    .into_iter()
    .flatten()
    {
        style_sanitize::sanitize(&mut group.style);
        let mut name = group.group.to_string();
        truncate_string(&mut name);
        group.group = name.into();
    }
    if let Some(group) = &mut interactivity.group {
        let mut name = group.to_string();
        truncate_string(&mut name);
        *group = name.into();
    }
}
