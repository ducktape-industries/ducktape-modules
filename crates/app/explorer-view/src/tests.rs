use super::*;
use abi::BlobId;
use ducktape_view_guest::testing::TestAppContext;

#[test]
fn preferred_window_keeps_the_original_baseline() {
    assert_eq!(<Explorer as View>::PREFERRED_WINDOW_SIZE, "760x640");
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let mut cx = ready();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(ducktape_view_guest::wire::Node::Container { style, .. }) = cx.find("explorer") else {
        panic!("explorer root is a styled container");
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

fn entry(program: &str, code: u8) -> registry::Entry {
    registry::Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: vec![1, 2, 3],
    }
}

fn programs() -> registry::Reply {
    registry::Reply::Programs(vec![entry("identity", 0xab), entry("valset", 0xcd)])
}

fn scheduled() -> registry::Reply {
    registry::Reply::Scheduled(vec![
        registry::Scheduled {
            height: 120,
            change: registry::Change::Set(entry("chat", 0xef)),
        },
        registry::Scheduled {
            height: 200,
            change: registry::Change::Remove("forge".into()),
        },
    ])
}

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<QueryBytes<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => programs(),
            registry::Query::Scheduled => scheduled(),
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert_eq!(
        cx.host().asked::<QueryBytes<Registry>>(),
        vec![registry::Query::At(0), registry::Query::Scheduled]
    );
    assert_eq!(
        cx.host().asked::<Live>(),
        vec![registry::PROGRAM.to_string()]
    );
    cx
}

#[test]
fn the_registry_lists_what_runs_and_what_is_scheduled() {
    let cx = ready();
    let texts = cx.texts();
    assert!(cx.has_text("2 programs"), "{texts:?}");
    assert!(cx.has_text("identity") && cx.has_text("valset"));
    assert!(cx.has_text("Running") && cx.has_text("Scheduled"));
    // a scheduled change says what it does, to what, and when
    assert!(cx.has_text("Set") && cx.has_text("chat") && cx.has_text("at 120"));
    assert!(cx.has_text("Remove") && cx.has_text("forge") && cx.has_text("at 200"));
    // the code blob reaches the screen shortened, never as 64 hex chars
    assert!(
        texts.iter().any(|text| text == "abababababab…"),
        "{texts:?}"
    );
    assert!(cx.has_text("3 param bytes"));
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host().never::<QueryBytes<Registry>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the registry…"));
}

#[test]
fn an_empty_set_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host().handle::<QueryBytes<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => registry::Reply::Programs(vec![]),
            registry::Query::Scheduled => registry::Reply::Scheduled(vec![]),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("No programs"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
    cx.host()
        .refuse::<QueryBytes<Registry>>("unavailable", "the registry is not running here");
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("the registry is not running here"));
    respond(&mut cx);
    cx.simulate_click("explorer-retry");
    cx.run_until_parked();
    assert!(cx.has_text("identity"));
    assert_eq!(cx.host().asked::<QueryBytes<Registry>>().len(), 3);
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let mut cx = TestAppContext::new();
    let feed = cx.host().stream::<Live>();
    respond(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.host()
        .refuse::<QueryBytes<Registry>>("unavailable", "refresh temporarily unavailable");
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("identity"));
    assert_eq!(cx.host().asked::<QueryBytes<Registry>>().len(), 3);
    cx.host().handle::<QueryBytes<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => registry::Reply::Programs(vec![entry("forge", 0x11)]),
            registry::Query::Scheduled => registry::Reply::Scheduled(vec![]),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    feed.push(());
    cx.run_until_parked();
    assert!(cx.has_text("forge") && !cx.has_text("identity"));
    assert!(cx.has_text("Nothing is scheduled against the registry."));

    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<Live>();
    restored.host().never::<QueryBytes<Registry>>();
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("forge"));
    assert_eq!(restored.host().asked::<QueryBytes<Registry>>().len(), 1);
    assert_eq!(
        restored.host().asked::<Live>(),
        vec![registry::PROGRAM.to_string()]
    );
}

#[test]
fn the_ready_set_is_accessible() {
    ready().assert_accessible();
}
