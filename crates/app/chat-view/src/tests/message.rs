use super::*;
use ducktape_view_guest::{StyleRefinement, Styled};

#[test]
fn action_strip_uses_native_group_visibility_and_is_not_inside_selection_target() {
    let (cx, _) = opened();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        style,
        interactivity,
        ..
    })) = cx.find("chat-message-m1-actions")
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
    // GPUI dispatches a click to every interactive element whose hitbox
    // contains it, not just the topmost one — being a sibling of `card`
    // (checked below) is not enough to keep a click here off `card`'s
    // row-select handler too, which would clobber the mode `open_menu`
    // just set back to `Mode::Toolbar`. Only `occlude` stops that.
    assert!(
        interactivity.occlude,
        "action strip must occlude so its clicks don't also fire card's row-select"
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
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode { style, .. })) =
        cx.find("chat-message-m1")
    else {
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

#[test]
fn reaction_rows_keep_add_action_and_selected_accessibility() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        chat.room.as_mut().unwrap().messages.ready_mut().unwrap()[0]
            .reactions
            .push(chat::ReactionSummary {
                emoji: "🔥".into(),
                count: 2,
                reacted_by_me: true,
            });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        interactivity, ..
    })) = cx.find("chat-message-m1-reaction-🔥")
    else {
        panic!("reaction pill")
    };
    assert_eq!(interactivity.aria.description.as_deref(), Some("🔥"));
    assert!(interactivity.aria.toggled.is_some());
    assert!(cx.find("chat-message-m1-reaction-add").is_some());
    cx.simulate_click("chat-message-m1-reaction-add");
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|menu| menu.mode == Mode::Reactions && menu.seq == 1)
        )
    });
}

#[test]
fn thread_root_uses_reply_count_as_a_separator() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap()[0].reply_count = 2;
        room.thread = Some(Thread {
            root: 1,
            replies: Loaded::Ready(Vec::new()),
            ..Thread::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-message-m1-reply-separator").is_some());
    assert!(cx.has_text("2 replies"));
}
