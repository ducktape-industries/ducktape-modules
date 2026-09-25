//! Who reads: the seated key, the account it gains, the names it learns.
use super::*;

#[test]
fn session_key_resolves_to_its_account() {
    let (_cx, view) = opened();
    view.read(|chat| {
        assert_eq!(chat.my_account(), Some(7));
        assert!(chat.holds_account());
        assert_eq!(chat.me(), Some(Principal::Account(7)));
    });
}

#[test]
fn an_unregistered_key_stays_read_only() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        key: "ffff".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(chat.my_account(), None);
        assert!(!chat.holds_account());
        assert_eq!(chat.write_gate(), Some(crate::session::Gate::NoAccount));
    });
    cx.simulate_click("chat-sidebar-new-channel");
    cx.run_until_parked();
    assert!(cx.has_text("Create an account to create a channel"));
}

/// The reader creates the account in Settings, then switches to Chat: the
/// seated key never changes; the host resolves its new account and hands it
/// over as a session change.
#[test]
fn an_account_gained_later_re_enables_create_channel() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    let unregistered = Session {
        key: "0102".into(),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    };
    props.push(unregistered.clone());
    visible.push(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-new-channel");
    cx.run_until_parked();
    assert!(cx.has_text("Create an account to create a channel"));

    props.push(Session {
        account: Some(7),
        ..unregistered
    });
    cx.run_until_parked();

    assert!(!cx.has_text("Create an account to create a channel"));
    view.read(|chat| assert_eq!(chat.my_account(), Some(7)));
}

/// A peer who registers their account AFTER this room's roster was first
/// read still shows up under "account N" unless the roster naming everyone
/// is re-read on identity's live stream: so a fresh signer's messages stayed numbered forever
/// (regression: a two-account chat never named the other side's reply).
#[test]
fn a_peers_name_gained_later_replaces_its_numeric_fallback() {
    let known = std::rc::Rc::new(std::cell::Cell::new(false));
    let has_gary = known.clone();
    let mut cx = TestAppContext::new();
    quiet_methods(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::methods::HostWidget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<Ask<ChatApi>>(move |query| {
        Ok(match query {
            Query::Accounts { .. } => {
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
                Reply::Accounts(page(accounts))
            }
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 2)])),
            Query::Roots { .. } => {
                Reply::Roots(page(vec![row(1, 7, "hello"), row(2, 9, "hi from gary")]))
            }
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));

    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    cx.host().never::<Changes<ChatApi>>();
    let live = cx.host().stream::<Changes<Identity>>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
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
        cx.has_text("account 9"),
        "an unregistered-at-load author falls back to a numeric label"
    );
    assert!(!cx.has_text("gary"));

    known.set(true);
    live.push(Some(1));
    cx.run_until_parked();

    assert!(
        cx.has_text("gary"),
        "the roster re-reads on identity's live stream"
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
    quiet_methods(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::methods::HostWidget>(|command| {
            assert!(matches!(command, wire::WidgetCommand::Focus { .. }));
            Ok(())
        });
    cx.host().handle::<Ask<ChatApi>>(move |query| {
        Ok(match query {
            Query::Accounts { .. } => {
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
                Reply::Accounts(page(accounts))
            }
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 0)])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));

    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    cx.host().never::<Changes<ChatApi>>();
    let live = cx.host().stream::<Changes<Identity>>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
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

/// A roster longer than one page (identity pages at 256) is read to its
/// end: the view follows `next` rather than naming only the first page.
#[test]
fn the_roster_is_read_past_its_first_page() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { page } => {
                let start = page.after.map_or(1, |after| after[0] as u64 * 256 + 1);
                let end = (start + 255).min(600);
                let next = (end < 600).then(|| vec![(end / 256) as u8]);
                Reply::Accounts(::chat::PageReply {
                    height: 1,
                    items: (start..=end)
                        .map(|number| chat::AccountRow {
                            number,
                            name: format!("user{number}"),
                            program: false,
                            keys: Vec::new(),
                        })
                        .collect(),
                    next,
                })
            }
            Query::Channels { .. } => Reply::Channels(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    view.read(|chat| {
        let names = chat.names.ready().expect("the roster landed");
        assert_eq!(names.numbers().count(), 600);
        assert_eq!(names.name(&Principal::Account(600)), Some("user600"));
        assert!(!names.more(), "the whole roster was read");
    });
    assert!(cx.find("chat-sidebar-more").is_none());
}

/// A channel list that never ends is read to its page budget, and the
/// sidebar says it goes on rather than passing for the whole list.
#[test]
fn a_channel_list_cut_at_its_budget_says_so() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<ChatApi>>(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { page } => {
                let n = page.after.map_or(0, |after| after[0]);
                Reply::Channels(::chat::PageReply {
                    height: 1,
                    items: vec![channel(&format!("room{n}"), "Room", 1)],
                    next: Some(vec![n + 1]),
                })
            }
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    let props = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    props.push(Session {
        key: "0102".into(),
        account: Some(7),
        connected: true,
        chain: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    visible.push(true);
    cx.run_until_parked();
    view.read(|chat| assert!(chat.channels_more));
    assert!(cx.find("chat-sidebar-more").is_some());
}
