//! Native editor projection and key decisions for the composer.

use super::*;
use crate::wire;
use crate::{EditorBinding, EditorStateView, EditorTransactionEvent};
use wire::keyboard::{Key, Modifiers, Named};

pub(super) fn matching_choices<'a>(
    choices: &'a [MentionChoice],
    partial: &str,
) -> Vec<&'a MentionChoice> {
    let needle = partial.to_lowercase();
    choices
        .iter()
        .filter(|choice| choice.label.to_lowercase().starts_with(&needle))
        .take(32)
        .collect()
}

pub(crate) fn key_tag(
    draft: &Draft,
    choices: &[MentionChoice],
    state: EditorStateView<'_>,
    key: &wire::keyboard::KeyState,
) -> String {
    let command = key.modifiers.control || key.modifiers.logo;
    if command {
        return match (&key.key, key.modifiers.shift) {
            (Key::Character(key), false) if key == "z" => "undo",
            (Key::Character(key), true) if key == "z" => "redo",
            (Key::Character(key), false) if key == "y" => "redo",
            (Key::Character(key), false) if key == "b" => "bold",
            (Key::Character(key), false) if key == "i" => "italic",
            (Key::Character(key), true) if key == "c" => "code",
            (Key::Character(key), true) if key == "9" => "quote",
            (Key::Character(key), false) if key == "v" => "paste",
            (Key::Character(key), false) if key == "c" => "copy",
            (Key::Character(key), false) if key == "x" => "cut",
            _ => "",
        }
        .into();
    }
    match &key.key {
        Key::Named(Named::Enter | Named::Tab) => {
            if let Some((_, partial)) = draft.query(state) {
                let choices = matching_choices(choices, &partial);
                let selected = draft.menu_index.min(choices.len().saturating_sub(1));
                if let Some(choice) = choices.get(selected) {
                    return format!("mention:{}", choice.token);
                }
            }
            if key.key == Key::Named(Named::Enter) {
                "send".into()
            } else {
                String::new()
            }
        }
        Key::Named(Named::ArrowDown) if draft.query(state).is_some() => "menu-next".into(),
        Key::Named(Named::ArrowUp) if draft.query(state).is_some() => "menu-previous".into(),
        Key::Named(Named::Escape) if draft.query(state).is_some() => "menu-dismiss".into(),
        Key::Named(Named::ArrowUp | Named::ArrowDown | Named::Escape) => "ignore".into(),
        Key::Named(Named::Backspace) => "backspace".into(),
        Key::Named(Named::Delete) => "delete".into(),
        _ => String::new(),
    }
}

pub(super) fn editor<V: 'static>(
    draft: &Draft,
    key: &str,
    document_key: &str,
    placeholder: &str,
    editable: bool,
    choices: &[MentionChoice],
    handle: Handle<V>,
    accent: gpui::Hsla,
) -> EditorElement<Change, Callback<V>> {
    let effect = move |event: Event<V>| {
        let handle = handle.clone();
        let event = std::cell::RefCell::new(Some(event));
        let callback: Callback<V> = Rc::new(
            move |view: &mut V, window: &mut Window, cx: &mut Context<V>| {
                if let Some(event) = event.borrow_mut().take() {
                    handle(view, event, window, &mut *cx);
                }
            },
        );
        callback
    };
    let effect = Rc::new(effect);
    let choices = choices.to_vec();
    let claims = [Named::Enter, Named::Tab, Named::Backspace, Named::Delete]
        .into_iter()
        .chain(
            draft
                .query(draft.editor.state_view())
                .is_some()
                .then_some([Named::ArrowUp, Named::ArrowDown, Named::Escape])
                .into_iter()
                .flatten(),
        )
        .map(|key| wire::EditorKeyClaim {
            key: Key::Named(key),
            modifiers: Modifiers::default(),
            command: false,
        })
        .chain(
            [
                ("z", false),
                ("z", true),
                ("y", false),
                ("b", false),
                ("i", false),
                ("c", true),
                ("9", true),
                ("v", false),
                ("c", false),
                ("x", false),
            ]
            .into_iter()
            .map(|(key, shift)| wire::EditorKeyClaim {
                key: Key::Character(key.into()),
                modifiers: Modifiers {
                    shift,
                    ..Modifiers::default()
                },
                command: true,
            }),
        )
        .collect::<Vec<_>>();
    let deciding = draft.clone();
    let observing = draft.clone();
    let interacting = draft.clone();
    let decide_choices = choices.clone();
    let observed_choices = choices.clone();
    let interaction_choices = choices.clone();
    let binding = EditorBinding::<Change>::new(
        claims,
        move |request| {
            if !editable {
                return wire::EditorDecision::Noop;
            }
            let tag = key_tag(&deciding, &decide_choices, request.state, request.key);
            if tag == "send" && request.repeat {
                return wire::EditorDecision::Noop;
            }
            deciding.decide(&tag, &decide_choices, request.state)
        },
        move |event| match event {
            EditorTransactionEvent::Commit {
                before,
                after,
                origin,
                ..
            } => {
                let tag = match origin {
                    Some(wire::EditorRequestInput::Key { key, .. }) => {
                        key_tag(&observing, &observed_choices, before, key)
                    }
                    Some(wire::EditorRequestInput::Interaction {
                        action: wire::editor_presentation::EditorInteraction::Action { tag },
                    }) => tag.clone(),
                    _ => String::new(),
                };
                Some(Change {
                    before: before.text.into(),
                    after: after.text.into(),
                    cursor: before.cursor,
                    tag,
                })
            }
            EditorTransactionEvent::Interaction { .. }
            | EditorTransactionEvent::Fault { .. }
            | EditorTransactionEvent::Cancelled { .. } => None,
        },
    )
    .on_interaction(move |request| {
        if !editable {
            return wire::EditorDecision::Noop;
        }
        match request.action {
            wire::editor_presentation::EditorInteraction::Action { tag } => {
                interacting.decide(tag, &interaction_choices, request.state)
            }
            _ => wire::EditorDecision::Noop,
        }
    });
    let mut presentation = wire::editor_presentation::EditorPresentation {
        formats: vec![wire::editor_presentation::EditorFormat {
            style: gpui::StyleRefinement::default().text_color(accent),
            ..Default::default()
        }],
        ..Default::default()
    };
    for mention in &draft.mentions {
        let start =
            super::super::editing::position(draft.editor.state_view().text, mention.range.start);
        let end =
            super::super::editing::position(draft.editor.state_view().text, mention.range.end);
        if start.line == end.line {
            presentation
                .spans
                .push(wire::editor_presentation::EditorSpan {
                    line: start.line,
                    start: start.column,
                    end: end.column,
                    format: 0,
                });
        }
    }
    let route_effect = effect;
    let mut editor = EditorElement::new(
        ElementId::Name(key.into()),
        &draft.editor,
        document_key,
        binding,
        move |event| {
            route_effect(match event {
                EditorElementEvent::Document(update) => Event::Document(update),
                EditorElementEvent::Observed(change) => Event::Committed(change),
                EditorElementEvent::Transaction(transaction) => Event::Transaction(transaction),
            })
        },
    )
    .placeholder(placeholder)
    .editable(editable)
    .w_full()
    .min_h(px(40.))
    .max_h(px(200.))
    .p(px(super::TEXT_INSET))
    .text_size(px(design::type_scale::BODY as f32))
    .whitespace_normal()
    .presentation(presentation);
    if !placeholder.is_empty() {
        editor = editor.label(placeholder);
    }
    editor
}
