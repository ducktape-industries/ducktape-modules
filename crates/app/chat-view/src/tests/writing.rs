//! Sending, and creating channels.
use super::*;

#[test]
fn a_send_shows_pending_then_lands_and_a_refusal_is_a_banner() {
    let (mut cx, view) = opened();
    let pending = MsgRow {
        message_id: "p1".into(),
        blocks: vec![chat::Block::paragraph("on its way")],
        ..MsgRow::by(Principal::Account(7))
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
    assert!(cx.has_text("2 results for “hello”"), "{:?}", cx.texts());
    cx.simulate_click("chat-sidebar-clear-search");
    view.read(|chat| assert!(chat.search.query.is_empty()));
    cx.host().handle::<HostId>(|kind| {
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
    assert!(cx.host().requests::<Submit<ChatApi>>().iter().any(|op| matches!(op, Op::CreateChannel { name, post_policy: PostPolicy::MembersOnly, .. } if name == "random")));
    assert!(cx.has_text("Couldn’t create this channel: no"));
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    configure(&mut restored);
    restored.host().never::<HostSession>();
    restored.host().never::<HostVisible>();
    let view = restored.restore::<Chat>(&bytes).unwrap();
    restored.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    assert!(restored.host().requests::<Ask<ChatApi>>().len() >= 2);
}

#[test]
fn channel_create_preserves_busy_and_account_gates() {
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
        "chat-create-members",
        "chat-create-cancel",
        "chat-create-submit",
    ] {
        assert!(disabled(&cx, id), "{id} must stay inert while busy");
    }

    view.update(&mut cx, |chat, _, cx| {
        let create = chat.create.as_mut().unwrap();
        create.busy = false;
        chat.session.account = None;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
    assert!(cx.has_text("Create an account to create a channel"));
    assert!(!disabled(&cx, "chat-create-cancel"));
    let submitted = cx.host().requests::<Submit<ChatApi>>().len();
    view.update(&mut cx, |chat, _, cx| chat.create_channel(cx));
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<ChatApi>>().len(), submitted);

    view.update(&mut cx, |chat, _, cx| {
        chat.session.account = Some(7);
        chat.session.connected = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
}

/// The composer follows the program's rule for a members-only room: its
/// owner writes without a seat, a seated member writes, a stranger reads.
#[test]
fn a_members_only_room_takes_its_owner_and_its_members() {
    let (mut cx, view) = opened();
    let gate = |cx: &mut TestAppContext, owner: u64, seated: bool| {
        view.update(cx, |chat, _, cx| {
            let general = chat
                .channels
                .ready_mut()
                .unwrap()
                .iter_mut()
                .find(|info| info.channel.id == "general")
                .unwrap();
            general.channel.post_policy = PostPolicy::MembersOnly;
            general.channel.owner = Principal::Account(owner);
            let seats = if seated {
                vec![chat::MemberRow {
                    principal: Principal::Account(7),
                    height: 1,
                    time: 1,
                }]
            } else {
                Vec::new()
            };
            chat.room.as_mut().unwrap().members = Loadable::Ready(seats);
            cx.notify();
        });
        view.read(|chat| chat.write_gate())
    };
    assert_eq!(gate(&mut cx, 7, false), None, "the owner needs no seat");
    assert_eq!(gate(&mut cx, 8, true), None, "a seated member writes");
    assert_eq!(
        gate(&mut cx, 8, false),
        Some(crate::session::Gate::NotMember),
        "a stranger reads"
    );
}

/// Hits in two channels at the same seq are two rows the host can tell
/// apart; one identity for both stopped the view.
#[test]
fn search_hits_in_two_channels_at_one_seq_are_two_rows() {
    let (mut cx, _) = opened();
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert!(cx.find("chat-search-hit-general-1").is_some());
    assert!(cx.find("chat-search-hit-dm-7-8-1").is_some());
    // the host's sanitizer refuses duplicate typed identities among siblings
    cx.frame_bytes();
}
