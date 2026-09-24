//! Every node kind shares the frame's one node and text budget: tooltip
//! content, anchored children, image state children and picture labels
//! cannot buy more than the tree they sit in.
use gpui::{StyleRefinement, Styled, px};
use view_wire::*;

fn container(interactivity: Interactivity, children: Vec<Node>) -> Node {
    Node::Container(view_wire::ContainerNode {
        id: None,
        style: StyleRefinement::default(),
        interactivity,
        children,
    })
}

#[test]
fn window_controls_and_focus_refinements_are_bounded_inside_tooltips() {
    for area in [
        WindowControlArea::Drag,
        WindowControlArea::Close,
        WindowControlArea::Max,
        WindowControlArea::Min,
    ] {
        let hostile = Interactivity {
            window_control_area: Some(area),
            focus: Some(StyleRefinement::default().w(px(f32::INFINITY)).opacity(10.)),
            in_focus: Some(StyleRefinement::default().m(px(-100.))),
            focus_visible: Some(StyleRefinement::default().text_size(px(1e20))),
            ..Default::default()
        };
        let root = container(
            Interactivity {
                tooltip: Some(Tooltip {
                    request: 1,
                    content: Some(Box::new(container(hostile.clone(), vec![]))),
                    hoverable: true,
                    delay_ms: u64::MAX,
                }),
                ..hostile
            },
            vec![],
        );
        let mut frame = Frame {
            root: Some(root),
            ..Default::default()
        };
        view_wire::sanitize(&mut frame).unwrap();
        let Node::Container(view_wire::ContainerNode { interactivity, .. }) = frame.root.unwrap()
        else {
            unreachable!()
        };
        let tooltip = interactivity.tooltip.as_ref().unwrap();
        assert_eq!(tooltip.delay_ms, 60_000);
        let Node::Container(view_wire::ContainerNode {
            interactivity: nested,
            ..
        }) = tooltip.content.as_deref().unwrap()
        else {
            unreachable!()
        };
        for interaction in [&interactivity, nested] {
            assert_eq!(interaction.window_control_area, None);
            assert_eq!(
                interaction.focus.as_ref().unwrap().size.width,
                Some(px(0.).into())
            );
            assert_eq!(interaction.focus.as_ref().unwrap().opacity, Some(1.));
            assert_eq!(
                interaction.in_focus.as_ref().unwrap().margin.left,
                Some(px(0.).into())
            );
            assert_eq!(
                interaction.focus_visible.as_ref().unwrap().text.font_size,
                Some(px(512.).into())
            );
        }
    }
}

#[test]
fn tooltip_content_and_regular_children_share_one_node_budget() {
    let root = container(
        Interactivity {
            tooltip: Some(Tooltip {
                request: 1,
                content: Some(Box::new(container(
                    Interactivity::default(),
                    (0..view_wire::MAX_NODES).map(|_| Node::empty()).collect(),
                ))),
                hoverable: false,
                delay_ms: 0,
            }),
            ..Default::default()
        },
        vec![Node::empty()],
    );
    let mut frame = Frame {
        root: Some(root),
        ..Default::default()
    };
    view_wire::sanitize(&mut frame).unwrap();
    let root = frame.root.unwrap();
    let Node::Container(view_wire::ContainerNode { interactivity, .. }) = &root else {
        unreachable!()
    };
    let tooltip_nodes = interactivity
        .tooltip
        .as_ref()
        .unwrap()
        .content
        .as_ref()
        .unwrap()
        .count();
    assert!(root.count() + tooltip_nodes <= view_wire::MAX_NODES);
}

fn text() -> Node {
    Node::Text(view_wire::TextNode {
        id: None,
        style: Default::default(),
        content: "row".into(),
        heading: None,
        live: None,
    })
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
