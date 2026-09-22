use super::*;

/// `diff` then `apply` is the identity on the new tree, and the patches
/// are the ones a reader expects: a changed field is `Props`, a moved
/// key is `Move`, a new key `Insert`, a gone key `Remove`, and a node
/// of another kind `Replace`.
#[test]
fn a_diff_applied_to_the_old_tree_is_the_new_tree() {
    let old = column(vec![
        keyed("a", "one"),
        keyed("b", "two"),
        keyed("c", "three"),
        Node::Container {
            id: Some(ElementIdWire::Name("box".into())),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children: vec![keyed("inner", "deep")],
        },
    ]);
    let mut new = column(vec![
        keyed("c", "three"),
        keyed("a", "one!"),
        keyed("d", "four"),
        Node::Container {
            id: Some(ElementIdWire::Name("box".into())),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children: vec![Node::empty()],
        },
    ]);
    let mut applied = old.clone();
    let patches = diff(&mut applied.clone(), &mut new.clone());
    let kinds: Vec<&str> = patches
        .iter()
        .map(|patch| match patch {
            Patch::Replace { .. } => "replace",
            Patch::Props { .. } => "props",
            Patch::Insert { .. } => "insert",
            Patch::Remove { .. } => "remove",
            Patch::Move { .. } => "move",
        })
        .collect();
    assert_eq!(
        kinds,
        ["remove", "move", "props", "insert", "remove", "insert"],
        "{patches:#?}"
    );
    apply(&mut applied, patches).unwrap();
    assert_eq!(applied, new);
    // Nothing changed hands: both inputs of the diff are as they were.
    let mut untouched = old.clone();
    diff(&mut untouched, &mut new);
    assert_eq!(untouched, old);
    assert!(diff(&mut new.clone(), &mut new).is_empty());
}

#[test]
fn a_patch_the_tree_cannot_take_is_refused() {
    let tree = column(vec![keyed("a", "one")]);
    let refused = |patch: Patch| apply(&mut tree.clone(), vec![patch]).unwrap_err();
    assert_eq!(
        refused(Patch::Remove {
            path: vec![7],
            index: 0
        }),
        "a path to no node"
    );
    assert_eq!(
        refused(Patch::Remove {
            path: vec![],
            index: 1
        }),
        "an index past the list"
    );
    assert_eq!(
        refused(Patch::Insert {
            path: vec![0],
            index: 0,
            node: Node::empty()
        }),
        "a list edit on no list"
    );
    assert_eq!(
        refused(Patch::Props {
            path: vec![0],
            node: button(ButtonContent::Child(Box::new(Node::empty())))
        }),
        "props of another arity"
    );
    let many = vec![
        Patch::Move {
            path: vec![],
            from: 0,
            to: 0
        };
        MAX_PATCHES + 1
    ];
    assert_eq!(
        apply(&mut tree.clone(), many).unwrap_err(),
        "more patches than the host applies"
    );
}

/// A patch is bounded like a tree: the result of applying it is inside
/// every limit, however the patches were shaped.
#[test]
fn an_applied_patch_frame_is_a_sanitized_tree() {
    let mut tree = column(
        (0..MAX_NODES - 1)
            .map(|i| keyed(&i.to_string(), "x"))
            .collect(),
    );
    sanitize_tree(&mut tree).unwrap();
    assert_eq!(tree.count(), MAX_NODES);
    let mut deep = keyed("0", "leaf");
    for _ in 0..MAX_DEPTH {
        deep = column(vec![deep]);
    }
    apply(
        &mut tree,
        vec![
            Patch::Insert {
                path: vec![],
                index: 0,
                node: keyed("inserted", &"y".repeat(MAX_STRING_BYTES + 1)),
            },
            Patch::Replace {
                path: vec![1],
                node: deep,
            },
        ],
    )
    .unwrap();
    assert!(tree.count() <= MAX_NODES, "{}", tree.count());
    let Node::Container { children, .. } = &tree else {
        panic!()
    };
    // A new typed ID leaves every existing sibling identity unchanged.
    assert_eq!(children[0].key(), Some("inserted"));
    assert_eq!(children[2].key(), Some("1"));
    let Node::Text { content, .. } = &children[0] else {
        panic!()
    };
    assert_eq!(content.len(), MAX_STRING_BYTES);
    let mut depth = 0;
    let mut node = &children[1];
    while let Node::Container { children, .. } = node {
        depth += 1;
        node = &children[0];
    }
    assert!(depth <= MAX_DEPTH, "{depth}");
}

#[test]
fn a_well_behaved_frame_is_untouched() {
    let mut frame = Frame {
        root: Some(column(vec![text("hello")])),
        ..Frame::default()
    };
    let before = frame.clone();
    sanitize(&mut frame).unwrap();
    assert_eq!(frame, before);
}

#[test]
fn a_frame_past_the_text_budget_keeps_its_head_and_loses_its_tail() {
    const NODES: usize = 8;
    const EACH: usize = MAX_TEXT_BYTES_PER_FRAME / 4;

    let children = sanitized_children(column(
        (0..NODES).map(|_| text(&"é".repeat(EACH / 2))).collect(),
    ));
    let shaped: Vec<usize> = children
        .iter()
        .map(|child| match child {
            Node::Text { content, .. } => content.len(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(shaped.iter().sum::<usize>(), MAX_TEXT_BYTES_PER_FRAME);
    assert_eq!(&shaped[..4], &[EACH; 4]);
    assert_eq!(&shaped[4..], &[0; NODES - 4]);
}

#[test]
fn display_truncation_shortens_a_placeholder_and_never_a_document_reference() {
    let long = "x".repeat(2 * MAX_TEXT_BYTES_PER_FRAME);
    let document = document_reference(
        "app:draft",
        editor_document::MAX_EDITOR_DOCUMENT_BYTES as u32,
    );
    let mut frame = Frame {
        root: Some(column(vec![
            editor("App/e", &long, document.clone()),
            text(&long),
        ])),
        ..Frame::default()
    };
    assert_eq!(
        sanitize(&mut frame),
        Ok(SanitizeReport {
            display_text_truncated: true
        })
    );
    let Some(Node::Container { children, .. }) = &frame.root else {
        panic!()
    };
    let Node::Editor {
        placeholder,
        document: kept,
        ..
    } = &children[0]
    else {
        panic!()
    };
    assert!(
        placeholder.len() < long.len(),
        "an editor placeholder is display text and spends the frame budget"
    );
    assert_eq!(
        kept, &document,
        "a 1 MiB document crosses as an exact reference, not as truncated text"
    );
}

#[test]
fn repeated_editor_bindings_must_describe_one_identical_document() {
    let document = document_reference("app:draft", 32);
    let mut agreeing = Frame {
        root: Some(column(vec![
            editor("App/one", "a", document.clone()),
            editor("App/two", "b", document.clone()),
        ])),
        ..Frame::default()
    };
    assert_eq!(sanitize(&mut agreeing), Ok(SanitizeReport::default()));
    // A distinct logical document is independent, even at the same state.
    let mut independent = Frame {
        root: Some(column(vec![
            editor("App/one", "a", document.clone()),
            editor("App/two", "b", document_reference("app:notes", 32)),
        ])),
        ..Frame::default()
    };
    assert_eq!(sanitize(&mut independent), Ok(SanitizeReport::default()));
    let mut stale = document.clone();
    stale.revision -= 1;
    let mut conflicting = Frame {
        root: Some(column(vec![
            editor("App/one", "a", document),
            editor("App/two", "b", stale),
        ])),
        ..Frame::default()
    };
    assert_eq!(
        sanitize(&mut conflicting),
        Err("invalid editor document references or budget")
    );
}

#[test]
fn a_document_projection_cannot_be_silently_removed_by_the_node_budget() {
    let mut deep = editor("App/e", "notes", document_reference("app:draft", 8));
    for _ in 0..MAX_DEPTH {
        deep = column(vec![deep]);
    }
    let mut frame = Frame {
        root: Some(deep),
        ..Frame::default()
    };
    assert_eq!(
        sanitize(&mut frame),
        Err("frame budget would remove an editor document projection")
    );
}

#[test]
fn every_shaped_string_spends_the_same_budget() {
    let long = "x".repeat(MAX_TEXT_BYTES_PER_FRAME);
    let children = sanitized_children(column(vec![
        Node::Input {
            options: Default::default(),
            id: ElementIdWire::Name("App/i".into()),
            placeholder: long.clone(),
            value: long.clone(),
            on_input: 0,
            on_submit: None,
            secure: false,
            style: gpui::StyleRefinement::default(),
        },
        Node::Button {
            checked: None,
            expanded: None,
            selected: None,
            role: None,
            description: Some("Details".into()),
            key: "App/b".into(),
            content: ButtonContent::Label(long.clone()),
            label: Some(long),
            on_press: None,
            style: gpui::StyleRefinement::default(),
        },
        text("tail"),
    ]));
    let Node::Input {
        placeholder, value, ..
    } = &children[0]
    else {
        panic!()
    };
    // The placeholder took the frame's whole budget and everything
    // shaped after it came out empty. The accessible name is not shaped,
    // so it answers to the per-string cap alone.
    assert_eq!(placeholder.len(), MAX_TEXT_BYTES_PER_FRAME);
    assert!(value.is_empty());
    let Node::Button {
        content,
        label,
        description,
        ..
    } = &children[1]
    else {
        panic!()
    };
    assert_eq!(*content, ButtonContent::Label(String::new()));
    assert_eq!(description.as_deref(), Some(""));
    assert_eq!(label.as_deref().map(str::len), Some(MAX_STRING_BYTES));
    assert_eq!(children[2], text(""));
}

#[test]
fn a_hostile_frame_is_pulled_into_range() {
    let mut deep = text("leaf");
    for _ in 0..MAX_DEPTH + 10 {
        deep = column(vec![deep]);
    }
    let wide = column((0..MAX_NODES + 5).map(|_| text("x")).collect());
    let root = sanitized_root(column(vec![
        Node::Text {
            id: Some(ElementIdWire::Name("k".repeat(MAX_STRING_BYTES).into())),
            style: gpui::StyleRefinement::default(),
            content: "é".repeat(MAX_STRING_BYTES),
            heading: None,
            live: None,
        },
        deep,
        wide,
    ]));
    // A container whose child fell past the budget keeps an empty
    // stand-in, one per level at most.
    assert!(root.count() <= MAX_NODES + MAX_DEPTH, "{}", root.count());
    let Node::Container { children, .. } = &root else {
        panic!()
    };
    let Node::Text { id, content, .. } = &children[0] else {
        panic!("{:?}", children[0])
    };
    assert_eq!(
        id.as_ref().and_then(ElementIdWire::name).unwrap().len(),
        MAX_STRING_BYTES
    );
    assert!(content.len() <= MAX_STRING_BYTES && content.is_char_boundary(content.len()));
}

#[test]
fn oversized_typed_identity_is_refused_whole_instead_of_truncated() {
    let mut frame = Frame {
        root: Some(keyed(&"k".repeat(MAX_STRING_BYTES + 3), "text")),
        ..Default::default()
    };
    assert_eq!(
        sanitize(&mut frame).unwrap_err(),
        "element identity name is too long"
    );
}

#[test]
fn depth_is_cut_before_the_host_recurses_into_it() {
    let mut deep = text("leaf");
    for _ in 0..MAX_DEPTH * 2 {
        deep = column(vec![deep]);
    }
    let root = sanitized_root(deep);
    let mut depth = 0;
    let mut node = &root;
    while let Node::Container { children, .. } = node {
        depth += 1;
        node = &children[0];
    }
    assert!(depth <= MAX_DEPTH, "{depth}");
}
