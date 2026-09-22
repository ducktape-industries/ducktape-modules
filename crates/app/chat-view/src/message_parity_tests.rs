use super::*;
use ducktape_view_guest::{StyleRefinement, Styled};

#[test]
fn action_strip_uses_native_group_visibility_and_is_not_inside_selection_target() {
    let (cx, _) = opened();
    let Some(wire::Node::Container {
        style,
        interactivity,
        ..
    }) = cx.find("chat-message-m1-actions")
    else {
        panic!("action strip")
    };
    assert_eq!(
        style.visibility,
        StyleRefinement::default().invisible().visibility
    );
    assert_eq!(
        interactivity.group_hover.as_ref().unwrap().style.visibility,
        StyleRefinement::default().visible().visibility
    );
    let card = cx.find("chat-message-m1").unwrap();
    fn has_actions(node: &wire::Node) -> bool {
        node.key() == Some("chat-message-m1-actions") || node.children().iter().any(has_actions)
    }
    assert!(
        !has_actions(card),
        "native action clicks must not bubble through selection"
    );
    assert!(cx.find("chat-message-m1-thumbs-up").is_some());
}

#[test]
fn copy_range_keeps_its_distinct_message_plate() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        chat.copy = Some(CopyRange {
            pane: Pane::Timeline,
            anchor: 1,
            head: 2,
        });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container { style, .. }) = cx.find("chat-message-m1") else {
        panic!("message card")
    };
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|color| color.as_solid()),
        Some(ducktape_view_guest::Theme::light().surface_raised)
    );
}
