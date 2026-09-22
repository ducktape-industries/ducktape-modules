use gpui::Styled;
use view_wire::{Anchor, AnchoredFitMode, AnchoredPositionMode, Frame, Node, decode, encode, sanitize};

#[test]
fn anchored_preserves_local_offsets_and_bounds_untrusted_coordinates() {
    let anchored = |x, y| Node::Anchored {
        anchor: Anchor::TopLeft,
        fit: AnchoredFitMode::SnapToWindow,
        position: Some([x, y]),
        position_mode: AnchoredPositionMode::Local,
        offset: Some([0.; 2]),
        children: vec![Node::Text {
            id: None,
            style: gpui::StyleRefinement::default().w(gpui::px(f32::INFINITY)).h(gpui::px(-1.0)),
            content: String::new(), heading: None, live: None,
        }],
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
        let Node::Anchored { position, children, .. } = node else { unreachable!() };
        assert_eq!(position, Some([expected.0, expected.1]));
        let Node::Text { style, .. } = &children[0] else { unreachable!() };
        // Native refinements strip nonfinite dimensions instead of expanding them.
        assert_eq!(style.size.width, Some(gpui::px(0.).into()));
        assert_eq!(style.size.height, Some(gpui::px(0.).into()));
    }
}
