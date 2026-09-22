use super::*;
use ducktape_view_guest::Entity;
use ducktape_view_guest::testing::TestAppContext;
use ducktape_view_guest::wire;

fn channel(id: &str, name: &str, head_seq: u64) -> ChannelInfo {
    ChannelInfo {
        channel: ::chat::ChannelRow {
            id: id.into(),
            name: name.into(),
            created_at: 0,
            post_policy: PostPolicy::Open,
            owner: "acct:7".into(),
            archived: false,
            huddle: Vec::new(),
            voice: false,
        },
        head_seq,
    }
}

#[test]
fn preferred_window_keeps_the_original_baseline() {
    assert_eq!(<Chat as View>::PREFERRED_WINDOW_SIZE, "1180,760");
}

#[test]
fn the_root_tracks_the_shared_theme_and_is_accessible() {
    let (mut cx, _) = opened();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode { style, .. })) =
        cx.find("chat-root")
    else {
        panic!("chat root is a styled container");
    };
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid()),
        Some(dark.background)
    );
    assert_eq!(style.text.color, Some(dark.foreground));
    cx.assert_accessible();
}

fn row(seq: u64, author: &str, text: &str) -> MsgRow {
    MsgRow {
        channel_id: "general".into(),
        seq,
        message_id: format!("m{seq}"),
        author: author.into(),
        height: 1,
        blocks: vec![chat::Block::paragraph(text)],
        text: text.into(),
        ..MsgRow::default()
    }
}

fn configure(cx: &mut TestAppContext) {
    cx.host()
        .handle::<ducktape_view_guest::caps::Widget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<ViewOf<ChatApi>>(|query| {
        Ok(match query {
            ChatViewQuery::Accounts { .. } => ChatViewReply::Accounts(vec![
                chat::AccountRow {
                    number: 7,
                    name: "eddy".into(),
                    program: false,
                    keys: vec!["0102".into()],
                },
                chat::AccountRow {
                    number: 8,
                    name: "reviewer".into(),
                    program: true,
                    keys: Vec::new(),
                },
            ]),
            ChatViewQuery::Channels { .. } => ChatViewReply::Channels {
                channels: vec![channel("general", "General", 3), channel("dm-7-8", "dm", 1)],
                has_more: false,
                next_after: None,
            },
            ChatViewQuery::Roots { channel_id, .. } => ChatViewReply::Roots {
                roots: if channel_id == "general" {
                    vec![row(1, "acct:7", "hello"), row(2, "acct:8", "**hi** there")]
                } else {
                    Vec::new()
                },
                has_more: false,
                next_before_seq: None,
            },
            ChatViewQuery::Members { .. } => ChatViewReply::Members {
                members: Vec::new(),
                has_more: false,
                next_after: None,
            },
            ChatViewQuery::Thread { .. } => ChatViewReply::Thread {
                root: None,
                replies: Vec::new(),
                has_more: false,
                next_reply_seq: None,
            },
            ChatViewQuery::Search { text, .. } => {
                assert_eq!(text, "hello");
                ChatViewReply::Hits(MessageHits {
                    hits: vec![row(1, "acct:7", "hello")],
                    capped: false,
                })
            }
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().never::<LiveChanges>();
    cx.host()
        .handle::<Submit<ChatApi>>(|_| Ok(serde_json::Value::Null));
}

/// Boots, seats a reader, lists rooms and opens `general` with two rows.
fn opened() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    assert!(cx.has_text("Not connected"));
    props.push(PropsItem::Session(Box::new(Session {
        me: "acct:7".into(),
        me_key: "0102".into(),
        connected: true,
        network_name: "duck".into(),
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    })));
    visible.push(true);
    cx.run_until_parked();
    assert!(cx.has_text("General"));
    assert!(cx.has_text("No channel open"));
    assert!(cx.has_text("Channels"));
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        style,
        interactivity,
        ..
    })) = cx.find("chat-sidebar-channel-general")
    else {
        panic!("channel row is a native container");
    };
    assert!(style.size.width.is_some(), "channel rows fill the sidebar");
    assert_eq!(interactivity.role, Some(ducktape_view_guest::Role::Button));
    assert_eq!(interactivity.aria.selected, Some(true));
    assert!(
        interactivity.on_click.is_some(),
        "channel rows keep their route"
    );
    (cx, view)
}

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
    cx.simulate_click("chat-message-m1-react");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::Reactions))
            .is_some(),
        "the host focus target must exist in the open menu"
    );
    assert!(
        cx.host()
            .asked::<ducktape_view_guest::caps::Widget>()
            .iter()
            .any(|command| {
                matches!(command, wire::WidgetCommand::Focus { target }
                if target == &vec![wire::ElementIdWire::Name(
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
            .any(|op| matches!(op, ChatMsg::AddReaction { emoji, .. } if emoji == "🔥"))
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
            .any(|op| matches!(op, ChatMsg::RenameChannel { name, .. } if name == "Lobby"))
    );
}

#[test]
fn viewport_and_pane_dividers_keep_their_behavior_routes() {
    let (mut cx, view) = opened();
    assert!(matches!(
        cx.find("chat-viewport"),
        Some(wire::Node::Sensor {
            on_show: Some(_),
            on_resize: Some(_),
            ..
        })
    ));
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
fn menus_and_dialogs_are_modal_overlays_with_dismiss_routes() {
    let (mut cx, view) = opened();
    cx.simulate_click("chat-message-m1-more");
    assert!(matches!(
        cx.find("chat-menu-overlay"),
        Some(wire::Node::Overlay {
            label: Some(label),
            on_dismiss: Some(_),
            children,
            ..
        }) if label == "Message menu" && children.len() == 2
    ));

    cx.simulate_dismiss("chat-menu-overlay");
    view.read(|chat| assert!(chat.menu.is_none()));
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(matches!(
        cx.find("chat-create-overlay"),
        Some(wire::Node::Overlay {
            label: Some(label),
            on_dismiss: Some(_),
            children,
            ..
        }) if label == "Create channel" && children.len() == 2
    ));
    cx.simulate_dismiss("chat-create-overlay");
    view.read(|chat| assert!(chat.create.is_none()));
}

#[test]
fn message_menu_preserves_disabled_actions_and_executes_enabled_routes() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::More,
            at: (611., 455.),
        });
        chat.session.me.clear();
        cx.notify();
    });
    cx.run_until_parked();
    for id in [
        "chat-menu-add-reaction",
        "chat-menu-edit",
        "chat-menu-delete",
    ] {
        let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
            interactivity,
            ..
        })) = cx.find(id)
        else {
            panic!("{id} remains a visible native menu row");
        };
        assert_eq!(interactivity.aria.disabled, Some(true));
        assert!(interactivity.on_click.is_none());
    }
    assert!(cx.has_text("😀") && cx.has_text("✎") && cx.has_text("🗑"));

    view.update(&mut cx, |chat, _, cx| {
        chat.session.me = "acct:7".into();
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-delete");
    assert!(cx.has_text("Delete this message?"));
    view.update(&mut cx, |chat, _, cx| {
        chat.session.busy = true;
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        interactivity, ..
    })) = cx.find("chat-menu-confirm-delete")
    else {
        panic!("busy delete confirmation remains visible");
    };
    assert_eq!(interactivity.aria.disabled, Some(true));
    assert!(interactivity.on_click.is_none());
    view.update(&mut cx, |chat, _, cx| {
        chat.session.busy = false;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-confirm-delete");
    cx.run_until_parked();
    assert!(cx.host().asked::<Submit<ChatApi>>().iter().any(|op| {
        matches!(op, ChatMsg::DeleteMessage { channel_id, seq: 1 } if channel_id == "general")
    }));
}

#[test]
fn reaction_picker_keeps_labels_and_its_stable_action_id() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::Reactions,
            at: (333., 222.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        interactivity, ..
    })) = cx.find("chat-reaction-🔥")
    else {
        panic!("reaction is a native cell");
    };
    assert_eq!(interactivity.aria.label.as_deref(), Some("Add reaction"));
    assert_eq!(interactivity.aria.description.as_deref(), Some("🔥"));
    cx.simulate_click("chat-reaction-🔥");
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<Submit<ChatApi>>()
            .iter()
            .any(|op| { matches!(op, ChatMsg::AddReaction { emoji, .. } if emoji == "🔥") })
    );
}

#[test]
fn a_send_shows_pending_then_lands_and_a_refusal_is_a_banner() {
    let (mut cx, view) = opened();
    let pending = MsgRow {
        message_id: "p1".into(),
        author: "acct:7".into(),
        blocks: vec![chat::Block::paragraph("on its way")],
        ..MsgRow::default()
    };
    view.update(&mut cx, |chat, _, cx| {
        chat.room.as_mut().unwrap().pending.push(pending.clone());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("on its way") && cx.has_text("sending…"));
    view.update(&mut cx, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages
            .ready_mut()
            .unwrap()
            .push(MsgRow { seq: 3, ..pending });
        room.settle();
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("on its way") && !cx.has_text("sending…"));
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert!(cx.has_text("1 result for “hello”"), "{:?}", cx.texts());
    cx.simulate_click("chat-sidebar-clear-search");
    view.read(|chat| assert!(chat.search.query.is_empty()));
    cx.host().handle::<Id>(|kind| {
        assert_eq!(kind, "channel");
        Ok("chan-1".into())
    });
    cx.host().refuse::<Submit<ChatApi>>("no", "no");
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(cx.has_text("Create a channel"));
    cx.simulate_input("chat-create-name", "random");
    cx.simulate_click("chat-create-members");
    cx.simulate_submit("chat-create-name");
    cx.run_until_parked();
    assert!(cx.host().asked::<Submit<ChatApi>>().iter().any(|op| matches!(op, ChatMsg::CreateChannel { name, post_policy: PostPolicy::MembersOnly, .. } if name == "random")));
    assert!(cx.has_text("Couldn’t create this channel: no"));
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    configure(&mut restored);
    restored.host().never::<Props>();
    restored.host().never::<Visible>();
    let view = restored.restore::<Chat>(&bytes).unwrap();
    restored.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    assert!(restored.host().asked::<ViewOf<ChatApi>>().len() >= 2);
}

#[test]
fn channel_create_preserves_busy_account_and_voice_gates() {
    fn disabled(cx: &TestAppContext, id: &str) -> bool {
        let Some(wire::Node::Container { interactivity, .. }) = cx.find(id) else {
            panic!("{id} button")
        };
        interactivity.aria.disabled == Some(true) && interactivity.on_click.is_none()
    }

    let (mut cx, view) = opened();
    cx.simulate_click("chat-sidebar-new-channel");
    view.update(&mut cx, |chat, _, cx| {
        chat.create.as_mut().unwrap().busy = true;
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Input {
        options, on_submit, ..
    }) = cx.find("chat-create-name")
    else {
        panic!("channel name input")
    };
    assert!(options.disabled);
    assert!(on_submit.is_none());
    for id in [
        "chat-create-voice",
        "chat-create-members",
        "chat-create-cancel",
        "chat-create-submit",
    ] {
        assert!(disabled(&cx, id), "{id} must stay inert while busy");
    }

    view.update(&mut cx, |chat, _, cx| {
        let create = chat.create.as_mut().unwrap();
        create.busy = false;
        create.voice = true;
        chat.session.me = "user:0102".into();
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-members"));
    assert!(disabled(&cx, "chat-create-submit"));
    assert!(cx.has_text("Create an account to create a channel"));
    assert!(!disabled(&cx, "chat-create-cancel"));
    let submitted = cx.host().asked::<Submit<ChatApi>>().len();
    view.update(&mut cx, |chat, _, cx| chat.create_channel(cx));
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<Submit<ChatApi>>().len(), submitted);

    view.update(&mut cx, |chat, _, cx| {
        chat.session.me = "acct:7".into();
        chat.session.connected = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
    view.update(&mut cx, |chat, _, cx| {
        chat.session.connected = true;
        chat.session.busy = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
}

#[test]
fn unread_rooms_carry_a_dot_and_the_open_room_a_divider() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, window, cx| {
        chat.open("other".into(), window, cx);
        chat.channels_arrived(vec![
            channel("general", "General", 9),
            channel("other", "Other", 0),
        ]);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    view.update(&mut cx, |chat, _, cx| {
        chat.reads.entering = true;
        chat.room = Some(Room {
            id: "general".into(),
            messages: Loaded::Ready(vec![row(3, "acct:7", "old"), row(9, "acct:8", "new")]),
            at_tail: true,
            reaches_head: true,
            ..Room::default()
        });
        chat.channels_arrived(vec![channel("general", "General", 9)]);
        assert_eq!(chat.reads.boundary, 3);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("New messages"));
}

#[path = "message_parity_tests.rs"]
mod message_parity;

#[path = "rich_message_tests.rs"]
mod rich_message;
