use super::*;

#[test]
fn frames_allow_one_document_message_and_reject_a_second_before_delivery() {
    let (id, _) = metadata(MAX_EDITOR_DOCUMENT_BYTES);
    let message =
        EditorDocumentMessage::Transfer(chunk(&id, 0, vec![b'x'; MAX_EDITOR_CHUNK_BYTES]));
    let mut frame = crate::Frame {
        editor_documents: vec![message.clone()],
        ..Default::default()
    };
    let decoded: crate::Frame = crate::decode(&crate::encode(&frame)).unwrap();
    assert_eq!(decoded.editor_documents, frame.editor_documents);
    frame.editor_documents.push(message);
    assert!(
        crate::decode::<crate::Frame>(&crate::encode(&frame)).is_err(),
        "a frame cannot allocate a second document payload"
    );
}

#[test]
fn document_messages_reject_cross_document_targets_and_unbounded_chunks() {
    let (id, target) = metadata(MAX_EDITOR_DOCUMENT_BYTES);
    let request = EditorDocumentMessage::Request {
        id: id.clone(),
        target: target.clone(),
    };
    assert_eq!(request.validate(), Ok(()));
    let mut wrong = target;
    wrong.reset += 1;
    assert_eq!(
        EditorDocumentMessage::Request {
            id: id.clone(),
            target: wrong
        }
        .validate(),
        Err(EditorTransferError::Identity)
    );
    for (index, length) in [(16, 1), (0, 0), (0, MAX_EDITOR_CHUNK_BYTES + 1)] {
        assert_eq!(
            EditorDocumentMessage::Transfer(chunk(&id, index, vec![b'x'; length])).validate(),
            Err(EditorTransferError::Limit)
        );
    }
    assert_eq!(
        EditorDocumentMessage::Acknowledged { id: id.clone() }.id(),
        &id
    );
}

#[test]
fn repeated_bindings_charge_one_document_but_each_native_projection() {
    let (_, reference) = metadata(MAX_EDITOR_DOCUMENT_BYTES);
    let usage = validate_editor_document_refs(std::iter::repeat_n(&reference, 8)).unwrap();
    assert_eq!(
        usage,
        EditorDocumentUsage {
            documents: 1,
            live_bytes: MAX_EDITOR_DOCUMENT_BYTES,
            projection_bytes: MAX_EDITOR_PROJECTION_BYTES,
        }
    );
    assert_eq!(
        validate_editor_document_refs(std::iter::repeat_n(&reference, 9)),
        Err(EditorTransferError::Limit)
    );
    let mut conflicting = reference.clone();
    conflicting.revision += 1;
    assert_eq!(
        validate_editor_document_refs([&reference, &conflicting]),
        Err(EditorTransferError::Identity)
    );
}

#[test]
fn independent_documents_have_separate_count_and_live_byte_limits() {
    let documents: Vec<_> = (0..=MAX_EDITOR_DOCUMENTS)
        .map(|index| {
            let (_, mut reference) = metadata(0);
            reference.document = format!("app:document-{index}");
            reference
        })
        .collect();
    assert_eq!(
        validate_editor_document_refs(&documents[..MAX_EDITOR_DOCUMENTS])
            .unwrap()
            .documents,
        MAX_EDITOR_DOCUMENTS
    );
    assert_eq!(
        validate_editor_document_refs(&documents),
        Err(EditorTransferError::Limit)
    );
    let mut full = documents[..5].to_vec();
    for reference in &mut full {
        reference.byte_len = MAX_EDITOR_DOCUMENT_BYTES as u32;
    }
    assert_eq!(
        validate_editor_document_refs(&full[..4])
            .unwrap()
            .live_bytes,
        MAX_EDITOR_LIVE_BYTES
    );
    assert_eq!(
        validate_editor_document_refs(&full),
        Err(EditorTransferError::Limit)
    );
}

#[test]
fn sender_borrows_one_mib_and_delivers_one_bounded_chunk_per_frame() {
    let mut text = "x".repeat(MAX_EDITOR_DOCUMENT_BYTES - 3);
    text.insert(MAX_EDITOR_CHUNK_BYTES - 1, '한');
    let (id, target) = metadata(text.len());
    let mut sender = EditorTransferSender::new(id.clone(), target.clone()).unwrap();
    let mut receiver = EditorTransferReceiver::new(id, target.clone()).unwrap();
    let mut chunks = 0;
    let mut frames = 0;
    let mut result = None;
    while let Some(frame) = sender.next_frame(&target, &text).unwrap() {
        if let EditorTransfer::Chunk { index, bytes, .. } = &frame {
            assert_eq!(usize::from(*index), chunks, "each frame advances one chunk");
            assert!(bytes.len() <= MAX_EDITOR_CHUNK_BYTES);
            chunks += 1;
        }
        let delivered = receiver.receive(&frame).unwrap();
        if matches!(frame, EditorTransfer::Complete { .. }) {
            result = delivered;
        } else {
            assert!(delivered.is_none(), "no prefix is visible before Complete");
        }
        frames += 1;
        assert!(frames <= MAX_EDITOR_CHUNKS + 2);
    }
    assert_eq!(chunks, MAX_EDITOR_CHUNKS);
    assert_eq!(frames, MAX_EDITOR_CHUNKS + 2);
    assert_eq!(result.as_deref(), Some(text.as_str()));
}

#[test]
fn source_reset_aborts_an_incomplete_transfer_without_sending_new_bytes() {
    let text = "a".repeat(MAX_EDITOR_CHUNK_BYTES + 1);
    let (id, target) = metadata(text.len());
    let mut sender = EditorTransferSender::new(id.clone(), target.clone()).unwrap();
    assert!(matches!(
        sender.next_frame(&target, &text).unwrap(),
        Some(EditorTransfer::Begin { .. })
    ));
    assert!(matches!(
        sender.next_frame(&target, &text).unwrap(),
        Some(EditorTransfer::Chunk { index: 0, .. })
    ));
    let mut next = target;
    next.reset += 1;
    assert_eq!(
        sender.next_frame(&next, &text),
        Ok(Some(EditorTransfer::Abort { id }))
    );
    assert_eq!(sender.next_frame(&next, &text), Ok(None));
}

#[test]
fn a_one_mib_document_sends_only_the_changed_byte_and_caret_sends_nothing() {
    let before = "a".repeat(MAX_EDITOR_DOCUMENT_BYTES - 1);
    let mut after = before.clone();
    let at = MAX_EDITOR_DOCUMENT_BYTES / 2;
    after.insert(at, 'X');
    let patches = editor_changed_span(&before, &after).unwrap();
    assert_eq!(
        patches,
        vec![crate::EditorPatch {
            start_byte: at as u32,
            end_byte: at as u32,
            replacement: "X".into(),
        }]
    );
    assert!(editor_changed_span(&after, &after).unwrap().is_empty());
    let mut observed = before;
    for patch in patches.iter().rev() {
        observed.replace_range(
            patch.start_byte as usize..patch.end_byte as usize,
            &patch.replacement,
        );
    }
    assert_eq!(observed, after);
}

#[test]
fn minimal_spans_preserve_combining_emoji_and_paired_line_endings() {
    for (before, after, start, end, replacement) in [
        ("Ae\u{301}Z", "AeZ", 1, 4, "e"),
        ("A👍🏽Z", "A👍Z", 1, 9, "👍"),
        ("a\r\nb", "a\rX\nb", 1, 3, "\rX\n"),
        ("a\n\rb", "a\nX\rb", 1, 3, "\nX\r"),
        ("", "한", 0, 0, "한"),
        ("한", "", 0, 3, ""),
    ] {
        let patches = editor_changed_span(before, after).unwrap();
        assert_eq!(
            patches,
            vec![crate::EditorPatch {
                start_byte: start,
                end_byte: end,
                replacement: replacement.into(),
            }],
            "{before:?} -> {after:?}"
        );
        assert_eq!(
            crate::patched_editor_text(before, &patches, EditorCursor::default()).unwrap(),
            after
        );
    }
}

fn metadata(len: usize) -> (EditorTransferId, EditorDocumentRef) {
    (
        EditorTransferId {
            instance: 3,
            document: "app:draft".into(),
            reset: 7,
            serial: 11,
            attempt: 0,
        },
        EditorDocumentRef {
            document: "app:draft".into(),
            reset: 7,
            text_revision: 2,
            revision: 4,
            cursor: EditorCursor::default(),
            byte_len: len as u32,
        },
    )
}

/// The bytes stay the caller's: which byte a chunk carries is what the
/// append, prefix and UTF-8 cases are each testing.
fn chunk(id: &EditorTransferId, index: u8, bytes: Vec<u8>) -> EditorTransfer {
    EditorTransfer::Chunk {
        id: id.clone(),
        index,
        bytes,
    }
}

fn begun(len: usize) -> (EditorTransferId, EditorTransferReceiver) {
    let (id, target) = metadata(len);
    let mut receiver = EditorTransferReceiver::new(id.clone(), target.clone()).unwrap();
    assert_eq!(
        receiver.receive(&EditorTransfer::Begin {
            id: id.clone(),
            target
        }),
        Ok(None)
    );
    (id, receiver)
}

#[test]
fn exact_one_mib_is_published_only_after_complete_even_when_utf8_crosses_a_chunk() {
    let mut text = "x".repeat(MAX_EDITOR_DOCUMENT_BYTES - 3);
    text.insert(MAX_EDITOR_CHUNK_BYTES - 1, '한');
    let (id, mut receiver) = begun(text.len());
    let chunks: Vec<_> = text.as_bytes().chunks(MAX_EDITOR_CHUNK_BYTES).collect();
    assert_eq!(chunks.len(), MAX_EDITOR_CHUNKS);
    assert!(std::str::from_utf8(chunks[0]).is_err());
    for (index, bytes) in chunks.iter().enumerate() {
        assert!(
            matches!(
                receiver.receive(&chunk(&id, index as u8, bytes.to_vec())),
                Ok(None)
            ),
            "a chunk must not publish a document prefix"
        );
        assert_eq!(
            receiver.buffered_bytes(),
            (index + 1) * MAX_EDITOR_CHUNK_BYTES
        );
    }
    assert_eq!(
        receiver.receive(&EditorTransfer::Complete { id }),
        Ok(Some(text))
    );
    assert_eq!(receiver.buffered_bytes(), 0);
}

#[test]
fn every_interruption_boundary_discards_staging_without_publishing_a_prefix() {
    for boundary in 0..=MAX_EDITOR_CHUNKS {
        let (id, mut receiver) = begun(MAX_EDITOR_DOCUMENT_BYTES);
        for index in 0..boundary {
            assert_eq!(
                receiver.receive(&chunk(&id, index as u8, vec![b'x'; MAX_EDITOR_CHUNK_BYTES])),
                Ok(None)
            );
        }
        assert_eq!(
            receiver.receive(&EditorTransfer::Abort { id: id.clone() }),
            Err(EditorTransferError::Aborted)
        );
        assert_eq!(receiver.buffered_bytes(), 0);
        assert_eq!(
            receiver.receive(&EditorTransfer::Complete { id }),
            Err(EditorTransferError::Order)
        );
    }
}

#[test]
fn stale_identity_cannot_abort_or_append_to_the_requested_document() {
    let (id, mut receiver) = begun(MAX_EDITOR_CHUNK_BYTES + 1);
    assert_eq!(
        receiver.receive(&chunk(&id, 0, vec![b'a'; MAX_EDITOR_CHUNK_BYTES])),
        Ok(None)
    );
    for change in 0..4 {
        let mut stale = id.clone();
        match change {
            0 => stale.instance += 1,
            1 => stale.serial += 1,
            2 => stale.reset += 1,
            _ => stale.document = "app:another".into(),
        }
        for event in [
            EditorTransfer::Abort { id: stale.clone() },
            chunk(&stale, 1, vec![b'b']),
            EditorTransfer::Complete { id: stale },
        ] {
            assert_eq!(receiver.receive(&event), Err(EditorTransferError::Identity));
            assert_eq!(receiver.buffered_bytes(), MAX_EDITOR_CHUNK_BYTES);
        }
    }
    receiver.receive(&chunk(&id, 1, vec![b'b'])).unwrap();
    let text = receiver
        .receive(&EditorTransfer::Complete { id })
        .unwrap()
        .unwrap();
    assert_eq!(text, format!("{}b", "a".repeat(MAX_EDITOR_CHUNK_BYTES)));
}

#[test]
fn malformed_active_transfer_fails_closed_instead_of_becoming_a_partial_document() {
    for bad in 0..4 {
        let (id, mut receiver) = begun(MAX_EDITOR_CHUNK_BYTES + 1);
        receiver
            .receive(&chunk(&id, 0, vec![b'a'; MAX_EDITOR_CHUNK_BYTES]))
            .unwrap();
        let event = match bad {
            0 => chunk(&id, 0, vec![b'a'; MAX_EDITOR_CHUNK_BYTES]),
            1 => chunk(&id, 2, vec![b'b']),
            2 => chunk(&id, 1, vec![b'b'; 2]),
            _ => EditorTransfer::Complete { id: id.clone() },
        };
        assert!(receiver.receive(&event).is_err());
        assert_eq!(receiver.buffered_bytes(), 0);
        assert_eq!(
            receiver.receive(&EditorTransfer::Complete { id }),
            Err(EditorTransferError::Order)
        );
    }
}

#[test]
fn complete_checks_utf8_and_native_cursor_and_empty_documents_need_no_chunk() {
    let (id, mut receiver) = begun(2);
    receiver.receive(&chunk(&id, 0, vec![0xff, 0xff])).unwrap();
    assert_eq!(
        receiver.receive(&EditorTransfer::Complete { id }),
        Err(EditorTransferError::Utf8)
    );
    let (id, mut target) = metadata(3);
    target.cursor.position.column = 1;
    let mut receiver = EditorTransferReceiver::new(id.clone(), target.clone()).unwrap();
    receiver
        .receive(&EditorTransfer::Begin {
            id: id.clone(),
            target,
        })
        .unwrap();
    receiver
        .receive(&chunk(&id, 0, "e\u{301}".as_bytes().to_vec()))
        .unwrap();
    assert_eq!(
        receiver.receive(&EditorTransfer::Complete { id }),
        Err(EditorTransferError::Cursor)
    );
    let (id, mut receiver) = begun(0);
    assert_eq!(
        receiver.receive(&EditorTransfer::Complete { id }),
        Ok(Some(String::new()))
    );
}

#[test]
fn reference_limits_are_checked_before_allocating_a_document() {
    let (id, mut target) = metadata(MAX_EDITOR_DOCUMENT_BYTES + 1);
    assert_eq!(
        EditorTransferReceiver::new(id.clone(), target.clone()).unwrap_err(),
        EditorTransferError::Limit
    );
    target.byte_len = 0;
    target.document = "d".repeat(1025);
    assert_eq!(target.validate(), Err(EditorTransferError::Limit));
    let (_, mut target) = metadata(0);
    target.cursor.position.line = 1;
    assert_eq!(target.validate(), Err(EditorTransferError::Cursor));
    target.cursor = EditorCursor::default();
    target.reset += 1;
    assert_eq!(
        EditorTransferReceiver::new(id, target).unwrap_err(),
        EditorTransferError::Identity
    );
    assert!(EditorChunkAssembler::new(MAX_EDITOR_DOCUMENT_BYTES + 1).is_err());
}

#[test]
fn decoder_rejects_advertised_oversized_chunks_before_reading_their_payload() {
    let (id, _) = metadata(0);
    let event = chunk(&id, 0, vec![]);
    let mut encoded = crate::encode(&event);
    assert_eq!(encoded.last(), Some(&0x90));
    encoded.pop();
    encoded.push(0xdd);
    encoded.extend_from_slice(&((MAX_EDITOR_CHUNK_BYTES as u32) + 1).to_be_bytes());
    let error = crate::decode::<EditorTransfer>(&encoded)
        .unwrap_err()
        .to_string();
    assert!(error.contains("editor chunk byte limit"), "{error}");
}
