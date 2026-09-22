use super::*;
use ducktape_view_guest::Entity;
use ducktape_view_guest::testing::TestAppContext;

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
    assert!(cx.host().asked::<ducktape_view_guest::caps::Widget>().iter().any(|command| {
        matches!(command, wire::WidgetCommand::Focus { target } if target == &ui::menu::focus_key(Pane::Timeline, Mode::Reactions))
    }));
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|m| m.mode == Mode::Reactions)
        )
    });
    cx.simulate_click("chat-message-m1-reaction-🔥");
    cx.run_until_parked();
    assert!(
        cx.host()
            .asked::<Submit<ChatApi>>()
            .iter()
            .any(|op| matches!(op, ChatMsg::AddReaction { emoji, .. } if emoji == "🔥"))
    );
    view.read(|chat| assert!(chat.menu.is_none()));
    cx.simulate_click("chat-message-m1-more");
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
    cx.simulate_click("chat-create-submit");
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
