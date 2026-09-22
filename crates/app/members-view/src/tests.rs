use super::*;
use abi::Scheme;
use ducktape_view_guest::testing::TestAppContext;

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

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<QueryBytes<Identity>>(|query| {
        assert_eq!(query, identity::Query::List { page: Page::all() });
        Ok(identity::Reply::Accounts(vec![
            person(7, "eddy", b"\x01\x02"),
            program(8, "chat"),
        ]))
    });
    cx.host().handle::<QueryBytes<Valset>>(|query| {
        assert_eq!(query, valset::Query::Memberships);
        Ok(valset::Reply::Memberships(vec![membership(
            b"\x01\x02",
            valset::Standing::Validator,
        )]))
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
    cx.host().never::<QueryBytes<Identity>>();
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the roster…"));
}

#[test]
fn a_roster_with_nobody_in_it_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .handle::<QueryBytes<Identity>>(|_| Ok(identity::Reply::Accounts(vec![])));
    cx.host()
        .handle::<QueryBytes<Valset>>(|_| Ok(valset::Reply::Memberships(vec![])));
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("No accounts"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .refuse::<QueryBytes<Identity>>("unavailable", "identity is not running here");
    cx.open::<Members>();
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    respond(&mut cx);
    cx.simulate_click("members-retry");
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().asked::<QueryBytes<Identity>>().len(), 2);
}

#[test]
fn the_filter_narrows_the_list_without_asking_again() {
    let mut cx = ready();
    let reads = cx.host().asked::<QueryBytes<Identity>>().len();
    cx.simulate_input("members-filter", "ed");
    assert!(cx.has_text("eddy") && !cx.has_text("chat"));
    assert_eq!(cx.host().asked::<QueryBytes<Identity>>().len(), reads);
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
        .refuse::<QueryBytes<Identity>>("unavailable", "refresh temporarily unavailable");
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("eddy"));
    assert_eq!(cx.host().asked::<QueryBytes<Identity>>().len(), 2);
    cx.host().handle::<QueryBytes<Identity>>(|_| {
        Ok(identity::Reply::Accounts(vec![person(
            9, "newcomer", b"\x09",
        )]))
    });
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("newcomer") && !cx.has_text("eddy"));
    cx.simulate_input("members-filter", "new");
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<Live>();
    restored.host().never::<QueryBytes<Identity>>();
    let view = restored.restore::<Members>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("newcomer"));
    view.read(|view| assert_eq!(view.filter, "new"));
    assert_eq!(restored.host().asked::<QueryBytes<Identity>>().len(), 1);
    assert_eq!(
        restored.host().asked::<Live>(),
        vec![identity::PROGRAM.to_string()]
    );
}

#[test]
fn the_ready_roster_is_accessible() {
    ready().assert_accessible();
}
