use super::*;
use std::cell::Cell;
fn id(attempt: u32) -> wire::EditorTransactionId {
    wire::EditorTransactionId {
        instance: 1,
        document: "app:draft".into(),
        reset: 0,
        sequence: 7,
        attempt,
        text_revision: 0,
        revision: 0,
    }
}
fn observer(context: &slots::Context, calls: Rc<Cell<usize>>) -> u32 {
    let callbacks = Rc::new(Callbacks::<()> {
        decide: Rc::new(|_| EditorDecision::Noop),
        interact: None,
        rich: None,
        on_event: Rc::new(move |_| {
            calls.set(calls.get() + 1);
            None
        }),
    });
    slots::handler::<(), Rc<Callbacks<()>>>(context, Box::new(move |()| Some(callbacks.clone())))
}
fn transaction(
    context: &slots::Context,
    event: wire::EditorTransactionEvent,
    map: u32,
) -> EditorTransaction<()> {
    EditorTransaction {
        event: Transaction::Event(event),
        map,
        identity: context.identity(),
        message: std::marker::PhantomData,
    }
}
fn commit(
    id: wire::EditorTransactionId,
    before: &Editor,
    text: &str,
) -> wire::EditorTransactionEvent {
    let reference = before.document_reference(id.document.clone());
    let mut after = reference.clone();
    after.byte_len = text.len() as u32;
    after.revision += 1;
    after.text_revision += u64::from(before.text_ref() != text);
    after.cursor.clamp(text);
    wire::EditorTransactionEvent::Commit {
        id,
        origin: None,
        before: reference,
        after,
        patches: wire::editor_document::editor_changed_span(before.text_ref(), text).unwrap(),
        kind: wire::EditorEditKind::GuestPatch,
        history: wire::EditorHistoryEffect::NewGroup,
        input_time_ms: 1,
    }
}
#[test]
fn a_large_caret_commit_borrows_one_canonical_text_for_both_history_views() {
    let context = slots::Context::default();
    let mut editor = Editor::new("x".repeat(wire::editor_document::MAX_EDITOR_DOCUMENT_BYTES));
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let callbacks = Rc::new(Callbacks::<()> {
        decide: Rc::new(|_| EditorDecision::Noop),
        interact: None,
        rich: None,
        on_event: Rc::new(move |event| {
            let EditorTransactionEvent::Commit { before, after, .. } = event else {
                panic!("expected caret commit");
            };
            assert_eq!(
                    before.text.as_ptr(),
                    after.text.as_ptr(),
                    "metadata-only commit must borrow the same canonical text, not copy/compare one MiB"
                );
            assert_ne!(
                before.cursor, after.cursor,
                "before/after selection metadata stays distinct"
            );
            seen.set(seen.get() + 1);
            None
        }),
    });
    let map = slots::handler::<(), Rc<Callbacks<()>>>(
        &context,
        Box::new(move |()| Some(callbacks.clone())),
    );
    let before = editor.document_reference("app:draft".into());
    let mut after = before.clone();
    after.revision += 1;
    after.cursor.position.column = 1;
    transaction(
        &context,
        wire::EditorTransactionEvent::Commit {
            id: id(1),
            origin: None,
            before,
            after,
            patches: vec![],
            kind: wire::EditorEditKind::Cursor,
            history: wire::EditorHistoryEffect::Native,
            input_time_ms: 42,
        },
        map,
    )
    .apply_in(&mut editor, &context);
    assert_eq!(calls.get(), 1);
    assert_eq!(editor.cursor().position.column, 1);
}

#[test]
fn cancellation_after_reset_notifies_without_replacing_the_new_document() {
    let context = slots::Context::default();
    let mut editor = Editor::new("old");
    let current = id(1);
    slots::editor_response(
        &context,
        wire::EditorResponse {
            id: current.clone(),
            decision: EditorDecision::Noop,
        },
    );
    let state = editor.document_reference(current.document.clone());
    let calls = Rc::new(Cell::new(0));
    let map = observer(&context, calls.clone());
    editor.replace(Editor::new("new"), 0);
    transaction(
        &context,
        wire::EditorTransactionEvent::Cancelled { id: current, state },
        map,
    )
    .apply_in(&mut editor, &context);
    assert_eq!(calls.get(), 1, "retired identity gets cleanup after reset");
    assert_eq!(editor.text(), "new");
    assert_eq!(editor.reset_revision(), 1);
    assert!(!slots::editor_pending(&context));
}
#[test]
fn an_old_retry_cannot_commit_over_the_current_pending_attempt() {
    let context = slots::Context::default();
    let mut editor = Editor::new("before");
    slots::editor_response(
        &context,
        wire::EditorResponse {
            id: id(2),
            decision: EditorDecision::Noop,
        },
    );
    let calls = Rc::new(Cell::new(0));
    let map = observer(&context, calls.clone());
    transaction(&context, commit(id(1), &editor, "stale"), map).apply_in(&mut editor, &context);
    assert_eq!(
        editor.text(),
        "before",
        "an old attempt must not replace document state"
    );
    assert_eq!(
        calls.get(),
        0,
        "stale retry must not run the history reducer"
    );
    assert!(
        slots::editor_pending(&context),
        "current attempt remains outstanding"
    );
    let valid = transaction(&context, commit(id(2), &editor, "accepted"), map);
    valid.clone().apply_in(&mut editor, &context);
    assert_eq!(editor.text(), "accepted");
    assert_eq!(calls.get(), 1);
    assert!(!slots::editor_pending(&context));
    valid.apply_in(&mut editor, &context);
    assert_eq!(
        calls.get(),
        1,
        "duplicate accepted commit does not repeat history"
    );
}
#[test]
fn native_message_envelope_is_send_and_stale_commit_cannot_acknowledge() {
    fn is_send<T: Send>() {}
    is_send::<EditorTransaction<()>>();
    let context = slots::Context::default();
    let mut editor = Editor::new("before");
    slots::editor_response(
        &context,
        wire::EditorResponse {
            id: id(1),
            decision: EditorDecision::Noop,
        },
    );
    let calls = Rc::new(Cell::new(0));
    let map = observer(&context, calls.clone());
    let event = commit(id(1), &editor, "after");
    let mut stale = event.clone();
    if let wire::EditorTransactionEvent::Commit { after, .. } = &mut stale {
        after.reset = 99;
    }
    transaction(&context, stale, map).apply_in(&mut editor, &context);
    assert!(slots::editor_pending(&context));
    assert_eq!(calls.get(), 0);
    // Missing callback storage cannot accept or acknowledge a state update.
    transaction(&context, event.clone(), u32::MAX).apply_in(&mut editor, &context);
    assert_eq!(editor.text(), "before");
    assert!(slots::editor_pending(&context));
    let valid = transaction(&context, event, map);
    valid.clone().apply_in(&mut editor, &context);
    assert_eq!(editor.text(), "after");
    assert_eq!(calls.get(), 1);
    assert!(!slots::editor_pending(&context));
    valid.apply_in(&mut editor, &context);
    assert_eq!(calls.get(), 1, "duplicate commit does not re-run history");
}

#[test]
fn transaction_cannot_route_through_another_driver() {
    let source = crate::App::for_driver();
    let context = &source.inner.slots;
    let mut other = crate::App::for_driver();
    let mut editor = Editor::new("before");
    slots::editor_response(
        context,
        wire::EditorResponse {
            id: id(1),
            decision: EditorDecision::Noop,
        },
    );
    let calls = Rc::new(Cell::new(0));
    let map = observer(context, calls.clone());
    transaction(context, commit(id(1), &editor, "after"), map).apply(&mut editor, &mut other);
    assert_eq!(editor.text(), "before");
    assert_eq!(calls.get(), 0);
    assert!(slots::editor_pending(context));
}

#[test]
fn transaction_from_a_dropped_driver_cannot_route() {
    let (transaction, calls) = {
        let source = crate::App::for_driver();
        let context = &source.inner.slots;
        let editor = Editor::new("before");
        let calls = Rc::new(Cell::new(0));
        let map = observer(context, calls.clone());
        (
            transaction(context, commit(id(1), &editor, "after"), map),
            calls,
        )
    };
    let mut other = crate::App::for_driver();
    let mut editor = Editor::new("before");
    transaction.apply(&mut editor, &mut other);
    assert_eq!(editor.text(), "before");
    assert_eq!(calls.get(), 0);
}
