//! Event routes and sent pictures belong to one running driver.
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Weak};

type ClickRoute = Rc<dyn Fn(&gpui::ClickEvent, &mut crate::Window, &mut crate::App)>;
type TooltipRoute = Rc<dyn Fn(&mut crate::Window, &mut crate::App) -> crate::AnyView>;

struct EventRoute<A>(Rc<dyn Fn(&A, &mut crate::Window, &mut crate::App)>);

#[derive(Default)]
struct Tables {
    identity: Arc<()>,
    editor_responses: Vec<crate::wire::EditorResponse>,
    editor_documents: Vec<crate::wire::editor_document::EditorDocumentMessage>,
    editor_sender: Option<crate::wire::editor_document::EditorTransferSender>,
    editor_receiver: Option<(
        crate::wire::editor_document::EditorTransferId,
        crate::wire::editor_document::EditorDocumentRef,
        crate::wire::editor_document::EditorTransferReceiver,
    )>,
    editor_pending: Vec<crate::wire::EditorTransactionId>,
    host: crate::Host,
    mouse_interest: bool,
    event_interest: crate::wire::events::Interest,
    messages: Vec<Rc<dyn Any>>,
    handlers: Vec<Rc<dyn Any>>,
    clicks: Vec<ClickRoute>,
    tooltips: Vec<TooltipRoute>,
    tooltip_responses: Vec<crate::wire::TooltipResponse>,
    pictures: HashSet<(bool, u64)>,
}

#[derive(Clone, Default)]
pub struct Context(Rc<RefCell<Tables>>);

impl Context {
    pub(crate) fn with_macos(_macos: bool) -> Self {
        Self(Rc::new(RefCell::new(Tables {
            ..Tables::default()
        })))
    }

    pub(crate) fn with_host(macos: bool, host: crate::Host) -> Self {
        let context = Self::with_macos(macos);
        context.0.borrow_mut().host = host;
        context
    }

    fn tables(&self) -> Rc<RefCell<Tables>> {
        self.0.clone()
    }

    pub(crate) fn identity(&self) -> Weak<()> {
        Arc::downgrade(&self.0.borrow().identity)
    }
}

/// Returns a picture hash and its bytes the first time this driver sends it.
pub fn picture(context: &Context, bytes: impl AsRef<[u8]>) -> (u64, Option<Vec<u8>>) {
    use std::hash::{Hash, Hasher};
    let bytes = bytes.as_ref();
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let hash = hasher.finish();
    let mut tables = context.0.borrow_mut();
    if tables.pictures.len() >= 4_096 && !tables.pictures.contains(&(false, hash)) {
        tables.pictures.clear();
    }
    let first = tables.pictures.insert((false, hash));
    (hash, first.then(|| bytes.to_vec()))
}

pub(crate) fn clear_pictures(context: &Context) {
    context.0.borrow_mut().pictures.clear();
}

/// Registers a message in the frame currently being built.
pub fn message<M: 'static>(context: &Context, message: M) -> u32 {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let message: Rc<dyn Any> = Rc::new(message);
    let index = u32::try_from(tables.messages.len()).expect("too many message routes");
    tables.messages.push(message);
    index
}

/// A typed handler returns None for a value it cannot route.
pub fn handler<A: 'static, M: 'static>(
    context: &Context,
    handler: Box<dyn Fn(A) -> Option<M>>,
) -> u32 {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let handler: Rc<dyn Any> = Rc::new(handler);
    let index = u32::try_from(tables.handlers.len()).expect("too many handler routes");
    tables.handlers.push(handler);
    index
}

pub(crate) fn reset(context: &Context) {
    let tables = context.tables();
    let old = {
        let mut tables = tables.borrow_mut();
        (
            std::mem::take(&mut tables.messages),
            std::mem::take(&mut tables.handlers),
            std::mem::take(&mut tables.clicks),
            std::mem::take(&mut tables.tooltips),
        )
    };
    drop(old);
}

pub(crate) fn tooltip(
    context: &Context,
    build: Box<dyn Fn(&mut crate::Window, &mut crate::App) -> crate::AnyView + 'static>,
) -> u32 {
    let mut tables = context.0.borrow_mut();
    let index = u32::try_from(tables.tooltips.len()).expect("too many tooltip routes");
    tables.tooltips.push(Rc::from(build));
    index
}

pub(crate) fn tooltip_route(context: &Context, index: u32) -> Option<TooltipRoute> {
    context.0.borrow().tooltips.get(index as usize).cloned()
}

pub(crate) fn tooltip_response(context: &Context, response: crate::wire::TooltipResponse) {
    context.0.borrow_mut().tooltip_responses.push(response);
}

pub(crate) fn take_tooltip_responses(context: &Context) -> Vec<crate::wire::TooltipResponse> {
    std::mem::take(&mut context.0.borrow_mut().tooltip_responses)
}

pub(crate) fn take_message<M: Clone + 'static>(context: &Context, index: u32) -> Option<M> {
    let tables = context.tables();
    let message = {
        let tables = tables.borrow();
        tables.messages.get(index as usize).cloned()?
    };
    message.downcast_ref::<M>().cloned()
}

pub(crate) fn run_handler<A: 'static, M: 'static>(
    context: &Context,
    index: u32,
    value: A,
) -> Option<M> {
    let tables = context.tables();
    let handler = {
        let tables = tables.borrow();
        tables.handlers.get(index as usize).cloned()?
    };
    handler.downcast_ref::<Box<dyn Fn(A) -> Option<M>>>()?(value)
}

pub(crate) fn route<A: 'static>(
    context: &Context,
    listener: impl Fn(&A, &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let index = u32::try_from(tables.handlers.len()).expect("too many handler routes");
    tables
        .handlers
        .push(Rc::new(EventRoute::<A>(Rc::new(listener))));
    index
}

pub(crate) fn message_route(
    context: &Context,
    listener: impl Fn(&(), &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let index = u32::try_from(tables.messages.len()).expect("too many message routes");
    tables
        .messages
        .push(Rc::new(EventRoute::<()>(Rc::new(listener))));
    index
}

pub(crate) fn run_route<A: 'static>(
    context: &Context,
    index: u32,
    event: &A,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let tables = context.tables();
    let handler = {
        let tables = tables.borrow();
        tables.handlers.get(index as usize).cloned()
    };
    let Some(route) =
        handler.and_then(|route| route.downcast_ref::<EventRoute<A>>().map(|r| r.0.clone()))
    else {
        return false;
    };
    route(event, window, app);
    true
}

pub(crate) fn run_message_route(
    context: &Context,
    index: u32,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let tables = context.tables();
    let route = {
        let tables = tables.borrow();
        tables
            .messages
            .get(index as usize)
            .and_then(|route| route.downcast_ref::<EventRoute<()>>())
            .map(|route| route.0.clone())
    };
    let Some(route) = route else { return false };
    route(&(), window, app);
    true
}

pub(crate) fn event_interest(context: &Context) -> crate::wire::events::Interest {
    context.0.borrow().event_interest
}

pub(crate) fn mouse_interest(context: &Context) -> bool {
    context.0.borrow().mouse_interest
}

pub(crate) fn click(
    context: &Context,
    listener: impl Fn(&gpui::ClickEvent, &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let mut tables = context.0.borrow_mut();
    let index = u32::try_from(tables.clicks.len()).expect("too many click routes");
    tables.clicks.push(Rc::new(listener));
    index
}

pub(crate) fn run_click(
    context: &Context,
    index: u32,
    event: &gpui::ClickEvent,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let listener = context.0.borrow().clicks.get(index as usize).cloned();
    if let Some(listener) = listener {
        listener(event, window, app);
        true
    } else {
        false
    }
}

pub(crate) fn editor_response(context: &Context, response: crate::wire::EditorResponse) {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    tables.editor_pending.retain(|id| {
        !(id.instance == response.id.instance
            && id.document == response.id.document
            && id.sequence == response.id.sequence)
    });
    tables.editor_pending.push(response.id.clone());
    tables.editor_responses.push(response);
}
/// A native commit may have no decision, but an outstanding decision must
/// match its complete attempt/version before any state or route is accepted.
pub(crate) fn editor_request_current(
    context: &Context,
    id: &crate::wire::EditorTransactionId,
) -> bool {
    context.0.borrow().editor_pending.iter().all(|pending| {
        pending.instance != id.instance
            || pending.document != id.document
            || pending.sequence != id.sequence
            || pending.attempt <= id.attempt
    })
}
pub(crate) fn editor_matches_pending(
    context: &Context,
    id: &crate::wire::EditorTransactionId,
) -> bool {
    context.0.borrow().editor_pending.iter().all(|pending| {
        pending.instance != id.instance
            || pending.document != id.document
            || pending.sequence != id.sequence
            || pending == id
    })
}
pub(crate) fn editor_acknowledge(context: &Context, event: &crate::wire::EditorTransactionEvent) {
    use crate::wire::EditorTransactionEvent;
    let id = match event {
        EditorTransactionEvent::Interaction { id, .. }
        | EditorTransactionEvent::Commit { id, .. }
        | EditorTransactionEvent::Fault { id, .. }
        | EditorTransactionEvent::Cancelled { id, .. } => id,
    };
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    tables.editor_pending.retain(|pending| pending != id);
    tables
        .editor_responses
        .retain(|response| &response.id != id);
}
pub(crate) fn request_editor_mirror(
    context: &Context,
    request: &crate::wire::EditorRequest,
) -> Result<(), crate::wire::editor_document::EditorTransferError> {
    use crate::wire::editor_document::{
        EditorDocumentMessage, EditorTransferError, EditorTransferId, EditorTransferReceiver,
    };
    let id = EditorTransferId {
        instance: request.id.instance,
        document: request.id.document.clone(),
        reset: request.id.reset,
        serial: request.id.sequence,
        attempt: request.id.attempt,
    };
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if tables.editor_sender.is_some() || !tables.editor_documents.is_empty() {
        return Err(EditorTransferError::Limit);
    }
    if let Some((current, target, _)) = &tables.editor_receiver {
        return if current == &id && target == &request.state {
            Ok(())
        } else {
            Err(EditorTransferError::Identity)
        };
    }
    let receiver = EditorTransferReceiver::new(id.clone(), request.state.clone())?;
    tables.editor_receiver = Some((id.clone(), request.state.clone(), receiver));
    tables.editor_pending.retain(|pending| {
        !(pending.instance == request.id.instance
            && pending.document == request.id.document
            && pending.sequence == request.id.sequence)
    });
    tables.editor_pending.push(request.id.clone());
    tables
        .editor_documents
        .push(EditorDocumentMessage::Request {
            id,
            target: request.state.clone(),
        });
    Ok(())
}

pub(crate) fn receive_editor_mirror(
    context: &Context,
    transfer: &crate::wire::editor_document::EditorTransfer,
) -> Result<
    Option<(String, crate::wire::editor_document::EditorDocumentRef)>,
    crate::wire::editor_document::EditorTransferError,
> {
    use crate::wire::editor_document::EditorTransferError;
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let Some((id, target, receiver)) = &mut tables.editor_receiver else {
        return Err(EditorTransferError::Identity);
    };
    if transfer.id() != id {
        return Err(EditorTransferError::Identity);
    }
    let text = match receiver.receive(transfer) {
        Ok(text) => text,
        Err(error) => {
            tables.editor_receiver = None;
            return Err(error);
        }
    };
    if let Some(text) = text {
        let target = target.clone();
        tables.editor_receiver = None;
        Ok(Some((text, target)))
    } else {
        Ok(None)
    }
}

pub(crate) fn acknowledge_editor_mirror(
    context: &Context,
    id: crate::wire::editor_document::EditorTransferId,
) {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if tables.editor_documents.is_empty() {
        tables
            .editor_documents
            .push(crate::wire::editor_document::EditorDocumentMessage::Acknowledged { id });
    }
}

pub(crate) fn start_editor_transfer(
    context: &Context,
    id: crate::wire::editor_document::EditorTransferId,
    target: crate::wire::editor_document::EditorDocumentRef,
) -> Result<(), crate::wire::editor_document::EditorTransferError> {
    use crate::wire::editor_document::{EditorTransferError, EditorTransferSender};
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if let Some(sender) = &tables.editor_sender {
        return if sender.id() == &id {
            Ok(())
        } else {
            Err(EditorTransferError::Limit)
        };
    }
    tables.editor_sender = Some(EditorTransferSender::new(id, target)?);
    Ok(())
}

pub(crate) fn editor_document_frame(
    context: &Context,
    reference: &crate::wire::editor_document::EditorDocumentRef,
    text: &str,
) {
    use crate::wire::editor_document::EditorDocumentMessage;
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if !tables.editor_documents.is_empty() {
        return;
    }
    let Some(sender) = &mut tables.editor_sender else {
        return;
    };
    if sender.id().document != reference.document {
        return;
    }
    let message = match sender.next_frame(reference, text) {
        Ok(Some(transfer)) => EditorDocumentMessage::Transfer(transfer),
        Ok(None) => return,
        Err(reason) => EditorDocumentMessage::Failed {
            id: sender.id().clone(),
            reason,
        },
    };
    tables.editor_documents.push(message);
}

pub(crate) fn editor_document_failure(
    context: &Context,
    id: crate::wire::editor_document::EditorTransferId,
    reason: crate::wire::editor_document::EditorTransferError,
) {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if tables.editor_documents.is_empty() {
        tables
            .editor_documents
            .push(crate::wire::editor_document::EditorDocumentMessage::Failed { id, reason });
    }
}

pub(crate) fn finish_editor_transfer(
    context: &Context,
    id: &crate::wire::editor_document::EditorTransferId,
) {
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    if tables
        .editor_receiver
        .as_ref()
        .is_some_and(|(current, _, _)| current == id)
    {
        tables.editor_receiver = None;
    }
    if tables
        .editor_sender
        .as_ref()
        .is_some_and(|sender| sender.id() == id)
    {
        tables.editor_sender = None;
    }
}

pub(crate) fn editor_transferring(context: &Context) -> bool {
    {
        let tables = context.0.borrow();
        tables.editor_sender.is_some() || tables.editor_receiver.is_some()
    }
}

pub(crate) fn take_editor_documents(
    context: &Context,
) -> Vec<crate::wire::editor_document::EditorDocumentMessage> {
    std::mem::take(&mut context.0.borrow_mut().editor_documents)
}
pub(crate) fn take_editor_responses(context: &Context) -> Vec<crate::wire::EditorResponse> {
    use crate::wire::editor_transaction::{MAX_EDITOR_PATCH_BYTES, MAX_EDITOR_RESPONSES};
    let tables = context.tables();
    let mut tables = tables.borrow_mut();
    let mut bytes = 0usize;
    let mut count = 0;
    for response in tables.editor_responses.iter().take(MAX_EDITOR_RESPONSES) {
        let replacement_bytes = match &response.decision {
            crate::wire::EditorDecision::Apply { patches, .. } => {
                patches.iter().fold(0usize, |sum, patch| {
                    sum.saturating_add(patch.replacement.len())
                })
            }
            _ => 0,
        };
        if count > 0 && replacement_bytes > MAX_EDITOR_PATCH_BYTES.saturating_sub(bytes) {
            break;
        }
        // An invalid single response still reaches the strict host decoder;
        // it must not strand the outbox forever or silently become a fallback.
        bytes = bytes.saturating_add(replacement_bytes);
        count += 1;
    }
    tables.editor_responses.drain(..count).collect()
}
pub(crate) fn editor_responses_ready(context: &Context) -> bool {
    !context.0.borrow().editor_responses.is_empty()
}
pub(crate) fn editor_pending(context: &Context) -> bool {
    !context.0.borrow().editor_pending.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_contexts_restore_typed_routes_and_picture_history() {
        let first = Context::default();
        let first_route =
            handler::<String, String>(&first, Box::new(|text| Some(format!("first:{text}"))));
        assert!(picture(&first, b"svg").1.is_some());
        {
            let second = Context::default();
            let route =
                handler::<String, String>(&second, Box::new(|text| Some(format!("second:{text}"))));
            assert_eq!(route, 0, "each driver starts its own typed route table");
            assert_eq!(
                run_handler::<String, String>(&second, route, "x".into()).as_deref(),
                Some("second:x")
            );
            assert!(
                picture(&second, b"svg").1.is_some(),
                "a new host needs its own picture bytes"
            );
        }
        assert_eq!(
            run_handler::<String, String>(&first, first_route, "x".into()).as_deref(),
            Some("first:x")
        );
        assert!(
            picture(&first, b"svg").1.is_none(),
            "returning to the first driver preserves its picture history"
        );
    }

    #[test]
    fn clearing_picture_history_resends_content() {
        let context = Context::default();
        assert!(picture(&context, b"image").1.is_some());
        assert!(picture(&context, b"image").1.is_none());
        clear_pictures(&context);
        assert_eq!(
            picture(&context, b"image").1.as_deref(),
            Some(b"image".as_slice())
        );
    }
}

#[cfg(test)]
mod response_budget_tests {
    use super::*;
    use crate::wire::{
        self, EditorDecision, EditorHistoryEffect, EditorPatch, EditorResponse, EditorTransactionId,
    };

    #[test]
    fn independent_large_responses_cross_decodable_frames_without_losing_identity() {
        let context = Context::default();
        for sequence in 1..=2 {
            editor_response(
                &context,
                EditorResponse {
                    id: EditorTransactionId {
                        instance: 1,
                        document: format!("app:doc{sequence}"),
                        reset: 0,
                        sequence,
                        attempt: 1,
                        text_revision: 0,
                        revision: 0,
                    },
                    decision: EditorDecision::Apply {
                        patches: vec![EditorPatch {
                            start_byte: 0,
                            end_byte: 0,
                            replacement: "x"
                                .repeat(wire::editor_transaction::MAX_EDITOR_PATCH_BYTES),
                        }],
                        cursor: Default::default(),
                        history: EditorHistoryEffect::NewGroup,
                    },
                },
            );
        }
        for sequence in 1..=2 {
            let frame = wire::Frame {
                editor_decisions: take_editor_responses(&context),
                ..Default::default()
            };
            assert_eq!(
                frame.editor_decisions.len(),
                1,
                "one complete response per aggregate byte budget"
            );
            assert_eq!(frame.editor_decisions[0].id.sequence, sequence);
            assert!(wire::decode::<wire::Frame>(&wire::encode(&frame)).is_ok());
            assert_eq!(
                context.0.borrow().editor_responses.len(),
                (2 - sequence) as usize
            );
            assert!(
                editor_pending(&context),
                "sent responses remain outstanding until their commits"
            );
        }
        assert!(take_editor_responses(&context).is_empty());
    }
}

pub(crate) fn host(context: &Context) -> crate::Host {
    context.0.borrow().host.clone()
}
