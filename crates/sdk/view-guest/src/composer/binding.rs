//! Guest-owned composer projection built from the GPUI-shaped SDK surface.

use super::{Draft, MentionChoice};
use crate::context::Callback;
use crate::{Context, EditorDocumentUpdate, EditorTransaction, IntoElement, ParentElement,
    InteractiveElement, StatefulInteractiveElement, View, Window, div, wire};
use gpui::Styled;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Change {
    pub before: String,
    pub after: String,
    pub cursor: wire::EditorCursor,
    pub tag: String,
}

#[derive(Clone)]
pub enum Event<V> {
    Document(EditorDocumentUpdate),
    Transaction(EditorTransaction<Callback<V>>),
    Committed(Change),
    Action(String),
}

pub enum Outcome<V> {
    Updated,
    Run(Callback<V>),
    Action(String),
    Enqueue(String),
}

pub type Handle<V> = Rc<dyn Fn(&mut V, Event<V>, &mut Window, &mut Context<V>)>;

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
    let document_key = format!("{key}/editor");
    let document = draft.editor.document_reference(document_key);
    let on_document = 0;
    let editor = wire::Node::Editor {
        options: Box::new(wire::EditorOptions::default()),
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
            .filter(|choice| choice.label.to_lowercase().starts_with(&partial.to_lowercase()))
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
        root = root.child(div().text_color(gpui::rgb(0xff0000)).child(draft.note.clone()));
    }
    let sendable = editable && draft.can_send(draft.editor.state_view().text);
    let send = div()
        .id(format!("{key}/send"))
        .px_2()
        .py_1()
        .child("Send");
    if sendable {
        let click = cx.listener(move |view, _: &gpui::ClickEvent, window, cx| {
            handle(view, Event::Action("send".into()), window, cx);
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
