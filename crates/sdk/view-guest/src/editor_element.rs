//! Host-native multiline editor recipe.

use crate::{
    wire, Editor, EditorBinding, EditorDocumentUpdate, EditorTransaction, Element, IntoElement,
    Lowering,
};
use gpui::{ElementId, StyleRefinement, Styled};
use std::rc::Rc;

/// A driver-routed event produced by an [`EditorElement`].
pub enum EditorElementEvent<P, M> {
    Document(EditorDocumentUpdate),
    Observed(P),
    Transaction(EditorTransaction<M>),
}

/// A multiline host editor bound to guest-owned [`Editor`] state.
///
/// GPUI core has no editor widget, so lowering emits the host primitive while
/// preserving GPUI identity and style values.
pub struct EditorElement<P, M> {
    id: ElementId,
    editor: Editor,
    document: String,
    binding: EditorBinding<P>,
    route: Rc<dyn Fn(EditorElementEvent<P, M>) -> M>,
    placeholder: String,
    label: Option<String>,
    editable: bool,
    style: StyleRefinement,
    presentation: Option<Box<wire::editor_presentation::EditorPresentation>>,
    rich: Option<Box<wire::editor_rich::RichPresentation>>,
}

impl<P: 'static, M: 'static> EditorElement<P, M> {
    pub fn new(
        id: impl Into<ElementId>,
        editor: &Editor,
        document: impl Into<String>,
        binding: EditorBinding<P>,
        route: impl Fn(EditorElementEvent<P, M>) -> M + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            editor: editor.clone(),
            document: document.into(),
            binding,
            route: Rc::new(route),
            placeholder: String::new(),
            label: None,
            editable: true,
            style: StyleRefinement::default(),
            presentation: None,
            rich: None,
        }
    }

    pub fn placeholder(mut self, value: impl Into<String>) -> Self {
        self.placeholder = value.into();
        self
    }

    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }

    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    pub fn presentation(mut self, value: wire::editor_presentation::EditorPresentation) -> Self {
        self.presentation = Some(Box::new(value));
        self
    }

    pub fn rich_presentation(mut self, value: wire::editor_rich::RichPresentation) -> Self {
        self.rich = Some(Box::new(value));
        self
    }
}

impl<P, M> Styled for EditorElement<P, M> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<P: 'static, M: 'static> Element for EditorElement<P, M> {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            id,
            editor,
            document,
            binding,
            route,
            placeholder,
            label,
            editable,
            style,
            presentation,
            rich,
        } = *self;
        let id = wire::ElementIdWire::from_gpui(id)
            .expect("editor element ID must be portable across the view boundary");
        let context = &lowering.app().inner.slots;
        let document_route = route.clone();
        let (document, on_document) = editor.document(context, document, move |update| {
            document_route(EditorElementEvent::Document(update))
        });
        let observed_route = route.clone();
        let transaction_route = route;
        let binding = binding.register(
            context,
            move |value| observed_route(EditorElementEvent::Observed(value)),
            move |transaction| transaction_route(EditorElementEvent::Transaction(transaction)),
        );
        wire::Node::Editor {
            options: Box::new(wire::EditorOptions {
                rich,
                binding: Some(Box::new(binding)),
                presentation,
            }),
            id,
            style,
            placeholder,
            label,
            document,
            on_document,
            editable,
        }
    }
}

impl<P: 'static, M: 'static> IntoElement for EditorElement<P, M> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<P: 'static, M: 'static> gpui::prelude::FluentBuilder for EditorElement<P, M> {}
