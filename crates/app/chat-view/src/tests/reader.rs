//! Who reads: the seated key, the account it gains, the names it learns.
use super::*;

#[test]
fn session_key_resolves_to_its_account() {
    let (_cx, view) = opened();
    view.read(|chat| {
        assert_eq!(chat.my_account(), Some(7));
        assert!(chat.holds_account());
        assert_eq!(chat.me(), Some(Party::Account(7)));
    });
}

#[test]
fn an_unregistered_key_stays_read_only() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<HostProps>();
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
    let props = cx.host().stream::<HostProps>();
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
    quiet_doors(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::HostWidget>(|command| {
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
                Reply::Accounts(accounts)
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

    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    cx.host().never::<Live<ChatApi>>();
    let live = cx.host().stream::<Live<Identity>>();
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
    quiet_doors(&mut cx);
    cx.host()
        .handle::<ducktape_view_guest::doors::HostWidget>(|command| {
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
                Reply::Accounts(accounts)
            }
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 0)])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    });
    cx.host().handle::<Submit<ChatApi>>(|_| Ok(Vec::new()));

    let props = cx.host().stream::<HostProps>();
    let visible = cx.host().stream::<HostVisible>();
    cx.host().never::<Live<ChatApi>>();
    let live = cx.host().stream::<Live<Identity>>();
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
