use super::*;
use ducktape_view_guest::testing::{answer, has_text, item, press, refuse, type_into};
use ducktape_view_guest::view::Shell;
use ducktape_view_guest::{Driver, wire};

fn request<'a>(frame: &'a wire::Frame, kind: &str) -> impl Iterator<Item = &'a wire::Request> {
    frame
        .requests
        .iter()
        .filter(move |request| request.kind == kind)
}

fn view_request(frame: &wire::Frame, word: &str) -> u64 {
    request(frame, "rpc.view")
        .find(|r| std::str::from_utf8(&r.payload).unwrap().contains(word))
        .unwrap_or_else(|| panic!("no rpc.view asking {word}"))
        .id
}

fn roster_page() -> Vec<u8> {
    json!({"accounts": [
        {"number": 7, "name": "eddy", "control": "keys", "keys": [{"pubkey": [1, 2]}]},
        {"number": 8, "name": "reviewer", "control": {"program": "x"}, "keys": []}
    ]})
    .to_string()
    .into_bytes()
}

fn channel(id: &str, name: &str, head: u64) -> Value {
    json!({"id": id, "name": name, "created_at": 0, "post_policy": "open", "owner": "acct:7", "archived": false, "hooks": [], "huddle": [], "voice": false, "head_seq": head})
}

fn row(seq: u64, author: &str, text: &str) -> Value {
    json!({"channel_id": "general", "seq": seq, "message_id": format!("m{seq}"), "author": author, "height": 1, "time": 0, "blocks": [{"paragraph": [{"text": text, "marks": []}]}], "text": text, "deleted": false, "edited": false, "rev": 0, "edited_at": null, "base_rev": null, "thread": null, "reply_count": 0, "last_reply_seq": null, "reactions": [], "tags": []})
}

/// Boots, seats a reader, lists the rooms and opens `general` with one row.
fn opened() -> (Driver<Shell<Chat>>, wire::Frame) {
    let mut driver = Driver::<Shell<Chat>>::new();
    let frame = driver.tick(vec![]);
    let kinds: Vec<_> = frame.requests.iter().map(|r| r.kind.as_str()).collect();
    for kind in ["chat.props", "rpc.live", "host.visible", "clock.ticks"] {
        assert!(kinds.contains(&kind), "{kinds:?}");
    }
    assert!(has_text(&frame, "Not connected"));
    let props = request(&frame, "chat.props").next().unwrap().id;
    let visible = request(&frame, "host.visible").next().unwrap().id;
    let rooms = view_request(&frame, "\"channels\"");
    let session = json!({"me": "acct:7", "me_key": "0102", "connected": true, "network_name": "duck", "chain": "testnet#0a1b2c3d"});
    let frame = driver.tick(vec![
        item(props, session.to_string().as_bytes()),
        item(visible, b"true"),
    ]);
    let names = request(&frame, "rpc.query").next().unwrap().id;
    let channels = json!({"channels": {"channels": [channel("general", "General", 3), channel(&format!("dm-{}", "a".repeat(64)), "dm", 1)], "has_more": false, "next_after": null}});
    // the connection coming up and the tab showing both re-read the rooms
    let mut answers = vec![answer(names, &roster_page())];
    for id in std::iter::once(rooms).chain(request(&frame, "rpc.view").map(|r| r.id)) {
        answers.push(answer(id, channels.to_string().as_bytes()));
    }
    let frame = driver.tick(answers);
    assert!(has_text(&frame, "General"));
    assert!(has_text(&frame, "No channel open"));
    assert!(has_text(&frame, "Channels"));

    let frame = driver.tick(press(&frame, "chat/sidebar/channel/general"));
    assert_eq!(driver.app.state().room.as_ref().unwrap().id, "general");
    let (roots, seats) = (
        view_request(&frame, "\"roots\""),
        view_request(&frame, "\"members\""),
    );
    let page = json!({"roots": {"roots": [row(1, "acct:7", "hello"), row(2, "acct:8", "**hi** there")], "has_more": false, "next_before_seq": null}});
    let frame = driver.tick(vec![
        answer(roots, page.to_string().as_bytes()),
        answer(
            seats,
            br#"{"members":{"members":[],"has_more":false,"next_after":null}}"#,
        ),
    ]);
    (driver, frame)
}

#[test]
fn the_room_shows_its_rows_intro_and_actions() {
    let (mut driver, frame) = opened();
    let texts = ducktape_view_guest::testing::texts(&frame);
    assert!(has_text(&frame, "hello"), "{texts:?}");
    assert!(has_text(&frame, "eddy") && has_text(&frame, "reviewer"));
    assert!(
        has_text(&frame, "Agent"),
        "a program account wears the badge"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("This is the very beginning of #General")),
        "{texts:?}"
    );
    // the rich body kept its spans
    let bold = ducktape_view_guest::testing::keys(&frame)
        .into_iter()
        .filter(|k| k.contains("/block/0/text"))
        .count();
    assert!(bold >= 2, "each paragraph is a rich text");
    // the floating actions open the reaction picker; a press picks 👍
    let frame = driver.tick(press(&frame, "Manage reactions"));
    assert!(
        driver
            .app
            .state()
            .menu
            .as_ref()
            .is_some_and(|m| m.mode == Mode::Reactions)
    );
    let frame = driver.tick(press(&frame, "chat/room/message-reaction/🔥"));
    let submit = request(&frame, "op.submit").next().expect("a reaction op");
    assert!(
        std::str::from_utf8(&submit.payload)
            .unwrap()
            .contains("add_reaction")
    );
    assert!(driver.app.state().menu.is_none());
    // the "…" menu offers the thread, and Reply opens it
    let frame = driver.tick(press(&frame, "More message actions"));
    assert!(has_text(&frame, "Reply in thread") && has_text(&frame, "Copy link"));
    let frame = driver.tick(press(&frame, "chat/room/message-reply"));
    assert_eq!(
        driver
            .app
            .state()
            .room
            .as_ref()
            .unwrap()
            .thread
            .as_ref()
            .map(|t| t.root),
        Some(1)
    );
    let thread = view_request(&frame, "\"thread\"");
    let frame = driver.tick(vec![answer(
        thread,
        json!({"thread": {"replies": [], "has_more": false, "next_reply_seq": null}})
            .to_string()
            .as_bytes(),
    )]);
    assert!(has_text(&frame, "Thread") && has_text(&frame, "Reply in thread"));
    let frame = driver.tick(press(&frame, "Close thread"));
    assert!(driver.app.state().room.as_ref().unwrap().thread.is_none());
    // details: rename goes out as an op
    let frame = driver.tick(press(&frame, "Channel details"));
    assert!(has_text(&frame, "Archive channel"));
    let frame = driver.tick(type_into(&frame, "chat/details/name-input", "Lobby"));
    let frame = driver.tick(press(&frame, "chat/details/rename-button"));
    let submit = request(&frame, "op.submit").next().expect("a rename op");
    assert!(
        std::str::from_utf8(&submit.payload)
            .unwrap()
            .contains("rename_channel")
    );
    let _ = frame;
}

#[test]
fn a_send_shows_pending_then_lands_and_a_refusal_is_a_banner() {
    let (mut driver, frame) = opened();
    let pending = MsgRow {
        message_id: "p1".into(),
        author: "acct:7".into(),
        blocks: vec![chat::Block::paragraph("on its way")],
        ..MsgRow::default()
    };
    driver
        .app
        .state_mut()
        .room
        .as_mut()
        .unwrap()
        .pending
        .push(pending.clone());
    // state set from outside renders on the next event
    let frame = driver.tick(type_into(&frame, "chat/sidebar/search", "x"));
    assert!(has_text(&frame, "on its way") && has_text(&frame, "sending…"));
    {
        let mut chat = driver.app.state_mut();
        let room = chat.room.as_mut().unwrap();
        room.messages
            .ready_mut()
            .unwrap()
            .push(MsgRow { seq: 3, ..pending });
        room.settle();
    }
    let frame = driver.tick(type_into(&frame, "chat/sidebar/search", "xy"));
    assert!(has_text(&frame, "on its way") && !has_text(&frame, "sending…"));

    // a search stands in for the stream until cleared
    let mut chat = driver.app.state_mut();
    chat.search.draft = "hello".into();
    drop(chat);
    let frame = driver.tick(ducktape_view_guest::testing::submit(
        &frame,
        "chat/sidebar/search",
    ));
    let hits = view_request(&frame, "\"search\"");
    let frame = driver.tick(vec![answer(
        hits,
        json!({"hits": {"hits": [row(1, "acct:7", "hello")], "capped": false}})
            .to_string()
            .as_bytes(),
    )]);
    assert!(
        has_text(&frame, "1 result for “hello”"),
        "{:?}",
        ducktape_view_guest::testing::texts(&frame)
    );
    let frame = driver.tick(press(&frame, "Clear message search"));
    assert!(driver.app.state().search.query.is_empty());

    // creating a channel asks for an id, then submits the op; a refusal shows
    let frame = driver.tick(press(&frame, "New channel"));
    assert!(has_text(&frame, "Create a channel"));
    let frame = driver.tick(type_into(&frame, "chat/create/name", "random"));
    let frame = driver.tick(press(&frame, "chat/create/members"));
    let frame = driver.tick(press(&frame, "chat/create/submit"));
    let id = request(&frame, "host.id").next().unwrap().id;
    let frame = driver.tick(vec![answer(id, b"chan-1")]);
    let submit = request(&frame, "op.submit").next().unwrap();
    let payload = std::str::from_utf8(&submit.payload).unwrap();
    assert!(payload.contains("create_channel") && payload.contains("members_only"));
    let frame = driver.tick(vec![refuse(submit.id, "no")]);
    assert!(has_text(&frame, "Couldn’t create this channel: no"));

    // a snapshot restores with the room and reopens it
    let bytes = driver.snapshot().unwrap();
    let mut restored = Driver::<Shell<Chat>>::from_snapshot(&bytes, false).unwrap();
    let frame = restored.tick(vec![]);
    assert_eq!(restored.app.state().room.as_ref().unwrap().id, "general");
    assert!(request(&frame, "rpc.view").count() >= 2);
}

#[test]
fn unread_rooms_carry_a_dot_and_the_open_room_a_divider() {
    let (mut driver, frame) = opened();
    // the room's head moves while another room is open: it is unread
    driver
        .app
        .state_mut()
        .open("other".into(), &mut Cx::default());
    let mut chat = driver.app.state_mut();
    chat.channels_arrived(vec![
        serde_json::from_value(channel("general", "General", 9)).unwrap(),
        serde_json::from_value(channel("other", "Other", 0)).unwrap(),
    ]);
    drop(chat);
    let frame = driver.tick(type_into(&frame, "chat/sidebar/search", "z"));
    assert!(
        ducktape_view_guest::testing::keys(&frame)
            .iter()
            .any(|k| k.ends_with("/channel/general/unread"))
    );
    // entering it with unread rows draws the divider at the first new one
    let mut chat = driver.app.state_mut();
    chat.reads.entering = true;
    chat.room = Some(Room {
        id: "general".into(),
        messages: Loaded::Ready(vec![
            serde_json::from_value(row(3, "acct:7", "old")).unwrap(),
            serde_json::from_value(row(9, "acct:8", "new")).unwrap(),
        ]),
        at_tail: true,
        reaches_head: true,
        ..Room::default()
    });
    chat.channels_arrived(vec![
        serde_json::from_value(channel("general", "General", 9)).unwrap(),
    ]);
    assert_eq!(chat.reads.boundary, 3);
    drop(chat);
    let frame = driver.tick(type_into(&frame, "chat/sidebar/search", "zz"));
    assert!(has_text(&frame, "New messages"));
}
