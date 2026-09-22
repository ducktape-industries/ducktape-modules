use gpui::{StyleRefinement, Styled, px};
use view_wire::{Frame, Interactivity, Node, Tooltip, WindowControlArea};

fn container(interactivity: Interactivity, children: Vec<Node>) -> Node {
    Node::Container {
        id: None,
        style: StyleRefinement::default(),
        interactivity,
        children,
    }
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
        let Node::Container { interactivity, .. } = frame.root.unwrap() else {
            unreachable!()
        };
        let tooltip = interactivity.tooltip.as_ref().unwrap();
        assert_eq!(tooltip.delay_ms, 60_000);
        let Node::Container {
            interactivity: nested,
            ..
        } = tooltip.content.as_deref().unwrap()
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
    let Node::Container { interactivity, .. } = &root else {
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
