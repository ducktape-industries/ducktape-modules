//! Native editor projection and key decisions for the composer.
use super::super::editing;
use super::*;
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

pub(super) fn key_tag(
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
        // These three are claimed only while the menu is open (see `editor`),
        // but a claim is a frame behind the keystroke: if the menu closed in
        // between, the host still asks this frame. "ignore" is the answer —
        // never the empty tag, which falls to the catch-all default action,
        // and an app that knows only Enter/Tab/Backspace as defaults stops
        // the whole view when it is handed any other key.
        Key::Named(Named::ArrowUp | Named::ArrowDown | Named::Escape) => "ignore".into(),
        Key::Named(Named::Backspace) => "backspace".into(),
        Key::Named(Named::Delete) => "delete".into(),
        _ => String::new(),
    }
}

/// `key` names the NODE — the accessibility tree and every test door address
/// it. `document` names the DOCUMENT, and the host keys its native editor
/// state by that, not by the node key. They are not the same identity: one
/// place on screen presents a different draft as the reader moves between
/// rooms, so each draft must carry its own document id or the host hands the
/// new draft the old one's text and drops every transaction after it.
pub fn editor<V: 'static>(
    draft: &Draft,
    key: &str,
    document: &str,
    placeholder: &str,
    editable: bool,
    choices: &[MentionChoice],
    handle: Handle<V>,
) -> wire::Node {
    let effect = move |event: Event<V>| {
        let handle = handle.clone();
        {
            let event = std::cell::RefCell::new(Some(event));
            let callback: Callback<V> = Rc::new(move |view, window, cx| {
                if let Some(event) = event.borrow_mut().take() {
                    handle(view, event, window, cx);
                    cx.notify();
                }
            });
            callback
        }
    };
    let effect = Rc::new(effect);
    let doc_effect = effect.clone();
    let (document, on_document) = draft.editor.document(document.into(), move |update| {
        doc_effect(Event::Document(update))
    });
    let draft = draft.clone();
    let choices = choices.to_vec();
    let bare = Modifiers::default();
    let mut claims = [Named::Enter, Named::Tab, Named::Backspace, Named::Delete]
        .into_iter()
        .chain(
            // An arrow moves the caret, and this view cannot: it has no
            // layout to move it through. So the arrows are the menu's keys
            // while the menu is open, and the host's own the rest of the
            // time — claiming them always is how a plain ArrowUp reached the
            // guest with nothing to say. Escape rides along: a closed menu
            // has nothing to dismiss.
            draft
                .query(draft.editor.state_view())
                .is_some()
                .then_some([Named::ArrowUp, Named::ArrowDown, Named::Escape])
                .into_iter()
                .flatten(),
        )
        .map(|key| wire::EditorKeyClaim {
            key: Key::Named(key),
            modifiers: bare,
            command: false,
        })
        .collect::<Vec<_>>();
    claims.extend(
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
            modifiers: Modifiers { shift, ..bare },
            command: true,
        }),
    );
    let deciding = draft.clone();
    let decide_choices = choices.clone();
    let observing = draft.clone();
    let observed_choices = choices.clone();
    let interacting = draft.clone();
    let on_committed = effect.clone();
    let on_transaction = effect.clone();
    let binding = EditorBinding::new(
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
                interacting.decide(tag, &choices, request.state)
            }
            _ => wire::EditorDecision::Noop,
        }
    })
    .register(
        move |change| on_committed(Event::Committed(change)),
        move |transaction| on_transaction(Event::Transaction(transaction)),
    );
    // a mention wears the product's accent, the same one a chosen row and a
    // live dot wear — not a colour of this module's own
    let mut presentation = wire::editor_presentation::EditorPresentation {
        formats: vec![wire::editor_presentation::EditorFormat {
            color: Some(kit::rgba(kit::palette().accent)),
            ..Default::default()
        }],
        ..Default::default()
    };
    for mention in &draft.mentions {
        let start = editing::position(draft.editor.state_view().text, mention.range.start);
        let end = editing::position(draft.editor.state_view().text, mention.range.end);
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
    wire::Node::Editor {
        key: key.into(),
        document,
        on_document,
        editable,
        placeholder: placeholder.into(),
        // the field's accessible name is what its placeholder asks for, the
        // rule `kit::input` keeps for a plain input
        label: (!placeholder.is_empty()).then(|| placeholder.into()),
        width: None,
        height: None,
        // one row of body text, and room to grow to about eight before the
        // field scrolls instead of eating the timeline
        min_height: Some(40.),
        max_height: Some(200.),
        options: Box::new(wire::EditorOptions {
            binding: Some(Box::new(binding)),
            presentation: Some(Box::new(presentation)),
            // the body size every view writes at, not a size of its own
            size: Some(kit::type_scale::BODY as f32),
            // This IS the draft's text inset — the host pads the field's box
            // by it and the text control adds nothing of its own, so at 0 the
            // first letter sits on the plate's border (seen on a live app,
            // 2026-09-16). Vertically it is also the air above the first row,
            // which is why `min_height` is exactly one row plus twice this.
            padding: Some(TEXT_INSET),
            wrapping: Some(wire::Wrapping::Word),
            ..Default::default()
        }),
    }
}
