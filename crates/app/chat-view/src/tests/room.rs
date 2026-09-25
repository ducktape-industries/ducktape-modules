//! The open room: its rows, panes, lists and links.
use super::*;

#[test]
fn the_room_shows_its_rows_intro_and_actions() {
    let (mut cx, view) = opened();
    let texts = cx.texts();
    assert!(cx.has_text("hello"), "{texts:?}");
    assert!(cx.has_text("eddy") && cx.has_text("reviewer"));
    assert!(cx.has_text("Agent"), "a program account wears the badge");
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("This is the very beginning of #General")),
        "{texts:?}"
    );
    // Each paragraph keeps a stable typed element id for interaction queries.
    for seq in [1, 2] {
        assert!(cx.find(&format!("chat-message-m{seq}-block-0")).is_some());
    }
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::Reactions))
            .is_some(),
        "the host focus target must exist in the open menu"
    );
    assert!(
        cx.host()
            .asked::<ducktape_view_guest::methods::HostWidget>()
            .iter()
            .any(|command| {
                matches!(command, wire::WidgetCommand::Focus { target }
                if *target == vec![wire::ElementIdWire::Name(
                    ui::menu::focus_key(Pane::Timeline, Mode::Reactions).into()
                )])
            })
    );
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|m| m.mode == Mode::Reactions)
        )
    });
    cx.simulate_click("chat-reaction-🔥");
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<Submit<ChatApi>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, .. } if emoji == "🔥"))
    );
    view.read(|chat| assert!(chat.menu.is_none()));
    cx.simulate_click("chat-message-m1-more");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::More))
            .is_some()
    );
    assert!(cx.has_text("Reply in thread") && cx.has_text("Copy link"));
    cx.simulate_click("chat-message-m1-thread");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().unwrap().thread.as_ref().map(|t| t.root),
            Some(1)
        )
    });
    assert!(cx.has_text("Thread") && cx.has_text("Reply in thread"));
    cx.simulate_click("chat-thread-close");
    view.read(|chat| assert!(chat.room.as_ref().unwrap().thread.is_none()));
    cx.simulate_click("chat-room-details");
    assert!(cx.has_text("Archive channel"));
    cx.simulate_input("chat-details-name-input", "Lobby");
    cx.simulate_click("chat-details-rename-button");
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<Submit<ChatApi>>()
            .iter()
            .any(|op| matches!(op, Op::RenameChannel { name, .. } if name == "Lobby"))
    );
}

#[test]
fn viewport_and_pane_dividers_keep_their_behavior_routes() {
    let (mut cx, view) = opened();
    let full = StyleRefinement::default().size_full();
    let Some(wire::Node::Sensor {
        on_show: Some(_),
        on_resize: Some(_),
        style,
        ..
    }) = cx.find("chat-viewport")
    else {
        panic!("chat viewport sensor")
    };
    assert_eq!(style.size.width, full.size.width);
    assert_eq!(style.size.height, full.size.height);
    cx.simulate_measure("chat-viewport", 640., 480.);
    view.read(|chat| {
        assert_eq!(chat.layout.viewport, (640., 480.));
        assert!(chat.layout.sidebar <= 320.);
    });
    assert!(matches!(
        cx.find("chat-sidebar-resize"),
        Some(wire::Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
    let sidebar = view.read(|chat| chat.layout.sidebar);
    cx.simulate_drag("chat-sidebar-resize", 18., 0.);
    view.read(|chat| assert_eq!(chat.layout.sidebar, sidebar + 18.));

    cx.simulate_click("chat-room-details");
    assert!(matches!(
        cx.find("chat-details-resize"),
        Some(wire::Node::ResizeHandle {
            on_drag: Some(_),
            ..
        })
    ));
}

#[test]
fn timeline_retains_virtual_tail_anchoring_and_scroll_feedback() {
    let (cx, _) = opened();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode { children, .. })) =
        cx.find("chat-message-list")
    else {
        panic!("message list keeps its authored container identity")
    };
    assert!(matches!(children.as_slice(), [wire::Node::List {
        item_count: 3,
        alignment: wire::ListAlignment::Bottom,
        following_tail: true,
        scroll_handler: Some(_),
        children,
        ..
    }] if children.len() == 3));
}

#[test]
fn unread_rooms_carry_a_dot_and_the_open_room_a_divider() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, window, cx| {
        chat.open("other".into(), window, cx);
        chat.channels_arrived(
            vec![
                channel("general", "General", 9),
                channel("other", "Other", 0),
            ],
            cx,
        );
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    view.update(&mut cx, |chat, _, cx| {
        chat.reads.entering = true;
        chat.room = Some(Room {
            id: "general".into(),
            messages: Loaded::Ready(vec![row(3, 7, "old"), row(9, 8, "new")]),
            at_tail: true,
            reaches_head: true,
            ..Room::default()
        });
        chat.channels_arrived(vec![channel("general", "General", 9)], cx);
        assert_eq!(chat.reads.boundary, 3);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("New messages"));
}

/// A full room (one window of rows), each with markup, a reaction and a
/// thread, renders inside the host's frame budget and under a regression
/// guard on its bytes: the native proxy for a render's fuel.
#[test]
fn a_full_room_renders_inside_the_frame_budget() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let rows: Vec<MsgRow> = (1..=WINDOW as u64)
        .map(|seq| {
            let text = format!("row {seq}: **bold**, `code` and a [link](https://x.example/{seq})");
            let mut row = row(seq, if seq % 2 == 0 { 7 } else { 8 }, &text);
            row.blocks = chat::parse_message(&text);
            row.reply_count = seq % 3;
            row.reactions = vec![::chat::Reaction {
                emoji: "👍".into(),
                count: seq,
                reacted_by_me: seq % 2 == 0,
            }];
            row
        })
        .collect();
    cx.host().handle::<Ask<ChatApi>>(move |query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { .. } => {
                Reply::Channels(page(vec![channel("general", "General", WINDOW as u64)]))
            }
            Query::Roots { .. } => Reply::Roots(page(rows.clone())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    cx.open::<Chat>();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains(&format!("row {WINDOW}"))),
        "{:?}",
        cx.texts()
    );
    let bytes = cx.frame_bytes();
    // A regression guard, not a host limit: about 1.5x what a full room drew
    // when measured. The host's own limits are the sanitize check inside
    // `frame_bytes`. Tighten when the room slims, raise only on purpose.
    const REGRESSION_GUARD: usize = 220_000;
    assert!(
        bytes < REGRESSION_GUARD,
        "a {WINDOW}-row room drew {bytes} bytes, over its guard {REGRESSION_GUARD}"
    );
}

/// The timeline's list keeps its path when a message menu opens over the
/// room: the host keys the list's scroll by it, and a new path started the
/// room over at its latest message.
#[test]
fn a_menu_opening_leaves_the_timeline_where_it_was() {
    fn list_path(node: &wire::Node) -> Option<Vec<wire::ElementIdWire>> {
        if let wire::Node::List { path, .. } = node {
            return Some(path.clone());
        }
        node.children().iter().find_map(list_path)
    }
    let (mut cx, view) = opened();
    let closed = list_path(cx.root()).expect("the timeline is a list");
    view.update(&mut cx, |chat, window, cx| {
        cx.notify();
        chat.open_menu(Pane::Timeline, 2, 0, Mode::More, window, cx);
    });
    cx.run_until_parked();
    assert!(cx.find("chat-menu-copy-link").is_some(), "the menu is open");
    assert_eq!(list_path(cx.root()), Some(closed));
}

/// A thread with nothing under its root says so, and its reply field takes
/// the keys once the replies are read.
#[test]
fn an_empty_thread_says_so_and_its_field_takes_focus() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        cx.notify();
        chat.open_thread(1, cx);
    });
    cx.run_until_parked();
    assert!(cx.has_text("No replies yet"));
    let field = wire::ElementIdWire::Name("draft-general-1/editor".into());
    assert!(
        cx.host()
            .asked::<ducktape_view_guest::methods::HostWidget>()
            .iter()
            .any(|command| matches!(command, wire::WidgetCommand::Focus { target } if *target == vec![field.clone()]))
    );
}

/// A forge room's link spells its `:` as `%3A`; the app hands the view the
/// route decoded, and the view lands in that room.
#[test]
fn a_link_to_a_forge_room_lands_in_it() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let routes = cx.host().stream::<api::HostRoute>();
    let props = cx.host().stream::<HostSession>();
    cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    let link = crate::links::channel_link("testnet#0a1b2c3d", "forge:web:3", None).unwrap();
    assert!(link.ends_with("/chat/forge%3Aweb%3A3"), "{link}");
    // what the app does with a chain link: the tail, decoded, joined
    routes.push(ducklink::Link::parse(&link).unwrap().tail.join("/"));
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "forge:web:3"));
}

/// A dm belongs to its two peers alike: its details list them, with no way
/// to add a third, remove either, rename or archive it, as the program
/// refuses each.
#[test]
fn a_dms_details_show_its_two_people_and_nothing_to_reshape() {
    let (mut cx, view) = opened();
    let seat = |number| chat::MemberRow {
        party: Party::Account(number),
        height: 1,
        time: 1,
    };
    let seat_both = |cx: &mut TestAppContext| {
        view.update(cx, |chat, _, cx| {
            chat.room.as_mut().unwrap().members = Loaded::Ready(vec![seat(7), seat(8)]);
            cx.notify();
        });
        cx.run_until_parked();
    };
    cx.simulate_click("chat-room-details");
    seat_both(&mut cx);
    assert!(cx.find("chat-details-add-member").is_some());
    assert!(cx.has_text("Remove"));
    cx.simulate_click("chat-sidebar-dm-8");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    seat_both(&mut cx);
    assert!(cx.has_text("reviewer") && cx.has_text("eddy"));
    assert!(cx.has_text("Conversation details") && !cx.has_text("Channel details"));
    for id in [
        "chat-details-name-input",
        "chat-details-rename-button",
        "chat-details-archive",
    ] {
        assert!(cx.find(id).is_none(), "{id} offered in a dm");
    }
    assert!(cx.find("chat-details-add-member").is_none());
    assert!(cx.find("chat-details-member-input").is_none());
    assert!(!cx.has_text("Remove"));
}
