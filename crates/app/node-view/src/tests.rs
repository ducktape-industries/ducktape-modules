use super::*;
use ducktape_view_guest::testing::TestAppContext;

#[test]
fn preferred_window_keeps_the_original_baseline() {
    assert_eq!(<Nodes as View>::PREFERRED_WINDOW_SIZE, "680,620");
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let mut cx = ready();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { style, .. },
    )) = cx.find("nodes")
    else {
        panic!("nodes root is a styled container");
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

fn page<T>(items: Vec<T>) -> module_registry::PageReply<T> {
    module_registry::PageReply {
        height: 1,
        items,
        next: None,
    }
}

fn memberships() -> valset::Reply {
    valset::Reply::Memberships(page(vec![
        membership(b"\xab\xcd", "10.0.0.1:4000", valset::Standing::Validator),
        membership(b"\x01\x02", "10.0.0.2:4000", valset::Standing::Resident),
    ]))
}

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => validators(),
            valset::Query::Memberships { .. } => memberships(),
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<RpcLive>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert_eq!(
        cx.host().asked::<Query<Valset>>(),
        vec![
            valset::Query::Validators,
            valset::Query::Memberships {
                page: module_registry::Page {
                    after: None,
                    limit: None
                }
            }
        ]
    );
    assert_eq!(
        cx.host().asked::<RpcLive>(),
        vec![valset::PROGRAM.to_string()]
    );
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
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { interactivity, .. },
    )) = cx.find("nodes-set-header")
    else {
        panic!("section is a native container");
    };
    assert_eq!(interactivity.role, Some(ducktape_view_guest::Role::Heading));
    assert_eq!(interactivity.aria.level, Some(2));
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { children, .. },
    )) = cx.find("nodes-list")
    else {
        panic!("section list is a native container");
    };
    assert_eq!(children[0].key(), Some("nodes-set-header"));
    assert_eq!(children[1].key(), Some("nodes-validators"));
    assert_eq!(children[2].key(), Some("nodes-members-header"));
    assert_eq!(children[3].key(), Some("nodes-members"));
}

#[test]
fn short_ids_cut_on_unicode_boundaries_and_mark_only_a_cut() {
    for (id, keep, want) in [
        ("0123456789", 8, "01234567…"),
        ("01234567", 8, "01234567"),
        ("abc", 8, "abc"),
        ("오리테이프", 2, "오리…"),
        ("abc", 0, "…"),
        ("", 0, ""),
    ] {
        assert_eq!(short_id(id, keep), want, "short_id({id:?}, {keep})");
    }
}

#[test]
fn loading_waits_for_the_host() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<RpcLive>();
    cx.host().never::<Query<Valset>>();
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("Reading the validator set…"));
}

#[test]
fn an_empty_set_says_so() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<RpcLive>();
    cx.host().handle::<Query<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => valset::Reply::Validators(vec![]),
            valset::Query::Memberships { .. } => valset::Reply::Memberships(page(vec![])),
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
    cx.host().stream::<RpcLive>();
    cx.host()
        .refuse::<Query<Valset>>("unavailable", "valset is not running here");
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert!(cx.has_text("valset is not running here"));
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { children, .. },
    )) = cx.find("nodes-refused")
    else {
        panic!("refusal is a native container");
    };
    assert_eq!(
        children.last().and_then(|child| child.key()),
        Some("nodes-retry")
    );
    respond(&mut cx);
    cx.simulate_click("nodes-retry");
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"));
    assert_eq!(cx.host().asked::<Query<Valset>>().len(), 3);
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let mut cx = TestAppContext::new();
    let feed = cx.host().stream::<RpcLive>();
    respond(&mut cx);
    cx.open::<Nodes>();
    cx.run_until_parked();
    cx.host()
        .refuse::<Query<Valset>>("unavailable", "refresh temporarily unavailable");
    feed.push(None);
    cx.run_until_parked();
    assert!(cx.has_text("10.0.0.1:4000"));
    assert_eq!(cx.host().asked::<Query<Valset>>().len(), 3);
    cx.host().handle::<Query<Valset>>(|query| {
        Ok(match query {
            valset::Query::Validators => validators(),
            valset::Query::Memberships { .. } => {
                valset::Reply::Memberships(page(vec![membership(
                    b"\xab\xcd",
                    "10.9.9.9:4000",
                    valset::Standing::Validator,
                )]))
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    feed.push(None);
    cx.run_until_parked();
    assert!(cx.has_text("10.9.9.9:4000") && !cx.has_text("10.0.0.1:4000"));

    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<RpcLive>();
    restored.host().never::<Query<Valset>>();
    restored.restore::<Nodes>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("10.9.9.9:4000"));
    assert_eq!(restored.host().asked::<Query<Valset>>().len(), 1);
    assert_eq!(
        restored.host().asked::<RpcLive>(),
        vec![valset::PROGRAM.to_string()]
    );
}

#[test]
fn the_ready_set_is_accessible() {
    ready().assert_accessible();
}
