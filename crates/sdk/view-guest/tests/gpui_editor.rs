use serde::{Deserialize, Serialize};
use view_guest::{
    wire, Context, Driver, Editor, EditorBinding, EditorElement, EditorElementEvent, ElementId,
    Render, Styled, View, Window,
};

#[derive(Serialize, Deserialize)]
struct EditorView {
    #[serde(with = "editor_snapshot")]
    editor: Editor,
}

mod editor_snapshot {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        editor: &Editor,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&editor.snapshot())
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Editor, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        Editor::restore(&bytes).ok_or_else(|| serde::de::Error::custom("invalid editor"))
    }
}

impl View for EditorView {
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            editor: Editor::new("hello"),
        }
    }
}

impl Render for EditorView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl view_guest::IntoElement {
        let binding = EditorBinding::new(
            Vec::new(),
            |_| wire::EditorDecision::DefaultEditorAction,
            |_| None::<()>,
        );
        EditorElement::new(
            ElementId::Name("draft".into()),
            &self.editor,
            "app:draft",
            binding,
            |event| match event {
                EditorElementEvent::Document(_) => (),
                EditorElementEvent::Observed(()) => (),
                EditorElementEvent::Transaction(_) => (),
            },
        )
        .placeholder("Write a message")
        .label("Message")
        .w_full()
    }
}

#[test]
fn editor_element_lowers_identity_style_and_document_without_author_routes() {
    let mut driver = Driver::<EditorView>::new();
    let frame = driver.tick(Vec::new());
    let wire::Node::Editor {
        id,
        style,
        placeholder,
        label,
        document,
        options,
        ..
    } = frame.root.unwrap()
    else {
        panic!("editor element must lower to the host editor primitive")
    };
    assert_eq!(id, wire::ElementIdWire::Name("draft".into()));
    assert!(style.size.width.is_some());
    assert_eq!(placeholder, "Write a message");
    assert_eq!(label.as_deref(), Some("Message"));
    assert_eq!(document.document, "app:draft");
    assert_eq!(document.byte_len, 5);
    assert!(options.binding.is_some());
}
