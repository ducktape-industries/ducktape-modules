use super::*;
use ducktape_view_guest::testing::TestAppContext;

fn membership(key: &[u8], address: &str, standing: valset::Standing) -> valset::Membership {
    valset::Membership {
        key: key.to_vec(),
        address: address.into(),
        standing,
    }
}

fn validators() -> valset::Reply {
    valset::Reply::Validators(vec![vec![0xab, 0xcd]])
}

fn memberships() -> valset::Reply {
    valset::Reply::Memberships(vec![
        membership(b"\xab\xcd", "10.0.0.1:4000", valset::Standing::Validator),
        membership(b"\x01\x02", "10.0.0.2:4000", valset::Standing::Resident),
    ])
}

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<QueryBytes<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => validators(),
            valset::Query::Memberships => memberships(),
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert_eq!(
        cx.host().asked::<QueryBytes<Valset>>(),
        vec![valset::Query::Validators, valset::Query::Memberships]
    );
    assert_eq!(cx.host().asked::<Live>(), vec![valset::PROGRAM.to_string()]);
    cx
}

#[test]
fn the_set_shows_its_validators_memberships_and_counts() {
    let cx = ready();
    let texts = cx.texts();
    assert!(cx.has_text("1 validator · 2 members"), "{texts:?}");
    assert!(cx.has_text("Validator set") && cx.has_text("Memberships"));
    assert!(cx.has_text("10.0.0.1:4000") && cx.has_text("10.0.0.2:4000"));
    assert!(cx.has_text("Validator") && cx.has_text("Resident"));
    // a key reaches the screen shortened, never raw
    assert!(texts.iter().any(|text| text == "abcd"), "{texts:?}");
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host().never::<QueryBytes<Valset>>();
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the validator set…"));
}

#[test]
fn an_empty_set_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host().handle::<QueryBytes<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => valset::Reply::Validators(vec![]),
            valset::Query::Memberships => valset::Reply::Memberships(vec![]),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("No members"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .refuse::<QueryBytes<Valset>>("unavailable", "valset is not running here");
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("valset is not running here"));
    respond(&mut cx);
    cx.simulate_click("nodes-retry");
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"));
    assert_eq!(cx.host().asked::<QueryBytes<Valset>>().len(), 3);
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let mut cx = TestAppContext::new();
    let feed = cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    cx.host()
        .refuse::<QueryBytes<Valset>>("unavailable", "refresh temporarily unavailable");
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"));
    assert_eq!(cx.host().asked::<QueryBytes<Valset>>().len(), 3);
    cx.host().handle::<QueryBytes<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => validators(),
            valset::Query::Memberships => valset::Reply::Memberships(vec![membership(
                b"\xab\xcd",
                "10.9.9.9:4000",
                valset::Standing::Validator,
            )]),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("10.9.9.9:4000") && !cx.has_text("10.0.0.1:4000"));

    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<Live>();
    restored.host().never::<QueryBytes<Valset>>();
    restored.restore::<Nodes>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("10.9.9.9:4000"));
    assert_eq!(restored.host().asked::<QueryBytes<Valset>>().len(), 1);
    assert_eq!(
        restored.host().asked::<Live>(),
        vec![valset::PROGRAM.to_string()]
    );
}

#[test]
fn the_ready_set_is_accessible() {
    ready().assert_accessible();
}
