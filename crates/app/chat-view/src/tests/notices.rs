//! Notices, the tab badge, and the read cursors kept between runs.
use super::*;

/// A direct message landing in a room the reader is not in is handed to the
/// host as a notice linking to it, and counted on the tab until she opens
/// the room; opening it reads the host's rows under the notice's tag.
#[test]
fn a_direct_message_elsewhere_is_a_notice_and_a_badge_until_read() {
    use ducktape_view_guest::doors::{HostBadge, NotifyPost, NotifyRead};
    let (mut cx, view) = opened();
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::MessagesAround { channel_id, .. } => {
                assert_eq!(channel_id, "dm-7-8");
                let mut ping = row(2, 8, "ping");
                ping.channel_id = channel_id;
                Reply::Messages(vec![row(1, 7, "old"), ping])
            }
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
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
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&1));
    assert!(cx.host().asked::<NotifyRead>().is_empty());
    view.update(&mut cx, |chat, window, cx| {
        cx.notify();
        chat.choose("dm-7-8".into(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&0));
    assert_eq!(cx.host().asked::<NotifyRead>(), [posts[0].tag.clone()]);
}

/// A reload carries the read cursors but not the count: the first list after
/// it counts again what is meant for the reader in rooms still unread, and
/// posts nothing a second time.
#[test]
fn the_badge_is_counted_again_from_the_read_cursors() {
    use ducktape_view_guest::doors::{HostBadge, NotifyPost};
    let (mut cx, view) = opened();
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::MessagesAround { channel_id, .. } => {
                let mut ping = row(2, 8, "ping");
                ping.channel_id = channel_id;
                Reply::Messages(vec![row(1, 7, "old"), ping])
            }
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let rooms = || vec![channel("general", "General", 3), channel("dm-7-8", "dm", 2)];
    let posted = cx.host().asked::<NotifyPost>().len();
    view.update(&mut cx, |chat, _, cx| {
        cx.notify();
        // as a reload leaves it: the rooms and the cursors, no count
        chat.channels = Loaded::Ready(rooms());
        chat.reads.cursors.insert("general".into(), 3);
        chat.reads.cursors.insert("dm-7-8".into(), 1);
        chat.attention.clear();
        chat.badge = None;
        chat.recounted = false;
        chat.channels_landed(rooms(), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.host().asked::<NotifyPost>().len(),
        posted,
        "no second notice"
    );
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&1));
}

/// A relaunch starts with no cursors in memory: the ones kept on the device
/// come back, so a mention and a direct message still unread count on the
/// badge and dot their rooms, and reading a room keeps its new cursor.
#[test]
fn kept_cursors_bring_the_badge_back_after_a_relaunch() {
    use ducktape_view_guest::doors::{self, HostBadge, StoreGet, StoreSet};
    use std::collections::BTreeMap;
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<StoreGet>(|key| {
        Ok((key == "reads/0102").then(|| {
            doors::encode(&BTreeMap::from([
                ("general".to_owned(), 1u64),
                ("dm-7-8".to_owned(), 0),
                ("gone".to_owned(), 4),
            ]))
        }))
    });
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(Vec::new()),
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 3),
                channel("dm-7-8", "dm", 1),
            ])),
            Query::MessagesAround { channel_id, .. } => {
                let mut ping = row(2, 8, "@eddy");
                ping.blocks = vec![chat::Block::Paragraph(vec![chat::Span {
                    text: "@eddy".into(),
                    marks: vec![chat::Mark::Mention(chat::Party::Account(7))],
                }])];
                if channel_id == "dm-7-8" {
                    ping = row(1, 8, "hi");
                }
                ping.channel_id = channel_id;
                Reply::Messages(vec![ping])
            }
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    let _view = cx.open::<Chat>();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&2));
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    assert!(cx.find("chat-sidebar-dm-8-unread").is_some());

    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&1));
    let (key, kept) = cx
        .host()
        .asked::<StoreSet>()
        .pop()
        .expect("the read is kept");
    assert_eq!(key, "reads/0102");
    let kept: BTreeMap<String, u64> = doors::decode(&kept.unwrap()).unwrap();
    assert_eq!(
        kept,
        BTreeMap::from([("general".into(), 3), ("dm-7-8".into(), 0)]),
        "a room no longer listed is not kept"
    );
}

/// On a relaunch the kept cursors and the room list can land before the
/// host names the reader's account: the recount waits for her account, then
/// runs without another chat write to prompt it.
#[test]
fn the_relaunch_recount_waits_for_the_readers_account() {
    use ducktape_view_guest::doors::{self, HostBadge, StoreGet};
    use std::collections::BTreeMap;
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<StoreGet>(|key| {
        Ok((key == "reads/0102")
            .then(|| doors::encode(&BTreeMap::from([("dm-7-8".to_owned(), 0u64)]))))
    });
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(Vec::new()),
            Query::Channels { .. } => Reply::Channels(page(vec![channel("dm-7-8", "dm", 1)])),
            Query::MessagesAround { channel_id, .. } => {
                let mut hi = row(1, 8, "hi");
                hi.channel_id = channel_id;
                Reply::Messages(vec![hi])
            }
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    let _view = cx.open::<Chat>();
    let unresolved = Session {
        key: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    };
    props.push(unresolved.clone());
    visible.push(true);
    cx.run_until_parked();
    assert_ne!(cx.host().asked::<HostBadge>().last(), Some(&1));

    // the host names her account
    props.push(Session {
        account: Some(7),
        ..unresolved
    });
    cx.run_until_parked();
    assert_eq!(cx.host().asked::<HostBadge>().last(), Some(&1));
}

/// A device store that refuses the kept cursors is asked again, then chat
/// starts from what it sees: reading a room still keeps its cursor.
#[test]
fn a_refused_store_read_still_keeps_cursors() {
    use ducktape_view_guest::doors::{self, StoreGet, StoreSet};
    use ducktape_view_guest::host::malformed;
    use std::collections::BTreeMap;
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<StoreGet>(|key| match key.as_str() {
        "reads/0102" => Err(malformed("the store is unavailable".into())),
        _ => Ok(None),
    });
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(Vec::new()),
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    let _view = cx.open::<Chat>();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    let asked = cx.host().asked::<StoreGet>();
    assert_eq!(
        asked.iter().filter(|key| *key == "reads/0102").count(),
        3,
        "asked again before giving up: {asked:?}"
    );
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let (key, kept) = cx
        .host()
        .asked::<StoreSet>()
        .into_iter()
        .rfind(|(key, _)| key == "reads/0102")
        .expect("the read is kept");
    assert_eq!(key, "reads/0102");
    let kept: BTreeMap<String, u64> = doors::decode(&kept.unwrap()).unwrap();
    assert_eq!(kept, BTreeMap::from([("general".into(), 3)]));
}
