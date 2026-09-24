use super::*;
use ducktape_view_guest::{StyleRefinement, Styled};

#[test]
fn action_strip_keeps_the_rows_hover_and_is_not_inside_selection_target() {
    let (mut cx, view) = opened();
    assert!(
        cx.find("chat-message-m1-actions").is_none(),
        "no strip on a row the pointer is not over"
    );
    hover(&mut cx, &view, 1);
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
    // An occluding strip took the row's group hover away as the pointer
    // reached it: it hid itself and could not be clicked. It stays under
    // the group; its buttons claim their click from the card instead.
    assert!(
        !interactivity.occlude,
        "the strip must not take the row's hover from under the pointer"
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

/// The pointer over message `seq`'s row, as the host reports it.
pub(super) fn hover(cx: &mut TestAppContext, view: &Entity<Chat>, seq: u64) {
    view.update(cx, |chat, _, cx| {
        chat.hovered = Some((Pane::Timeline, seq));
        cx.notify();
    });
    cx.run_until_parked();
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

#[test]
fn a_control_on_a_card_keeps_its_click_from_the_card_beneath() {
    use ducktape_view_guest::{ClickEvent, Point, px};
    let pointer = |x: f32| {
        let button = wire::click::ButtonEvent {
            button: wire::click::MouseButton::Left,
            position: Point {
                x: px(x),
                y: px(9.),
            },
            modifiers: Default::default(),
            click_count: 1,
        };
        ClickEvent::from(wire::click::Click::Mouse {
            down: button.clone(),
            up: button,
            first_mouse: false,
        })
    };
    let mut chat = Chat::default();
    // GPUI hands one click to the control and then to the card under it
    chat.claim(&pointer(40.));
    assert!(chat.was_claimed((40., 9.)), "the card stands down");
    assert!(
        !chat.was_claimed((40., 9.)),
        "once: the next click is the card's"
    );
    chat.claim(&pointer(40.));
    assert!(
        !chat.was_claimed((41., 9.)),
        "a click elsewhere is the card's"
    );
    // a key press reaches only the focused control, never the card
    chat.claim(&ClickEvent::default());
    assert!(!chat.was_claimed((0., 0.)));
}

#[test]
fn replies_read_as_a_button() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        if let Some(Loaded::Ready(rows)) = chat.room.as_mut().map(|room| &mut room.messages) {
            rows[0].reply_count = 3;
        }
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        style,
        interactivity,
        ..
    })) = cx.find("chat-message-m1-replies")
    else {
        panic!("replies button")
    };
    assert_eq!(interactivity.role, Some(ducktape_view_guest::Role::Button));
    assert!(interactivity.focusable && interactivity.hover.is_some());
    assert!(
        interactivity.focus_visible.is_some(),
        "a visible focus ring"
    );
    assert_eq!(
        style.mouse_cursor,
        Some(ducktape_view_guest::CursorStyle::PointingHand)
    );
    assert!(cx.has_text("Open thread →"));
    cx.simulate_click("chat-message-m1-replies");
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().unwrap().thread.as_ref().map(|t| t.root),
            Some(1)
        );
    });
}

#[test]
fn the_picker_searches_and_enter_picks_the_first_match() {
    let (mut cx, view) = opened();
    hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    assert!(cx.find("chat-reaction-🔥").is_some(), "the frequent row");
    assert!(
        cx.find("chat-reaction-Smileys-😀").is_some(),
        "the first tab"
    );
    cx.simulate_click("chat-reaction-tab-Food");
    assert!(cx.find("chat-reaction-Food-🍕").is_some());
    let focus = ui::menu::focus_key(Pane::Timeline, Mode::Reactions);
    cx.simulate_input(&focus, "duck");
    assert!(cx.has_text("1 MATCH"));
    cx.simulate_submit(&focus);
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<Submit<ChatApi>>()
            .iter()
            .any(|op| matches!(op, ChatMsg::AddReaction { emoji, .. } if emoji == "🦆"))
    );
    view.read(|chat| {
        assert!(chat.menu.is_none());
        assert_eq!(chat.recent_emoji.first().map(String::as_str), Some("🦆"));
    });
}
