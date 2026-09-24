use super::*;
use abi::Scheme;
use ducktape_view_guest::testing::TestAppContext;

#[test]
fn preferred_window_keeps_the_original_baseline() {
    assert_eq!(<Members as View>::PREFERRED_WINDOW_SIZE, "720,640");
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let mut cx = ready();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { style, .. },
    )) = cx.find("members")
    else {
        panic!("members root is a styled container");
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
}

fn person(number: u64, name: &str, key: &[u8]) -> identity::Account {
    identity::Account {
        number,
        name: name.into(),
        control: identity::Control::Keys(vec![identity::Key {
            scheme: Scheme::Ed25519,
            key: key.to_vec(),
            label: None,
            added_at: 0,
        }]),
        avatar: None,
        bio: None,
        updated_at: 0,
    }
}

fn program(number: u64, name: &str) -> identity::Account {
    identity::Account {
        number,
        name: name.into(),
        control: identity::Control::Program {
            executor: "chat".into(),
            controller: 1,
            standing: identity::Standing::Active,
        },
        avatar: None,
        bio: None,
        updated_at: 0,
    }
}

fn membership(key: &[u8], standing: valset::Standing) -> valset::Membership {
    valset::Membership {
        key: key.to_vec(),
        address: "10.0.0.1:4000".into(),
        standing,
    }
}

fn page<T>(items: Vec<T>) -> module_registry::PageReply<T> {
    module_registry::PageReply {
        height: 1,
        items,
        next: None,
    }
}

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Identity>>(|query| {
        assert!(matches!(query, identity::Query::List { .. }));
        Ok(identity::Reply::Accounts(page(vec![
            person(7, "eddy", b"\x01\x02"),
            program(8, "chat"),
        ])))
    });
    cx.host().handle::<Query<Valset>>(|query| {
        assert!(matches!(query, valset::Query::Memberships { .. }));
        Ok(valset::Reply::Memberships(page(vec![membership(
            b"\x01\x02",
            valset::Standing::Validator,
        )])))
    });
}

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Members>();
    cx.run_until_parked();
    cx
}

#[test]
fn the_roster_lists_each_account_with_its_standing() {
    let cx = ready();
    for text in [
        "eddy",
        "chat",
        "#7",
        "#8",
        "Person",
        "Program",
        "Validator",
        "2 accounts",
    ] {
        assert!(cx.has_text(text), "{:?}", cx.texts());
    }
    assert_eq!(
        cx.texts()
            .iter()
            .filter(|text| *text == "Validator")
            .count(),
        1
    );
    assert_eq!(
        cx.host().asked::<Live>(),
        vec![identity::PROGRAM.to_string()]
    );
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host().never::<Query<Identity>>();
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the roster…"));
}

#[test]
fn a_roster_with_nobody_in_it_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .handle::<Query<Identity>>(|_| Ok(identity::Reply::Accounts(page(vec![]))));
    cx.host()
        .handle::<Query<Valset>>(|_| Ok(valset::Reply::Memberships(page(vec![]))));
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("No accounts"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().asked::<Query<Identity>>().len(), 2);
}

#[test]
fn the_filter_narrows_the_list_without_asking_again() {
    let mut cx = ready();
    let reads = cx.host().asked::<Query<Identity>>().len();
    cx.simulate_input("members-filter", "ed");
    assert!(cx.has_text("eddy") && !cx.has_text("chat"));
    assert_eq!(cx.host().asked::<Query<Identity>>().len(), reads);
    cx.simulate_input("members-filter", "8");
    assert!(cx.has_text("chat") && !cx.has_text("eddy"));
    cx.simulate_input("members-filter", "nobody");
    assert!(cx.has_text("Nothing matches"));
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let mut cx = TestAppContext::new();
    let feed = cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Members>();
    cx.run_until_parked();
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "refresh temporarily unavailable");
    feed.push(None);
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().asked::<Query<Identity>>().len(), 2);
    cx.host().handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(page(vec![person(
            9, "newcomer", b"\x09",
        )])))
    });
    feed.push(None);
    cx.run_until_parked();
    assert!(cx.has_text("newcomer") && !cx.has_text("eddy"));
    cx.simulate_input("members-filter", "new");
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<Live>();
    restored.host().never::<Query<Identity>>();
    let view = restored.restore::<Members>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("newcomer"));
    view.read(|view| assert_eq!(view.filter, "new"));
    assert_eq!(restored.host().asked::<Query<Identity>>().len(), 1);
    assert_eq!(
        restored.host().asked::<Live>(),
        vec![identity::PROGRAM.to_string()]
    );
}

#[test]
fn the_ready_roster_is_accessible() {
    ready().assert_accessible();
}
