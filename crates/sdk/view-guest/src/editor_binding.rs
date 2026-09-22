//! Guest-local decisions borrow the canonical document owned by application state.
use crate::{slots, wire, Editor};
use std::rc::Rc;

pub use wire::EditorDecision;

#[derive(Clone, Copy, Debug)]
pub struct EditorStateView<'a> {
    pub text: &'a str,
    pub cursor: wire::EditorCursor,
    pub reset: u64,
    pub text_revision: u64,
    pub revision: u64,
}
impl<'a> EditorStateView<'a> {
    fn new(text: &'a str, reference: &wire::editor_document::EditorDocumentRef) -> Self {
        Self {
            text,
            cursor: reference.cursor,
            reset: reference.reset,
            text_revision: reference.text_revision,
            revision: reference.revision,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct EditorKeyRequest<'a> {
    pub id: &'a wire::EditorTransactionId,
    pub state: EditorStateView<'a>,
    pub key: &'a wire::keyboard::KeyState,
    pub repeat: bool,
    pub input_time_ms: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct EditorInteractionRequest<'a> {
    pub id: &'a wire::EditorTransactionId,
    pub state: EditorStateView<'a>,
    pub action: &'a wire::editor_presentation::EditorInteraction,
    pub input_time_ms: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct EditorRichRequest<'a> {
    pub id: &'a wire::EditorTransactionId,
    pub state: EditorStateView<'a>,
    pub edit: &'a wire::editor_rich::RichEdit,
    pub input_time_ms: u64,
}
#[derive(Clone, Copy, Debug)]
pub enum EditorTransactionEvent<'a> {
    Interaction {
        id: &'a wire::EditorTransactionId,
        state: EditorStateView<'a>,
        action: &'a wire::editor_presentation::EditorInteraction,
        input_time_ms: u64,
    },
    Commit {
        id: &'a wire::EditorTransactionId,
        origin: Option<&'a wire::EditorRequestInput>,
        before: EditorStateView<'a>,
        after: EditorStateView<'a>,
        kind: wire::EditorEditKind,
        history: wire::EditorHistoryEffect,
        input_time_ms: u64,
    },
    Fault {
        id: &'a wire::EditorTransactionId,
        reason: wire::EditorFault,
    },
    Cancelled {
        id: &'a wire::EditorTransactionId,
    },
}

type Decide = Rc<dyn for<'a> Fn(EditorKeyRequest<'a>) -> EditorDecision>;
type Interact = Rc<dyn for<'a> Fn(EditorInteractionRequest<'a>) -> EditorDecision>;
type Rich = Rc<dyn for<'a> Fn(EditorRichRequest<'a>) -> EditorDecision>;
type Observe<P> = Rc<dyn for<'a> Fn(EditorTransactionEvent<'a>) -> Option<P>>;
pub struct EditorBinding<P> {
    authored: bool,
    claims: Vec<wire::EditorKeyClaim>,
    decide: Decide,
    interact: Option<Interact>,
    rich: Option<Rich>,
    on_event: Observe<P>,
}
struct Callbacks<M> {
    decide: Decide,
    interact: Option<Interact>,
    rich: Option<Rich>,
    on_event: Observe<M>,
}
impl<P: 'static> EditorBinding<P> {
    pub fn new(
        claims: Vec<wire::EditorKeyClaim>,
        decide: impl for<'a> Fn(EditorKeyRequest<'a>) -> EditorDecision + 'static,
        on_event: impl for<'a> Fn(EditorTransactionEvent<'a>) -> Option<P> + 'static,
    ) -> Self {
        assert!(
            claims.len() <= wire::editor_transaction::MAX_EDITOR_CLAIMS,
            "editor claim limit"
        );
        Self {
            authored: true,
            claims,
            decide: Rc::new(decide),
            interact: None,
            rich: None,
            on_event: Rc::new(on_event),
        }
    }
    pub fn on_rich_edit(
        mut self,
        decide: impl for<'a> Fn(EditorRichRequest<'a>) -> EditorDecision + 'static,
    ) -> Self {
        self.rich = Some(Rc::new(decide));
        self
    }
    pub fn on_interaction(
        mut self,
        decide: impl for<'a> Fn(EditorInteractionRequest<'a>) -> EditorDecision + 'static,
    ) -> Self {
        self.interact = Some(Rc::new(decide));
        self
    }
    pub(crate) fn register<M: 'static>(
        self,
        context: &slots::Context,
        route: impl Fn(P) -> M + 'static,
        wrap: impl Fn(EditorTransaction<M>) -> M + 'static,
    ) -> wire::EditorBinding {
        let observe = self.on_event;
        let callbacks = Rc::new(Callbacks {
            decide: self.decide,
            interact: self.interact,
            rich: self.rich,
            on_event: Rc::new(move |event| observe(event).map(&route)),
        });
        // Existing handler storage already supplies bounded frame-local lifetime
        // and memo capture. No second callback registry or copied document.
        let map = slots::handler::<(), Rc<Callbacks<M>>>(
            context,
            Box::new(move |()| Some(callbacks.clone())),
        );
        let identity = context.identity();
        let wrap = Rc::new(wrap);
        let request_wrap = wrap.clone();
        let request_identity = identity.clone();
        let on_request = slots::handler::<wire::EditorRequest, M>(
            context,
            Box::new(move |request| {
                Some(request_wrap(EditorTransaction {
                    event: Transaction::Request(request),
                    map,
                    identity: request_identity.clone(),
                    message: std::marker::PhantomData,
                }))
            }),
        );
        let on_event = slots::handler::<wire::EditorTransactionEvent, M>(
            context,
            Box::new(move |event| {
                Some(wrap(EditorTransaction {
                    event: Transaction::Event(event),
                    map,
                    identity: identity.clone(),
                    message: std::marker::PhantomData,
                }))
            }),
        );
        wire::EditorBinding {
            authored: self.authored,
            claims: self.claims,
            on_request,
            on_event,
        }
    }
}
impl EditorBinding<()> {
    pub fn plain() -> Self {
        let mut binding = EditorBinding::new(
            Vec::new(),
            |_| EditorDecision::DefaultEditorAction,
            |_| None,
        );
        binding.authored = false;
        binding
    }
}
#[derive(Clone, Debug)]
enum Transaction {
    Request(wire::EditorRequest),
    Event(wire::EditorTransactionEvent),
}
pub struct EditorTransaction<M> {
    event: Transaction,
    map: u32,
    identity: std::sync::Weak<()>,
    message: std::marker::PhantomData<fn() -> M>,
}
impl<M> Clone for EditorTransaction<M> {
    fn clone(&self) -> Self {
        Self {
            event: self.event.clone(),
            map: self.map,
            identity: self.identity.clone(),
            message: std::marker::PhantomData,
        }
    }
}
impl<M> std::fmt::Debug for EditorTransaction<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EditorTransaction")
            .field(&self.event)
            .finish()
    }
}
impl<M: 'static> EditorTransaction<M> {
    pub fn apply(self, editor: &mut Editor, cx: &mut crate::App) -> Option<M> {
        self.apply_in(editor, &cx.inner.slots)
    }

    fn apply_in(self, editor: &mut Editor, context: &slots::Context) -> Option<M> {
        if !std::sync::Weak::ptr_eq(&self.identity, &context.identity())
            || self.identity.upgrade().is_none()
        {
            return None;
        }
        let callbacks = slots::run_handler::<(), Rc<Callbacks<M>>>(context, self.map, ())?;
        match self.event {
            Transaction::Request(request) => {
                if !slots::editor_request_current(context, &request.id) {
                    return None;
                }
                if editor.document_reference(request.id.document.clone()) != request.state {
                    if request.state.reset == editor.reset_revision() {
                        if let Err(reason) = slots::request_editor_mirror(context, &request) {
                            slots::editor_document_failure(
                                context,
                                wire::editor_document::EditorTransferId {
                                    instance: request.id.instance,
                                    document: request.id.document.clone(),
                                    reset: request.id.reset,
                                    serial: request.id.sequence,
                                    attempt: request.id.attempt,
                                },
                                reason,
                            );
                        }
                    }
                    return None;
                }
                let state = EditorStateView::new(editor.text_ref(), &request.state);
                let decision = match &request.input {
                    wire::EditorRequestInput::RichEdit { edit } => callbacks
                        .rich
                        .as_ref()
                        .filter(|_| {
                            edit.document.validate().is_ok()
                                && edit
                                    .before
                                    .as_ref()
                                    .is_none_or(|before| before.validate().is_ok())
                        })
                        .map_or(EditorDecision::Noop, |decide| {
                            decide(EditorRichRequest {
                                id: &request.id,
                                state,
                                edit,
                                input_time_ms: request.input_time_ms,
                            })
                        }),
                    wire::EditorRequestInput::Key { key, repeat } => {
                        (callbacks.decide)(EditorKeyRequest {
                            id: &request.id,
                            state,
                            key,
                            repeat: *repeat,
                            input_time_ms: request.input_time_ms,
                        })
                    }
                    wire::EditorRequestInput::Interaction { action } => callbacks
                        .interact
                        .as_ref()
                        .map_or(EditorDecision::Noop, |decide| {
                            decide(EditorInteractionRequest {
                                id: &request.id,
                                state,
                                action,
                                input_time_ms: request.input_time_ms,
                            })
                        }),
                };
                slots::editor_response(
                    context,
                    wire::EditorResponse {
                        id: request.id,
                        decision,
                    },
                );
                None
            }
            Transaction::Event(event) => {
                let id = match &event {
                    wire::EditorTransactionEvent::Interaction { id, .. }
                    | wire::EditorTransactionEvent::Commit { id, .. }
                    | wire::EditorTransactionEvent::Fault { id, .. }
                    | wire::EditorTransactionEvent::Cancelled { id, .. } => id,
                };
                if !slots::editor_matches_pending(context, id) {
                    return None;
                }
                let mapped = match &event {
                    wire::EditorTransactionEvent::Interaction {
                        state,
                        action,
                        input_time_ms,
                        ..
                    } => {
                        if editor.document_reference(id.document.clone()) != *state {
                            return None;
                        }
                        (callbacks.on_event)(EditorTransactionEvent::Interaction {
                            id,
                            state: EditorStateView::new(editor.text_ref(), state),
                            action,
                            input_time_ms: *input_time_ms,
                        })
                    }
                    wire::EditorTransactionEvent::Commit {
                        origin,
                        before,
                        after,
                        patches,
                        kind,
                        history,
                        input_time_ms,
                        ..
                    } => {
                        if before.document != id.document
                            || before.reset != id.reset
                            || before.text_revision != id.text_revision
                            || before.revision != id.revision
                        {
                            return None;
                        }
                        let old = editor.accept_patch(before, after, patches)?;
                        (callbacks.on_event)(EditorTransactionEvent::Commit {
                            id,
                            origin: origin.as_ref(),
                            before: EditorStateView::new(
                                old.as_deref().unwrap_or_else(|| editor.text_ref()),
                                before,
                            ),
                            after: EditorStateView::new(editor.text_ref(), after),
                            kind: *kind,
                            history: *history,
                            input_time_ms: *input_time_ms,
                        })
                    }
                    wire::EditorTransactionEvent::Fault { state, reason, .. } => {
                        if state.document != id.document
                            || state.reset != id.reset
                            || state.reset != editor.reset_revision()
                        {
                            return None;
                        }
                        (callbacks.on_event)(EditorTransactionEvent::Fault {
                            id,
                            reason: *reason,
                        })
                    }
                    wire::EditorTransactionEvent::Cancelled { state, .. } => {
                        if state.document != id.document || state.reset != id.reset {
                            return None;
                        }
                        (callbacks.on_event)(EditorTransactionEvent::Cancelled { id })
                    }
                };
                slots::editor_acknowledge(context, &event);
                mapped
            }
        }
    }
}

#[cfg(test)]
mod tests;
