use super::*;

#[test]
fn actual_display_truncation_report_survives_encoding_and_resanitizing() {
    let mut frame = Frame {
        root: Some(text(&"x".repeat(MAX_STRING_BYTES + 1))),
        ..Default::default()
    };
    let report = sanitize(&mut frame).unwrap();
    assert!(
        report.display_text_truncated,
        "actual shortened text must be reported"
    );
    assert_eq!(frame.upstream_sanitization, report);
    let mut received: Frame = decode(&encode(&frame)).unwrap();
    assert!(
        !sanitize(&mut received).unwrap().display_text_truncated,
        "receiver observes no additional shortening"
    );
    assert!(
        received.upstream_sanitization.display_text_truncated,
        "producer loss cannot disappear across a wire hop"
    );
    let mut small = Frame {
        root: Some(text("complete")),
        ..Default::default()
    };
    assert_eq!(sanitize(&mut small).unwrap(), SanitizeReport::default());
    assert_eq!(small.upstream_sanitization, SanitizeReport::default());
}

#[test]
fn text_passed_to_a_host_surface_reports_actual_loss() {
    let mut frame = Frame {
        root: Some(Node::Surface {
            id: ElementIdWire::Name("preview".into()),
            style: Default::default(),
            name: "forge_code".into(),
            args: vec![SurfaceValue::Record {
                name: "Preview".into(),
                fields: vec![(
                    "text".into(),
                    SurfaceValue::Option(Some(Box::new(SurfaceValue::List(vec![
                        SurfaceValue::Str("x".repeat(MAX_STRING_BYTES)),
                    ])))),
                )],
            }],
            on_event: None,
        }),
        ..Default::default()
    };
    assert!(
        sanitize(&mut frame).unwrap().display_text_truncated,
        "surface text spends the same frame budget and its loss must be reported"
    );
    assert!(!sanitize(&mut frame).unwrap().display_text_truncated);
}

#[test]
fn applied_aggregate_text_and_rich_text_loss_is_reported_but_removal_is_not() {
    let rich = Node::RichText {
        id: Some(ElementIdWire::Name("rich".into())),
        style: gpui::StyleRefinement::default(),
        text: "y".repeat(MAX_TEXT_BYTES_PER_FRAME / 2),
        runs: RichTextRuns::default(),
        font_family_overrides: vec![],
        clickable_ranges: vec![],
        on_click: None,
        on_hover: None,
        tooltip: None,
    };
    let mut root = Node::Container(crate::ContainerNode {
        id: Some(ElementIdWire::Name("root".into())),
        style: gpui::StyleRefinement::default(),
        interactivity: Interactivity::default(),
        children: vec![text(&"x".repeat(MAX_TEXT_BYTES_PER_FRAME / 2 + 1))],
    });
    let report = apply(
        &mut root,
        vec![Patch::Insert {
            path: vec![],
            index: 1,
            node: rich,
        }],
    )
    .unwrap();
    assert!(
        report.display_text_truncated,
        "each node fits, but the applied aggregate loses tail text"
    );
    let (_, bytes) = text_amounts(&root).unwrap();
    assert_eq!(bytes, MAX_TEXT_BYTES_PER_FRAME);
    let report = apply(
        &mut root,
        vec![Patch::Remove {
            path: vec![],
            index: 0,
        }],
    )
    .unwrap();
    assert!(
        !report.display_text_truncated,
        "intentional removal precedes the measured sanitizer pass"
    );
}

/// The tree a host is left holding. Most tests here want only that —
/// the `Frame` around it is scaffolding, and the report is the business

#[test]
fn sanitized_surfaces_share_the_decoders_value_budget() {
    let mut frame = Frame {
        root: Some(column(
            (0..20)
                .map(|i| Node::Surface {
                    id: ElementIdWire::Name(format!("surface-{i}").into()),
                    style: Default::default(),
                    name: "many".into(),
                    args: vec![SurfaceValue::Unit; MAX_SURFACE_ARGS],
                    on_event: None,
                })
                .collect(),
        )),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let decoded = decode::<Frame>(&encode(&frame));
    assert!(decoded.is_ok(), "sanitized frame must decode: {decoded:?}");
}

#[test]
fn surfaces_round_trip_patch_and_bound_their_arguments() {
    use SurfaceValue as V;
    let values = vec![
        V::Unit,
        V::Bool(true),
        V::I64(i64::MAX),
        V::F64(1.25),
        V::Str("link".into()),
    ];
    let node = Node::Surface {
        id: ElementIdWire::Name("view".into()),
        style: Default::default(),
        name: "preview".into(),
        args: values.clone(),
        on_event: Some(4),
    };
    assert_eq!(decode::<Node>(&encode(&node)).unwrap(), node);
    for value in values {
        let event = Event::Surface { handler: 4, value };
        assert_eq!(decode::<Event>(&encode(&event)).unwrap(), event);
    }
    let mut changed = node.clone();
    if let Node::Surface { args, on_event, .. } = &mut changed {
        args[1] = V::Bool(false);
        *on_event = Some(9);
    }
    let patches = diff(&mut node.clone(), &mut changed.clone());
    let mut applied = node;
    apply(&mut applied, patches).unwrap();
    assert_eq!(applied, changed);
    let Node::Surface { name, args, .. } = sanitized_root(Node::Surface {
        id: ElementIdWire::Name("view".into()),
        style: Default::default(),
        name: "preview".into(),
        args: std::iter::once(V::F64(f64::NAN))
            .chain(std::iter::repeat_n(
                V::Str("é".repeat(MAX_STRING_BYTES)),
                MAX_SURFACE_ARGS + 1,
            ))
            .collect(),
        on_event: None,
    }) else {
        unreachable!()
    };
    assert_eq!(args.len(), MAX_SURFACE_ARGS);
    assert_eq!(args[0], V::F64(0.0));
    let bytes = args
        .iter()
        .map(|value| match value {
            V::Str(text) => text.len(),
            _ => 0,
        })
        .sum::<usize>();
    assert!(bytes + name.len() <= MAX_TEXT_BYTES_PER_FRAME);
}

#[test]
fn encoded_size_matches_named_messagepack_without_a_second_buffer() {
    for count in [0, 1, 16, 256, 2000] {
        let frame = Frame {
            root: Some(column(
                (0..count)
                    .map(|index| keyed(&index.to_string(), "한é"))
                    .collect(),
            )),
            ..Default::default()
        };
        let bytes = encode(&frame);
        assert_eq!(bytes, rmp_serde::to_vec_named(&frame).unwrap());
        assert_eq!(encoded_size(&frame), bytes.len() as u64);
        assert_eq!(decode::<Frame>(&bytes).unwrap(), frame);
        let node = frame.root.as_ref().unwrap();
        let mut fingerprint = std::hash::DefaultHasher::new();
        std::hash::Hasher::write(&mut fingerprint, &encode(node));
        assert_eq!(node.fingerprint(), std::hash::Hasher::finish(&fingerprint));
    }
}

#[test]
fn tooltip_responses_share_the_frame_node_budget() {
    let response = || TooltipResponse {
        request: 1,
        character_index: None,
        content: Some(Box::new(column(
            (0..MAX_NODES).map(|_| Node::empty()).collect(),
        ))),
    };
    let mut frame = Frame {
        tooltip_responses: vec![response(), response()],
        ..Default::default()
    };
    sanitize(&mut frame).unwrap();
    assert_eq!(frame.tooltip_responses.len(), 1);
    assert!(
        frame.tooltip_responses[0]
            .content
            .as_ref()
            .is_some_and(|content| content.count() <= MAX_NODES)
    );
}

#[test]
fn rich_tooltip_cache_and_explicit_none_share_the_frame_budget() {
    let mut frame = Frame {
        root: Some(Node::RichText {
            id: Some(ElementIdWire::Name("rich".into())),
            style: gpui::StyleRefinement::default(),
            text: "text".into(),
            runs: RichTextRuns::default(),
            font_family_overrides: Vec::new(),
            clickable_ranges: Vec::new(),
            on_click: None,
            on_hover: None,
            tooltip: Some(RichTextTooltip {
                request: 2,
                character_index: Some(0),
                content: Some(Box::new(column(
                    (0..MAX_NODES).map(|_| Node::empty()).collect(),
                ))),
            }),
        }),
        tooltip_responses: vec![TooltipResponse {
            request: 2,
            character_index: Some(1),
            content: None,
        }],
        ..Default::default()
    };
    sanitize(&mut frame).unwrap();
    let Node::RichText {
        tooltip: Some(tooltip),
        ..
    } = frame.root.unwrap()
    else {
        panic!("rich tooltip")
    };
    assert!(
        tooltip
            .content
            .as_ref()
            .is_some_and(|content| content.count() < MAX_NODES)
    );
    assert_eq!(frame.tooltip_responses.len(), 1);
    assert!(frame.tooltip_responses[0].content.is_none());
}

#[test]
fn a_frame_round_trips() {
    let frame = Frame {
        upstream_sanitization: Default::default(),
        editor_decisions: Vec::new(),
        editor_documents: Vec::new(),
        tooltip_responses: Vec::new(),
        mouse_interest: true,
        event_interest: Default::default(),
        root: Some(column(vec![
            text("hello"),
            Node::Button {
                checked: None,
                expanded: None,
                selected: None,
                role: None,
                description: None,
                id: ElementIdWire::Name("App/b".into()),
                content: ButtonContent::Label("Go".into()),
                label: None,
                on_press: Some(3),
                style: gpui::StyleRefinement::default(),
            },
            Node::Input {
                options: Default::default(),
                id: ElementIdWire::Name("App/i".into()),
                placeholder: "Name".into(),
                value: "x".into(),
                on_input: 0,
                on_submit: Some(4),
                secure: false,
                style: gpui::StyleRefinement::default(),
            },
            Node::Editor {
                options: Default::default(),
                id: ElementIdWire::Name("App/e".into()),
                style: gpui::StyleRefinement::default(),
                placeholder: "Notes".into(),
                label: None,
                document: document_reference("app:draft", 9),
                on_document: 5,
                editable: true,
            },
        ])),
        patches: vec![Patch::Remove {
            path: vec![0, 1],
            index: 2,
        }],
        requests: vec![Request {
            id: 1,
            kind: "host.echo".into(),
            payload: b"hi".to_vec(),
        }],
        cancels: vec![2],
        unchanged: false,
        busy: false,
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    let events = vec![
        Event::Message(3),
        Event::Input {
            handler: 0,
            text: "xy".into(),
        },
        Event::EditorTransaction {
            handler: 5,
            event: EditorTransactionEvent::Commit {
                origin: None,
                id: EditorTransactionId {
                    instance: 1,
                    document: "app:draft".into(),
                    reset: 0,
                    sequence: 2,
                    attempt: 0,
                    text_revision: 1,
                    revision: 3,
                },
                before: document_reference("app:draft", 9),
                after: document_reference("app:draft", 10),
                patches: vec![EditorPatch {
                    start_byte: 9,
                    end_byte: 9,
                    replacement: "z".into(),
                }],
                kind: EditorEditKind::Insert,
                history: EditorHistoryEffect::ExtendPrevious,
                input_time_ms: 42,
            },
        },
        Event::Toggle {
            handler: 1,
            on: true,
        },
        Event::Slide {
            handler: 2,
            value: 0.5,
        },
        Event::Select {
            handler: 3,
            index: 1,
        },
        Event::Pointer {
            handler: 4,
            x: 12.5,
            y: 3.0,
        },
        Event::Scroll {
            handler: 5,
            dx: 0.0,
            dy: -1.0,
            pixels: false,
        },
        Event::ScrollOffset {
            handler: 6,
            x: 24.0,
            y: 50.0,
            relative_x: 0.2,
            relative_y: 0.1,
        },
        Event::Response {
            id: 1,
            result: Err(Error::new("module", "nope")),
            done: true,
        },
        Event::Resync,
    ];
    assert_eq!(decode::<Vec<Event>>(&encode(&events)).unwrap(), events);
}
