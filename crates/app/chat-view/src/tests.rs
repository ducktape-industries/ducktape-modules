use super::*;
use ducktape_view_guest::doors::Query as ViewOf;
use ducktape_view_guest::testing::TestAppContext;
use ducktape_view_guest::wire;
use ducktape_view_guest::{Entity, StyleRefinement, Styled};

fn page<T>(items: Vec<T>) -> ::chat::PageReply<T> {
    ::chat::PageReply {
        height: 1,
        items,
        next: None,
    }
}

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

/// The host doors chat only talks to, never hears back from here.
fn quiet_doors(cx: &mut TestAppContext) {
    cx.host().never::<api::Route>();
    cx.host().never::<ducktape_view_guest::doors::Badge>();
    cx.host().never::<ducktape_view_guest::doors::NotifyPost>();
}

fn configure(cx: &mut TestAppContext) {
    quiet_doors(cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::Widget>(|command| {
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
            ChatViewQuery::Channels { .. } => ChatViewReply::Channels(page(vec![
                channel("general", "General", 3),
                channel("dm-7-8", "dm", 1),
            ])),
            ChatViewQuery::Roots { channel_id, .. } => {
                ChatViewReply::Roots(page(if channel_id == "general" {
                    vec![row(1, "acct:7", "hello"), row(2, "acct:8", "**hi** there")]
                } else {
                    Vec::new()
                }))
            }
            ChatViewQuery::Members { .. } => ChatViewReply::Members(page(Vec::new())),
            ChatViewQuery::Thread { .. } => ChatViewReply::Thread {
                root: None,
                replies: page(Vec::new()),
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
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));
    // The host hands every view the seated key as raw hex, never a handle:
    // resolve it the way identity itself would. "0102" is account 7's own
    // key, matching the roster above; any other key holds no account.
    cx.host()
        .handle::<ViewOf<identity::view::Identity>>(|query| {
            Ok(match query {
                identity::Query::OfKey { key } if key == [0x01, 0x02] => {
                    identity::Reply::Number(Some(7))
                }
                identity::Query::OfKey { .. } => identity::Reply::Number(None),
                query => panic!("unexpected identity query: {query:?}"),
            })
        });
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
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
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
    message::hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    assert!(
        cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::Reactions))
            .is_some(),
        "the host focus target must exist in the open menu"
    );
    assert!(
        cx.host()
            .asked::<ducktape_view_guest::doors::Widget>()
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
fn menus_and_dialogs_are_modal_overlays_with_dismiss_routes() {
    let (mut cx, view) = opened();
    message::hover(&mut cx, &view, 1);
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
fn message_menu_offers_only_what_the_reader_may_do_and_executes_it() {
    let (mut cx, view) = opened();
    let menu_on = |seq| Menu {
        pane: Pane::Timeline,
        seq,
        rev: 0,
        mode: Mode::More,
        at: (611., 455.),
    };
    view.update(&mut cx, |chat, _, cx| {
        chat.menu = Some(menu_on(1));
        chat.me = Loaded::Ready(None);
        cx.notify();
    });
    cx.run_until_parked();
    // a key with no account reads: nothing it would be refused is offered
    for id in [
        "chat-menu-add-reaction",
        "chat-menu-edit",
        "chat-menu-delete",
    ] {
        assert!(cx.find(id).is_none(), "{id} offered to a reader");
    }
    assert!(cx.find("chat-menu-copy-link").is_some());

    // someone else's message in a channel the reader owns: delete, no edit
    view.update(&mut cx, |chat, _, cx| {
        chat.menu = Some(menu_on(2));
        chat.me = Loaded::Ready(Some(7));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-menu-edit").is_none(), "only the author edits");
    assert!(cx.find("chat-menu-delete").is_some(), "the owner deletes");

    view.update(&mut cx, |chat, _, cx| {
        chat.menu = Some(menu_on(1));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("😀") && cx.has_text("✎") && cx.has_text("🗑"));
    view.update(&mut cx, |chat, _, cx| {
        chat.me = Loaded::Ready(Some(7));
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-delete");
    assert!(cx.has_text("Delete this message?"));
    // Anchored near the row it opened from, this popup can overlap the
    // message card beneath it; without occlude, a click on "Delete" here
    // also fires the card's row-select handler, which resets `chat.menu`
    // to `Mode::Toolbar` before `delete_armed` reads it, so the delete is
    // silently dropped (no submit, no error).
    let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
        interactivity, ..
    })) = cx.find(&ui::menu::focus_key(Pane::Timeline, Mode::Delete))
    else {
        panic!("delete confirmation frame")
    };
    assert!(
        interactivity.occlude,
        "delete confirmation popup must occlude so its clicks don't also fire the row beneath"
    );
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
        let Some(wire::Node::Container(ducktape_view_guest::wire::ContainerNode {
            interactivity,
            ..
        })) = cx.find(id)
        else {
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
        chat.me = Loaded::Ready(None);
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
        chat.me = Loaded::Ready(Some(7));
        chat.session.connected = false;
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

#[test]
fn session_key_resolves_to_its_account() {
    let (_cx, view) = opened();
    view.read(|chat| {
        assert_eq!(chat.my_account(), Some(7));
        assert!(chat.holds_account());
        assert_eq!(chat.my_handle(), "acct:7");
    });
}

#[test]
fn an_unregistered_key_stays_read_only() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        account: "ffff".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(chat.my_account(), None);
        assert!(!chat.holds_account());
        assert_eq!(chat.write_refusal(), "no_account");
    });
    cx.simulate_click("chat-sidebar-new-channel");
    cx.run_until_parked();
    assert!(cx.has_text("Create an account to create a channel"));
}

/// The reader creates the account in Settings, then switches to Chat: the
/// seated key never changes, only identity's own state does, so this has to
/// arrive over identity's live stream — not the session's.
#[test]
fn an_account_gained_later_re_enables_create_channel() {
    let registered = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let reply = registered.clone();
    cx.host()
        .handle::<ViewOf<identity::view::Identity>>(move |query| {
            Ok(match query {
                identity::Query::OfKey { key } if key == [0x01, 0x02] => {
                    identity::Reply::Number(reply.get().then_some(7))
                }
                identity::Query::OfKey { .. } => identity::Reply::Number(None),
                query => panic!("unexpected identity query: {query:?}"),
            })
        });
    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    let live = cx.host().stream::<LiveChanges>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-new-channel");
    cx.run_until_parked();
    assert!(cx.has_text("Create an account to create a channel"));

    registered.set(true);
    live.push(Some(1));
    cx.run_until_parked();

    assert!(!cx.has_text("Create an account to create a channel"));
    view.read(|chat| assert_eq!(chat.my_account(), Some(7)));
}

/// A peer who registers their account AFTER this room's roster was first
/// read still shows up under "account N" — the reader's own identity was
/// re-resolved on identity's live stream, but the roster naming everyone
/// ELSE never was, so a fresh signer's messages stayed numbered forever
/// (regression: a two-account chat never named the other side's reply).
#[test]
fn a_peers_name_gained_later_replaces_its_numeric_fallback() {
    let known = std::rc::Rc::new(std::cell::Cell::new(false));
    let has_gary = known.clone();
    let mut cx = TestAppContext::new();
    quiet_doors(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::Widget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<ViewOf<ChatApi>>(move |query| {
        Ok(match query {
            ChatViewQuery::Accounts { .. } => {
                let mut accounts = vec![chat::AccountRow {
                    number: 7,
                    name: "eddy".into(),
                    program: false,
                    keys: vec!["0102".into()],
                }];
                if has_gary.get() {
                    accounts.push(chat::AccountRow {
                        number: 9,
                        name: "gary".into(),
                        program: false,
                        keys: Vec::new(),
                    });
                }
                ChatViewReply::Accounts(accounts)
            }
            ChatViewQuery::Channels { .. } => {
                ChatViewReply::Channels(page(vec![channel("general", "General", 2)]))
            }
            ChatViewQuery::Roots { .. } => ChatViewReply::Roots(page(vec![
                row(1, "acct:7", "hello"),
                row(2, "acct:9", "hi from gary"),
            ])),
            ChatViewQuery::Members { .. } => ChatViewReply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));
    cx.host()
        .handle::<ViewOf<identity::view::Identity>>(|query| {
            Ok(match query {
                identity::Query::OfKey { key } if key == [0x01, 0x02] => {
                    identity::Reply::Number(Some(7))
                }
                identity::Query::OfKey { .. } => identity::Reply::Number(None),
                query => panic!("unexpected identity query: {query:?}"),
            })
        });

    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    let live = cx.host().stream::<LiveChanges>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();

    assert!(
        cx.has_text("account 9"),
        "an unregistered-at-load author falls back to a numeric label"
    );
    assert!(!cx.has_text("gary"));

    known.set(true);
    live.push(Some(1));
    cx.run_until_parked();

    assert!(
        cx.has_text("gary"),
        "the roster re-reads on identity's live stream, same as \"me\""
    );
    assert!(!cx.has_text("account 9"));
    let _ = view;
}

/// The same roster the last test names also gates the `@` mention menu's
/// candidates (`client::mention_choices` builds them from `self.names`):
/// a peer who registers their account after this view's roster was first
/// read is un-mentionable until identity's live stream re-reads it — even
/// in a channel neither side has posted to yet (regression: typing
/// `@qa-mention-b-...` right after account B onboarded never offered it).
#[test]
fn a_peers_mention_becomes_offerable_once_their_account_is_known() {
    let known = std::rc::Rc::new(std::cell::Cell::new(false));
    let has_gary = known.clone();
    let mut cx = TestAppContext::new();
    quiet_doors(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::Widget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<ViewOf<ChatApi>>(move |query| {
        Ok(match query {
            ChatViewQuery::Accounts { .. } => {
                let mut accounts = vec![chat::AccountRow {
                    number: 7,
                    name: "eddy".into(),
                    program: false,
                    keys: vec!["0102".into()],
                }];
                if has_gary.get() {
                    accounts.push(chat::AccountRow {
                        number: 9,
                        name: "gary".into(),
                        program: false,
                        keys: Vec::new(),
                    });
                }
                ChatViewReply::Accounts(accounts)
            }
            ChatViewQuery::Channels { .. } => {
                ChatViewReply::Channels(page(vec![channel("general", "General", 0)]))
            }
            ChatViewQuery::Roots { .. } => ChatViewReply::Roots(page(Vec::new())),
            ChatViewQuery::Members { .. } => ChatViewReply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));
    cx.host()
        .handle::<ViewOf<identity::view::Identity>>(|query| {
            Ok(match query {
                identity::Query::OfKey { key } if key == [0x01, 0x02] => {
                    identity::Reply::Number(Some(7))
                }
                identity::Query::OfKey { .. } => identity::Reply::Number(None),
                query => panic!("unexpected identity query: {query:?}"),
            })
        });

    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    let live = cx.host().stream::<LiveChanges>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();

    view.read(|chat| {
        assert!(
            !chat
                .mention_choices()
                .iter()
                .any(|choice| choice.label == "gary"),
            "not known to the roster yet, so not offerable"
        );
    });

    known.set(true);
    live.push(Some(1));
    cx.run_until_parked();

    view.read(|chat| {
        assert!(
            chat.mention_choices()
                .iter()
                .any(|choice| choice.label == "gary"),
            "the roster re-read makes the peer mentionable, same as it names their messages"
        );
    });
}

mod message;
mod rich;

/// `CHAT_SCREEN_EXPORT=1` writes the opened room's tree for the app's
/// node-less renderer (`ducktape-app --render-tree`), light and dark.
#[test]
fn export_chat_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let (cx, _view) = opened();
    std::fs::write(
        out.join("room-light.json"),
        serde_json::to_vec(cx.root()).unwrap(),
    )
    .unwrap();
}

/// A direct message landing in a room the reader is not in is handed to the
/// host as a notice linking to it, and counted on the tab until she opens
/// the room.
#[test]
fn a_direct_message_elsewhere_is_a_notice_and_a_badge_until_read() {
    use ducktape_view_guest::doors::{Badge, NotifyPost};
    let (mut cx, view) = opened();
    cx.host().handle::<ViewOf<ChatApi>>(|query| {
        Ok(match query {
            ChatViewQuery::MessagesAround { channel_id, .. } => {
                assert_eq!(channel_id, "dm-7-8");
                let mut ping = row(2, "acct:8", "ping");
                ping.channel_id = channel_id;
                ChatViewReply::Messages(vec![row(1, "acct:7", "old"), ping])
            }
            ChatViewQuery::Roots { .. } => ChatViewReply::Roots(page(Vec::new())),
            ChatViewQuery::Members { .. } => ChatViewReply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    view.update(&mut cx, |chat, _, cx| {
        cx.notify();
        chat.channels_landed(
            vec![channel("general", "General", 3), channel("dm-7-8", "dm", 2)],
            cx,
        );
    });
    cx.run_until_parked();
    let posts = cx.host().asked::<NotifyPost>();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        (posts[0].title.as_str(), posts[0].body.as_str()),
        ("reviewer", "ping")
    );
    assert_eq!(posts[0].link, "duck://testnet-0a1b2c3d/chat/dm-7-8/2");
    assert_eq!(cx.host().asked::<Badge>().last(), Some(&1));
    view.update(&mut cx, |chat, window, cx| {
        cx.notify();
        chat.choose("dm-7-8".into(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<Badge>().last(), Some(&0));
}

/// A room of 256 rows, each with markup, a reaction and a thread, renders
/// inside the host's frame budget and under a byte ceiling: the native proxy
/// for a render's fuel.
#[test]
fn a_room_of_256_rows_renders_inside_the_frame_budget() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let rows: Vec<MsgRow> = (1..=256)
        .map(|seq| {
            let text = format!("row {seq}: **bold**, `code` and a [link](https://x.example/{seq})");
            let mut row = row(seq, if seq % 2 == 0 { "acct:7" } else { "acct:8" }, &text);
            row.blocks = chat::parse_message(&text);
            row.reply_count = seq % 3;
            row.reactions = vec![::chat::ReactionSummary {
                emoji: "👍".into(),
                count: seq,
                reacted_by_me: seq % 2 == 0,
            }];
            row
        })
        .collect();
    cx.host().handle::<ViewOf<ChatApi>>(move |query| {
        Ok(match query {
            ChatViewQuery::Accounts { .. } => ChatViewReply::Accounts(Vec::new()),
            ChatViewQuery::Channels { .. } => {
                ChatViewReply::Channels(page(vec![channel("general", "General", 256)]))
            }
            ChatViewQuery::Roots { .. } => ChatViewReply::Roots(page(rows.clone())),
            ChatViewQuery::Members { .. } => ChatViewReply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<Props>();
    let visible = cx.host().stream::<Visible>();
    cx.open::<Chat>();
    props.push(Session {
        account: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        cx.texts().iter().any(|text| text.contains("row 256")),
        "{:?}",
        cx.texts()
    );
    let bytes = cx.frame_bytes();
    // about 1.5x what it drew when this was written: tighten when the room
    // slims, raise only on purpose
    assert!(bytes < 220_000, "a 256-row room drew {bytes} bytes");
}
