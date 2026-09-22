//! Guest-owned composer projection built from the GPUI-shaped SDK surface.

use super::{Draft, MentionChoice};
use crate::context::Callback;
use crate::{
    Context, EditorBinding, EditorDocumentUpdate, EditorKeyRequest, EditorTransaction,
    EditorTransactionEvent, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, View, Window, div, wire,
};
use gpui::Styled;
use std::rc::Rc;
use wire::keyboard::{Key, Modifiers, Named};

#[derive(Clone, Debug)]
pub struct Change {
    pub before: String,
    pub after: String,
    pub cursor: wire::EditorCursor,
    pub tag: String,
}

pub enum Event<V> {
    Document(EditorDocumentUpdate),
    Transaction(EditorTransaction<Callback<V>>),
    Committed(Change),
    Action(String),
}

impl<V> Clone for Event<V> {
    fn clone(&self) -> Self {
        match self {
            Self::Document(update) => Self::Document(update.clone()),
            Self::Transaction(transaction) => Self::Transaction(transaction.clone()),
            Self::Committed(change) => Self::Committed(change.clone()),
            Self::Action(action) => Self::Action(action.clone()),
        }
    }
}

pub enum Outcome<V> {
    Updated,
    Run(Callback<V>),
    Action(String),
    Enqueue(String),
}

pub type Handle<V> = Rc<dyn Fn(&mut V, Event<V>, &mut Window, &mut Context<V>)>;

fn matching_choices<'a>(choices: &'a [MentionChoice], partial: &str) -> Vec<&'a MentionChoice> {
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
    state: crate::EditorStateView<'_>,
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

impl Draft {
    pub fn handle<V: 'static>(&mut self, event: Event<V>, choices: &[MentionChoice]) -> Outcome<V> {
        match event {
            Event::Document(update) => {
                update.apply(&mut self.editor);
                Outcome::Updated
            }
            Event::Transaction(transaction) => transaction
                .apply(&mut self.editor)
                .map_or(Outcome::Updated, Outcome::Run),
            Event::Committed(change) => {
                self.committed(
                    &change.before,
                    &change.after,
                    change.cursor,
                    &change.tag,
                    choices,
                );
                match change.tag.as_str() {
                    "send" | "attach" | "paste" | "copy" | "cut" | "restore" => {
                        Outcome::Action(change.tag)
                    }
                    _ if change.tag.starts_with("remove:") || change.tag.starts_with("retry:") => {
                        Outcome::Action(change.tag)
                    }
                    _ => Outcome::Updated,
                }
            }
            Event::Action(tag) => Outcome::Enqueue(tag),
        }
    }
}

/// Render a composer using the host editor node and GPUI-shaped surrounding
/// elements. Native editor behavior remains a host primitive.
#[allow(clippy::too_many_arguments)]
pub fn view<V: View + 'static>(
    draft: &Draft,
    key: &str,
    hint: &str,
    editable: bool,
    choices: &[MentionChoice],
    cx: &mut Context<V>,
    handle: impl Fn(&mut V, Event<V>, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let handle: Handle<V> = Rc::new(handle);
    let effect_handle = handle.clone();
    let effect = Rc::new(move |event: Event<V>| {
        let handle = effect_handle.clone();
        let callback: Callback<V> = Rc::new(
            move |view: &mut V, window: &mut Window, cx: &mut Context<'_, V>| {
                handle(view, event.clone(), window, cx)
            },
        );
        callback
    });
    let slots = cx.app.inner.slots.clone();
    let document_key = format!("{key}/editor");
    let document_effect = effect.clone();
    let (document, on_document) = draft.editor.document(&slots, document_key, move |update| {
        document_effect(Event::Document(update))
    });
    let draft_for_decisions = draft.clone();
    let choices_for_decisions = choices.to_vec();
    let deciding = draft_for_decisions.clone();
    let decide_choices = choices_for_decisions.clone();
    let observing = draft_for_decisions.clone();
    let observed_choices = choices_for_decisions.clone();
    let interacting = draft_for_decisions;
    let interaction_choices = choices_for_decisions;
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
        .collect();
    let binding = EditorBinding::<Change>::new(
        claims,
        move |request: EditorKeyRequest<'_>| {
            if !editable {
                return wire::EditorDecision::Noop;
            }
            let tag = key_tag(&deciding, &decide_choices, request.state, request.key);
            if tag == "send" && request.repeat {
                wire::EditorDecision::Noop
            } else {
                deciding.decide(&tag, &decide_choices, request.state)
            }
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
    let committed_effect = effect.clone();
    let transaction_effect = effect.clone();
    let binding = binding.register(
        &slots,
        move |change| committed_effect(Event::Committed(change)),
        move |transaction| transaction_effect(Event::Transaction(transaction)),
    );
    let editor = wire::Node::Editor {
        options: Box::new(wire::EditorOptions {
            binding: Some(Box::new(binding)),
            ..Default::default()
        }),
        key: format!("{key}/editor"),
        placeholder: hint.to_owned(),
        label: (!hint.is_empty()).then(|| hint.to_owned()),
        document,
        on_document,
        editable,
        width: None,
        height: None,
        min_height: Some(40.),
        max_height: Some(200.),
    };
    let mut root = div()
        .id(key.to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .rounded_md()
        .border_1()
        .p_2()
        .child(editor);
    if let Some((_, partial)) = draft.query(draft.editor.state_view()) {
        for choice in choices
            .iter()
            .filter(|choice| {
                choice
                    .label
                    .to_lowercase()
                    .starts_with(&partial.to_lowercase())
            })
            .take(32)
        {
            root = root.child(
                div()
                    .id(format!("{key}/mention/{}", choice.token))
                    .px_2()
                    .py_1()
                    .child(format!("@{}", choice.label)),
            );
        }
    }
    if !draft.note.is_empty() {
        root = root.child(
            div()
                .text_color(gpui::rgb(0xff0000))
                .child(draft.note.clone()),
        );
    }
    let sendable = editable && draft.can_send(draft.editor.state_view().text);
    let send = div().id(format!("{key}/send")).px_2().py_1().child("Send");
    if sendable {
        let send_handle = handle.clone();
        let click = cx.listener(move |view, _: &gpui::ClickEvent, window, cx| {
            send_handle(view, Event::Action("send".into()), window, cx);
            cx.notify();
        });
        root.child(send.on_click(click))
    } else {
        root.child(send)
    }
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
