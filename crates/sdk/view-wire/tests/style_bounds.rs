//! `StyleRefinement` on the wire: every conditional style is bounded by one
//! sanitize walk, a style change is a props patch that receives the same
//! bounds, and anchored offsets keep their sign while their magnitude is cut.
use gpui::{StyleRefinement, Styled, px};
use view_wire::{
    Anchor, AnchoredFitMode, AnchoredPositionMode, ContainerNode, Frame, GroupRefinement,
    Interactivity, Node, Patch, TextNode, decode, encode, sanitize,
};

fn container(style: StyleRefinement) -> Node {
    Node::Container(view_wire::ContainerNode {
        id: None,
        style,
        interactivity: Interactivity::default(),
        children: vec![Node::Text(view_wire::TextNode {
            id: None,
            style: StyleRefinement::default(),
            content: "stable child".into(),
            heading: None,
            live: None,
        })],
    })
}

#[test]
fn whole_frames_bound_base_and_every_conditional_style() {
    let hostile = StyleRefinement::default()
        .w(px(f32::INFINITY))
        .m(px(-900.))
        .opacity(7.);
    let mut root = container(hostile.clone());
    let Node::Container(view_wire::ContainerNode { interactivity, .. }) = &mut root else {
        unreachable!()
    };
    interactivity.hover = Some(hostile.clone());
    interactivity.active = Some(hostile.clone());
    interactivity.group_hover = Some(GroupRefinement {
        group: "row".into(),
        style: hostile.clone(),
    });
    interactivity.group_active = Some(GroupRefinement {
        group: "row".into(),
        style: hostile,
    });
    let mut frame = Frame {
        root: Some(root),
        ..Default::default()
    };
    view_wire::sanitize(&mut frame).unwrap();
    let Node::Container(view_wire::ContainerNode {
        style,
        interactivity,
        ..
    }) = frame.root.unwrap()
    else {
        unreachable!()
    };
    for style in [
        style,
        interactivity.hover.unwrap(),
        interactivity.active.unwrap(),
        interactivity.group_hover.unwrap().style,
        interactivity.group_active.unwrap().style,
    ] {
        assert_eq!(style.size.width, Some(px(0.).into()));
        assert_eq!(style.margin.left, Some(px(0.).into()));
        assert_eq!(style.opacity, Some(1.));
    }
}

#[test]
fn style_changes_are_props_and_patches_receive_the_same_bounds() {
    let mut old = container(StyleRefinement::default().w(px(100.)));
    let mut changed = container(StyleRefinement::default().w(px(1e20)));
    let patches = view_wire::diff(&mut old, &mut changed);
    assert!(matches!(patches.as_slice(), [Patch::Props { path, .. }] if path.is_empty()));
    let encoded = view_wire::encode(&patches);
    let patches = view_wire::decode(&encoded).unwrap();
    view_wire::apply(&mut old, patches).unwrap();
    let Node::Container(view_wire::ContainerNode {
        style, children, ..
    }) = old
    else {
        unreachable!()
    };
    assert_eq!(style.size.width, Some(px(8192.).into()));
    assert!(
        matches!(&children[0], Node::Text (view_wire::TextNode { content, .. }) if content == "stable child")
    );
}

#[test]
fn text_styles_are_bounded_in_the_same_walk() {
    let mut frame = Frame {
        root: Some(Node::Text(view_wire::TextNode {
            id: None,
            style: StyleRefinement::default().text_size(px(1e20)).w(px(1e20)),
            content: "text".into(),
            heading: None,
            live: None,
        })),
        ..Default::default()
    };
    view_wire::sanitize(&mut frame).unwrap();
    let Node::Text(view_wire::TextNode { style, .. }) = frame.root.unwrap() else {
        unreachable!()
    };
    assert_eq!(style.size.width, Some(px(8192.).into()));
    assert_eq!(style.text.font_size, Some(px(512.).into()));
}

#[test]
fn fingerprints_keep_refinement_field_names() {
    let width = container(StyleRefinement::default().w(px(12.)));
    let height = container(StyleRefinement::default().h(px(12.)));
    let padding = container(StyleRefinement::default().pl(px(12.)));
    let margin = container(StyleRefinement::default().ml(px(12.)));
    assert_ne!(width.fingerprint(), height.fingerprint());
    assert_ne!(padding.fingerprint(), margin.fingerprint());
    assert_eq!(width.fingerprint(), width.clone().fingerprint());
}

#[test]
fn actual_gpui_styled_payloads_roundtrip_and_patch_without_replacing_children() {
    let text = TextNode {
        content: "retained child".into(),
        ..Default::default()
    }
    .text_size(px(13.))
    .text_color(gpui::rgb(0xabcdef));
    let container = ContainerNode {
        children: vec![Node::Text(text)],
        ..Default::default()
    }
    .flex()
    .gap_2()
    .p_4()
    .rounded_md()
    .bg(gpui::rgb(0x112233));
    let mut old = Node::Container(container.clone());
    let mut changed = Node::Container(container.p_6());
    let patches = view_wire::diff(&mut old, &mut changed);
    assert!(matches!(patches.as_slice(), [Patch::Props { path, .. }] if path.is_empty()));
    let patches = view_wire::decode(&view_wire::encode(&patches)).unwrap();
    view_wire::apply(&mut old, patches).unwrap();
    assert_eq!(old, changed);
    let Node::Container(container) = old else {
        unreachable!()
    };
    let [Node::Text(text)] = container.children.as_slice() else {
        unreachable!()
    };
    assert_eq!(text.content, "retained child");
    assert_eq!(text.style.text.font_size, Some(px(13.).into()));
}

#[test]
fn anchored_preserves_local_offsets_and_bounds_untrusted_coordinates() {
    let anchored = |x, y| Node::Anchored {
        anchor: Anchor::TopLeft,
        fit: AnchoredFitMode::SnapToWindow,
        position: Some([x, y]),
        position_mode: AnchoredPositionMode::Local,
        offset: Some([0.; 2]),
        children: vec![Node::Text(view_wire::TextNode {
            id: None,
            style: gpui::StyleRefinement::default()
                .w(gpui::px(f32::INFINITY))
                .h(gpui::px(-1.0)),
            content: String::new(),
            heading: None,
            live: None,
        })],
    };
    for (x, y, expected) in [
        (-4.0, 6.0, (-4.0, 6.0)),
        (f32::NAN, f32::INFINITY, (0.0, 8192.0)),
        (f32::NEG_INFINITY, 9000.0, (-8192.0, 8192.0)),
    ] {
        let mut frame = Frame {
            root: Some(anchored(x, y)),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        let node: Node = decode(&encode(&frame.root.unwrap())).unwrap();
        assert_eq!(node.children().len(), 1);
        let Node::Anchored {
            position, children, ..
        } = node
        else {
            unreachable!()
        };
        assert_eq!(position, Some([expected.0, expected.1]));
        let Node::Text(view_wire::TextNode { style, .. }) = &children[0] else {
            unreachable!()
        };
        // Native refinements strip nonfinite dimensions instead of expanding them.
        assert_eq!(style.size.width, Some(gpui::px(0.).into()));
        assert_eq!(style.size.height, Some(gpui::px(0.).into()));
    }
}
