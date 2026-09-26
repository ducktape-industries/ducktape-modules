use super::*;

#[test]
fn the_overview_shows_the_head_and_the_latest_blocks_and_transactions() {
    let (cx, _) = ready();
    let texts = cx.texts();
    assert!(
        cx.has_text("Height") && cx.has_text("Block every 1.0 s"),
        "{texts:?}"
    );
    assert!(cx.has_text("Next in 7 blocks"), "{texts:?}");
    assert!(cx.has_text("Validators") && cx.has_text("Accounts"));
    assert!(!cx.has_text("Transactions ") && !texts.iter().any(|t| t.contains("tx count")));
    assert!(cx.has_text("Latest blocks") && cx.has_text("Latest transactions"));
    assert!(cx.has_text("Post in #design") && cx.has_text("Ada") && cx.has_text("#3"));
    assert!(cx.has_text("mystery · 4 bytes") && cx.has_text("02020202…0202"));
    assert!(
        cx.has_text("6f6f6f6f…6f6f"),
        "block 11's hash, shortened: {texts:?}"
    );
    // blocks 0–10 carry nothing: one quiet line, not eleven rows
    assert!(cx.has_text("0–10 · 11 empty blocks"), "{texts:?}");
    assert!(
        !cx.has_text("6c6c6c6c…6c6c"),
        "block 8 is folded: {texts:?}"
    );
    assert_eq!(
        cx.host().requests::<ChainBlocks>(),
        vec![BlockPage {
            before: None,
            limit: PAGE
        }],
        "13 blocks is the whole archive: one page"
    );
    cx.assert_accessible();
}

#[test]
fn a_block_opens_with_its_fields_its_proposer_and_its_transactions() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(
        cx.has_text("11") && cx.has_text(&abi::hex(&[111; 32])),
        "{texts:?}"
    );
    assert!(cx.has_text("validator 1"), "{texts:?}");
    assert!(cx.has_text("Post in #design"));
    assert!(
        !texts.iter().any(|t| t.contains("Applied")),
        "no receipts: {texts:?}"
    );
    assert!(!texts.iter().any(|t| t.contains("State root")), "{texts:?}");
    cx.assert_accessible();
    cx.simulate_click("explorer-next");
    cx.run_until_parked();
    assert!(cx.has_text("mystery · 4 bytes"));
    assert!(
        cx.host().requests::<ChainBlock>().is_empty(),
        "both were in the window"
    );
}

#[test]
fn a_transaction_shows_its_block_signer_and_operation() {
    let (mut cx, _) = ready();
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&[0xa1; 32])));
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("In block 11"), "{texts:?}");
    assert!(cx.has_text(&abi::hex(&[0xa1; 32])));
    assert!(
        cx.has_text("#3 laptop · ed25519 01010101…0101"),
        "{texts:?}"
    );
    assert!(cx.has_text("code abababab…abab"), "{texts:?}");
    assert!(cx.has_text("channel") && cx.has_text("#design"));
    assert!(cx.has_text("text") && cx.has_text("hello there"));
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("Applied") || t.contains("Rejected"))
    );
    cx.assert_accessible();
    cx.simulate_click("explorer-from");
    cx.run_until_parked();
    assert!(
        cx.has_text("2 transactions in the last 13 blocks"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn an_account_shows_its_devices_and_what_it_used_in_the_window() {
    let (mut cx, _) = ready();
    cx.simulate_input("explorer-search", "ada");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("account 3   Person   1 device"), "{texts:?}");
    assert!(
        cx.has_text("laptop") && cx.has_text("last used 1s ago"),
        "{texts:?}"
    );
    assert!(cx.has_text("Programs used") && cx.has_text("2 tx"));
    assert!(!cx.has_text("mystery · 4 bytes"), "not Ada's");
    cx.assert_accessible();
}

#[test]
fn programs_lists_what_runs_and_what_is_scheduled() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("2 programs") && cx.has_text("identity"));
    assert!(cx.has_text("Remove") && cx.has_text("forge") && cx.has_text("at 120"));
    assert!(cx.texts().iter().any(|text| text == "abababab…abab"));
    assert!(
        cx.has_text("1 view") && cx.has_text("explorer") && cx.has_text("view only"),
        "a view-only entry is listed beside the programs"
    );
    cx.assert_accessible();
}

#[test]
fn the_scheduled_changes_survive_a_snapshot() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    quiet_host(&restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("Remove") && restored.has_text("at 120"));
}

#[test]
fn the_root_tracks_the_shared_theme() {
    let (mut cx, _) = ready();
    let dark = ducktape_view_guest::Theme::dark();
    cx.set_global(dark);
    let Some(ducktape_view_guest::wire::Node::Container(
        ducktape_view_guest::wire::ContainerNode { style, .. },
    )) = cx.find("explorer")
    else {
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
}

#[test]
fn accounts_say_what_each_is_as_members_does() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    for text in [
        "Person",
        "Agent · managed by Ada · suspended",
        "Module · forge",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_click("explorer-account-5");
    cx.run_until_parked();
    assert!(
        cx.has_text("account 5   Agent · managed by Ada · suspended   0 devices"),
        "{:?}",
        cx.texts()
    );
}
