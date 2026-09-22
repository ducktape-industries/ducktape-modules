use view_wire::*;

fn text() -> Node {
    Node::Text {
        id: None,
        style: Default::default(),
        content: "row".into(),
        heading: None,
        live: None,
    }
}
fn anchored(children: Vec<Node>) -> Node {
    Node::Anchored {
        anchor: Anchor::TopLeft,
        fit: AnchoredFitMode::SnapToWindow,
        position: None,
        position_mode: AnchoredPositionMode::Local,
        offset: None,
        children,
    }
}
fn image(label: String, state_children: Vec<Node>) -> Node {
    Node::Image {
        id: None,
        hash: 0,
        data: None,
        label: Some(label),
        image_style: ImageStyle {
            grayscale: false,
            object_fit: ImageObjectFit::Contain,
        },
        loading: !state_children.is_empty(),
        fallback: state_children.len() > 1,
        state_children,
        style: Default::default(),
        interactivity: Default::default(),
    }
}

#[test]
fn anchored_children_obey_the_global_node_budget() {
    let mut frame = Frame {
        root: Some(anchored((0..MAX_NODES + 10).map(|_| text()).collect())),
        ..Default::default()
    };
    sanitize(&mut frame).unwrap();
    assert_eq!(frame.root.unwrap().count(), MAX_NODES);
}

#[test]
fn image_state_children_cannot_survive_past_the_global_node_budget() {
    let children = vec![anchored((0..MAX_NODES).map(|_| text()).collect()), text()];
    let mut frame = Frame {
        root: Some(image(String::new(), children)),
        ..Default::default()
    };
    sanitize(&mut frame).unwrap();
    let root = frame.root.unwrap();
    assert!(root.count() <= MAX_NODES);
    let Node::Image {
        loading,
        fallback,
        state_children,
        ..
    } = root
    else {
        unreachable!()
    };
    assert!(
        !loading && !fallback,
        "incomplete state recipes must not retain invalid indexes"
    );
    assert!(state_children.is_empty());
}

#[test]
fn picture_labels_share_the_frame_text_budget() {
    let mut frame = Frame {
        root: Some(anchored(vec![
            image("a".repeat(MAX_STRING_BYTES), Vec::new()),
            Node::Svg {
                id: None,
                source: SvgSource::None,
                transformation: SvgTransformation {
                    scale: [1., 1.],
                    translate: [0., 0.],
                    rotate: 0.,
                },
                label: Some("b".repeat(MAX_STRING_BYTES)),
                style: Default::default(),
                interactivity: Default::default(),
            },
        ])),
        ..Default::default()
    };
    sanitize(&mut frame).unwrap();
    let Node::Anchored { children, .. } = frame.root.unwrap() else {
        unreachable!()
    };
    let Node::Image { label, .. } = &children[0] else {
        unreachable!()
    };
    assert_eq!(label.as_ref().unwrap().len(), MAX_TEXT_BYTES_PER_FRAME);
    let Node::Svg { label, .. } = &children[1] else {
        unreachable!()
    };
    assert_eq!(label.as_deref(), Some(""));
}
