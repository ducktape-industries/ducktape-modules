use gpui::{Styled, px};
use view_wire::{ContainerNode, Node, Patch, TextNode};

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
