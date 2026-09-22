use super::*;

/// A button holding a node is the third way the tree recurses, and the
/// only one that hangs off an enum's field rather than a struct's.
#[test]
fn a_button_holding_a_node_round_trips_and_counts_as_a_child() {
    let frame = Frame {
        root: Some(button(ButtonContent::Child(Box::new(text("inside"))))),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);

    let mut nested = Node::empty();
    for _ in 0..MAX_DEPTH + 1 {
        nested = button(ButtonContent::Child(Box::new(nested)));
    }
    let bytes = encode(&Frame {
        root: Some(nested),
        ..Frame::default()
    });
    assert!(decode::<Frame>(&bytes).is_err());
}

/// A mouse area recurses like a container, is diffed as a node with
/// one fixed child, and keeps typed identity exact.
#[test]
fn a_mouse_area_round_trips_diffs_by_props_and_claims_its_key() {
    let frame = Frame {
        root: Some(mouse_area("App/m", Some(0), text("inside"))),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    assert_eq!(frame.root.as_ref().unwrap().count(), 2);

    // A changed route index is a `Props` patch that keeps the child.
    let mut old = mouse_area("App/m", Some(0), text("inside"));
    let mut new = mouse_area("App/m", Some(7), text("inside"));
    let patches = diff(&mut old, &mut new);
    assert!(
        matches!(patches.as_slice(), [Patch::Props { path, .. }] if path.is_empty()),
        "{patches:?}"
    );
    apply(&mut old, patches).unwrap();
    assert_eq!(old, new);

    // Typed duplicates are rejected rather than renamed into a different target.
    let mut duplicate = Frame {
        root: Some(column(vec![
            mouse_area("App/m", None, text("a")),
            mouse_area("App/m", None, text("b")),
        ])),
        ..Frame::default()
    };
    assert_eq!(
        sanitize(&mut duplicate).unwrap_err(),
        "duplicate typed element identity among siblings"
    );
}

#[test]
fn control_labels_round_trip() {
    let mut notes = editor("App/e", "Notes", document_reference("app:draft", 9));
    let Node::Editor { label, .. } = &mut notes else {
        unreachable!()
    };
    *label = Some("Notes".into());
    let frame = Frame {
        root: Some(column(vec![
            notes,
            Node::Slider {
                id: ElementIdWire::Name("App/s".into()),
                label: Some("Volume".into()),
                value: 0.5,
                min: 0.0,
                max: 1.0,
                step: 0.1,
                on_change: 1,
                on_release: None,
                axis: Axis::Row,
                style: gpui::StyleRefinement::default(),
            },
            Node::ComboBox {
                id: ElementIdWire::Name("App/c".into()),
                state_key: "App/c".into(),
                options: vec!["Serif".into()],
                selected: None,
                reset: 0,
                placeholder: String::new(),
                label: Some("Font".into()),
                on_select: 2,
                style: Default::default(),
                settings: Box::default(),
            },
            Node::PickList {
                settings: Default::default(),
                id: ElementIdWire::Name("App/p".into()),
                options: vec!["Dark".into()],
                selected: Some(0),
                placeholder: None,
                label: Some("Theme".into()),
                on_select: 3,
                style: gpui::StyleRefinement::default(),
            },
        ])),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn a_mouse_areas_role_name_and_state_round_trip() {
    let mut area = mouse_area("App/m", None, text("inside"));
    let Node::MouseArea {
        role,
        label,
        expanded,
        selected,
        checked,
        ..
    } = &mut area
    else {
        unreachable!()
    };
    *role = Some(Role::Checkbox);
    *label = Some("Wrap lines".into());
    *expanded = Some(false);
    *selected = Some(true);
    *checked = Some(true);
    let frame = Frame {
        root: Some(area),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn a_selected_button_round_trips() {
    let mut tab = button(ButtonContent::Label("Inbox".into()));
    let Node::Button { selected, .. } = &mut tab else {
        unreachable!()
    };
    *selected = Some(true);
    let frame = Frame {
        root: Some(tab),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn a_buttons_role_round_trips() {
    let mut link = button(ButtonContent::Label("Docs".into()));
    let Node::Button { role, .. } = &mut link else {
        unreachable!()
    };
    *role = Some(Role::Link);
    let frame = Frame {
        root: Some(link),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn a_texts_heading_and_live_region_round_trip() {
    let mut title = text("Inbox");
    let Node::Text { heading, live, .. } = &mut title else {
        unreachable!()
    };
    *heading = Some(1);
    *live = Some(Live::Assertive);
    let mut status = text("3 new");
    let Node::Text { live, .. } = &mut status else {
        unreachable!()
    };
    *live = Some(Live::Polite);
    let frame = Frame {
        root: Some(column(vec![title, status])),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn an_overlays_label_round_trips() {
    let frame = Frame {
        root: Some(Node::Overlay {
            id: ElementIdWire::Name("ask".into()),
            label: Some("Delete page".into()),
            style: Default::default(),
            on_dismiss: Some(4),
            children: vec![text("base"), text("Delete this page?")],
        }),
        ..Frame::default()
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
}

#[test]
fn a_heading_level_outside_1_to_6_is_no_heading() {
    for (level, kept) in [
        (0, None),
        (1, Some(1)),
        (6, Some(6)),
        (7, None),
        (255, None),
    ] {
        let mut node = text("Title");
        let Node::Text { heading, .. } = &mut node else {
            unreachable!()
        };
        *heading = Some(level);
        let Node::Text { heading, .. } = sanitized_root(node) else {
            panic!("still text")
        };
        assert_eq!(heading, kept, "level {level}");
    }
}

/// Building and encoding a chain this deep recurses as far as decoding
/// it would, so the hostile frame is made where there is stack for it.

#[test]
fn a_tree_the_host_would_not_walk_is_refused_before_it_is_built() {
    assert!(decode::<Frame>(&deep_chain_bytes(MAX_DEPTH - 1)).is_ok());
    let refused = decode::<Frame>(&deep_chain_bytes(MAX_DEPTH + 1)).unwrap_err();
    assert!(
        refused.contains("deeper than the host renders"),
        "{refused}"
    );
}

/// The bug this guards: a chain of a few thousand containers is a frame
/// of ~100 KB — far inside any byte cap a host sets — and decoding it
/// walked a host thread off its stack, aborting the process. A refusal
/// is a message in one app's window; an overflow is every window gone.
#[test]
fn a_chain_that_overflowed_the_host_stack_is_an_error_not_a_crash() {
    let bytes = deep_chain_bytes(5_000);
    assert!(bytes.len() < 1 << 20, "{} bytes", bytes.len());
    assert!(decode::<Frame>(&bytes).is_err());
}

#[test]
fn more_nodes_than_the_host_holds_is_refused() {
    let wide = column((0..MAX_DECODED_NODES + 2).map(|_| Node::empty()).collect());
    let bytes = encode(&Frame {
        root: Some(wide),
        ..Frame::default()
    });
    let refused = decode::<Frame>(&bytes).unwrap_err();
    assert!(
        refused.contains("more nodes than the host holds"),
        "{refused}"
    );
}

/// A frame is bytes a module wrote, so every byte of it is the guest's
/// to choose. Whatever they say, `decode` answers rather than aborts.
#[test]
fn bytes_a_hostile_guest_could_write_are_answered_not_survived() {
    let sound = encode(&Frame {
        upstream_sanitization: Default::default(),
        editor_decisions: Vec::new(),
        editor_documents: Vec::new(),
        tooltip_responses: Vec::new(),
        mouse_interest: false,
        event_interest: Default::default(),
        root: Some(column(vec![text("hello"), Node::empty()])),
        requests: vec![Request {
            id: 7,
            kind: "host.echo".into(),
            payload: b"hi".to_vec(),
        }],
        cancels: vec![1, 2],
        unchanged: false,
        busy: false,
        patches: Vec::new(),
    });
    for cut in 0..sound.len() {
        let _ = decode::<Frame>(&sound[..cut]);
    }
    for at in 0..sound.len() {
        for bit in 0..8 {
            let mut flipped = sound.clone();
            flipped[at] ^= 1 << bit;
            let _ = decode::<Frame>(&flipped);
        }
    }
}

#[test]
fn duplicate_typed_ids_are_refused_instead_of_silently_renamed() {
    let mut frame = Frame {
        root: Some(column(vec![keyed("same", "one"), keyed("same", "two")])),
        ..Default::default()
    };
    assert_eq!(
        sanitize(&mut frame).unwrap_err(),
        "duplicate typed element identity among siblings"
    );
    let mut root = column(vec![keyed("same", "one")]);
    let error = apply(
        &mut root,
        vec![Patch::Insert {
            path: vec![],
            index: 1,
            node: keyed("same", "two"),
        }],
    )
    .unwrap_err();
    assert_eq!(error, "duplicate typed element identity among siblings");
}

#[test]
fn uniform_list_path_must_match_its_typed_tree_ancestry() {
    let parent = ElementIdWire::Integer(7);
    let list = ElementIdWire::Name("list".into());
    let mut valid = Frame {
        root: Some(Node::Container {
            id: Some(parent.clone()),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children: vec![uniform(vec![parent.clone(), list.clone()])],
        }),
        ..Default::default()
    };
    sanitize(&mut valid).unwrap();

    let Node::Container { children, .. } = valid.root.as_mut().unwrap() else {
        unreachable!()
    };
    let Node::UniformList { path, .. } = &mut children[0] else {
        unreachable!()
    };
    *path = vec![ElementIdWire::Name("forged-parent".into()), list];
    assert_eq!(
        sanitize(&mut valid).unwrap_err(),
        "uniform-list authored path is invalid"
    );
}

/// A hostile screen cannot cause quadratic identity repair or alias state.
#[test]
fn a_screen_of_one_typed_id_is_refused_in_linear_time() {
    let mut frame = Frame {
        root: Some(column(
            (0..MAX_NODES - 1).map(|_| keyed("same", "x")).collect(),
        )),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    assert_eq!(
        sanitize(&mut frame).unwrap_err(),
        "duplicate typed element identity among siblings"
    );
    if cfg!(not(debug_assertions)) {
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
    }
}
