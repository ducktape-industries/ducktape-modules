//! Document delivery is routed through the generated mutable Editor binding.
use crate::{slots, wire, Editor};
use wire::editor_document::{EditorDocumentMessage, EditorDocumentRef, EditorTransferError};

#[derive(Clone)]
pub struct EditorDocumentUpdate {
    document: String,
    message: EditorDocumentMessage,
    context: slots::Context,
}

impl EditorDocumentUpdate {
    pub fn apply(self, editor: &mut Editor) {
        let id = self.message.id().clone();
        if id.document != self.document {
            return;
        }
        let result = match self.message {
            EditorDocumentMessage::Request { id, target } => {
                let current = editor.document_reference(self.document);
                if target != current {
                    Err(EditorTransferError::Identity)
                } else {
                    slots::start_editor_transfer(&self.context, id, target)
                }
            }
            EditorDocumentMessage::Acknowledged { .. } | EditorDocumentMessage::Failed { .. } => {
                slots::finish_editor_transfer(&self.context, &id);
                Ok(())
            }
            EditorDocumentMessage::Transfer(transfer) => {
                match slots::receive_editor_mirror(&self.context, &transfer) {
                    Ok(Some((text, target))) => {
                        if editor.install_mirror(text, &target) {
                            slots::acknowledge_editor_mirror(&self.context, id.clone());
                            Ok(())
                        } else {
                            Err(EditorTransferError::Identity)
                        }
                    }
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                }
            }
        };
        if let Err(reason) = result {
            slots::editor_document_failure(&self.context, id, reason);
        }
    }
}

impl Editor {
    /// Generated code calls this for every projection. The mirror stays owned
    /// by application state; routes and transfer progress retain only identity.
    pub(crate) fn document<M: 'static>(
        &self,
        context: &slots::Context,
        document: String,
        wrap: impl Fn(EditorDocumentUpdate) -> M + 'static,
    ) -> (EditorDocumentRef, u32) {
        let reference = self.document_reference(document.clone());
        slots::editor_document_frame(context, &reference, self.text_ref());
        let context = context.clone();
        let route_context = context.clone();
        let handler = slots::handler::<EditorDocumentMessage, M>(
            &route_context,
            Box::new(move |message| {
                Some(wrap(EditorDocumentUpdate {
                    document: document.clone(),
                    message,
                    context: context.clone(),
                }))
            }),
        );
        (reference, handler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, Driver, EditorBinding, EditorElement, EditorElementEvent, Render, View, Window,
    };
    use serde::{Deserialize, Serialize};
    use std::rc::Rc;
    use wire::editor_document::{EditorTransfer, EditorTransferId};

    #[derive(Serialize, Deserialize)]
    struct DocumentApp {
        #[serde(with = "editor_snapshot")]
        editor: Editor,
    }
    mod editor_snapshot {
        use super::*;
        pub fn serialize<S: serde::Serializer>(
            editor: &Editor,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            editor.snapshot().serialize(serializer)
        }
        pub fn deserialize<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Editor, D::Error> {
            let bytes = Vec::<u8>::deserialize(deserializer)?;
            Editor::restore(&bytes).ok_or_else(|| serde::de::Error::custom("invalid editor"))
        }
    }
    impl View for DocumentApp {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                editor: Editor::new("x".repeat(wire::MAX_STRING_BYTES)),
            }
        }
    }
    impl Render for DocumentApp {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
            EditorElement::new(
                "draft",
                &self.editor,
                "app:draft",
                EditorBinding::plain(),
                |event| -> crate::context::Callback<Self> {
                    match event {
                        EditorElementEvent::Document(update) => Rc::new(move |view, _, _| {
                            update.clone().apply(&mut view.editor);
                        }),
                        EditorElementEvent::Observed(()) => Rc::new(|_, _, _| {}),
                        EditorElementEvent::Transaction(transaction) => {
                            Rc::new(move |view, _, cx| {
                                transaction.clone().apply(&mut view.editor, cx);
                            })
                        }
                    }
                },
            )
        }
    }

    #[test]
    fn editor_view_progresses_without_messages_and_waits_for_exact_ack() {
        let mut driver = Driver::<DocumentApp>::new();
        let first = driver.tick(vec![]);
        let wire::Node::Editor { on_document, .. } = first.root.unwrap() else {
            panic!("document view must render an editor")
        };
        let target = driver
            .entity()
            .read(|view| view.editor.document_reference("app:draft".into()));
        let id = EditorTransferId {
            instance: 9,
            document: target.document.clone(),
            reset: target.reset,
            serial: 4,
            attempt: 0,
        };
        let begin = driver.tick(vec![wire::Event::EditorDocument {
            handler: on_document,
            message: EditorDocumentMessage::Request {
                id: id.clone(),
                target,
            },
        }]);
        assert!(matches!(
            &begin.editor_documents[..],
            [EditorDocumentMessage::Transfer(
                EditorTransfer::Begin { .. }
            )]
        ));
        assert!(driver.snapshot().is_err());
        let chunk = driver.tick(vec![]);
        assert!(
            matches!(&chunk.editor_documents[..], [EditorDocumentMessage::Transfer(EditorTransfer::Chunk { index: 0, bytes, .. })] if bytes.len() == wire::MAX_STRING_BYTES),
            "unchanged view must produce the next bounded chunk"
        );
        let complete = driver.tick(vec![]);
        assert!(matches!(
            &complete.editor_documents[..],
            [EditorDocumentMessage::Transfer(
                EditorTransfer::Complete { .. }
            )]
        ));
        assert!(
            driver.snapshot().is_err(),
            "Complete is not a receiver acknowledgment"
        );
        let mut stale = id.clone();
        stale.serial -= 1;
        driver.tick(vec![wire::Event::EditorDocument {
            handler: u32::MAX,
            message: EditorDocumentMessage::Acknowledged { id: stale },
        }]);
        assert!(
            driver.snapshot().is_err(),
            "a stale acknowledgment cannot release current source progress"
        );
        driver.tick(vec![wire::Event::EditorDocument {
            handler: u32::MAX,
            message: EditorDocumentMessage::Acknowledged { id },
        }]);
        assert!(driver.snapshot().is_ok());
        assert!(driver.tick(vec![]).editor_documents.is_empty());
    }
}
