use super::*;

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
