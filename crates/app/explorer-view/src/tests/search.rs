use super::*;

#[test]
fn search_finds_heights_hashes_accounts_and_programs() {
    let (mut cx, _) = ready();
    let search = |cx: &mut TestAppContext, text: &str| {
        cx.simulate_input("explorer-search", text);
        cx.simulate_submit("explorer-search");
        cx.run_until_parked();
    };
    search(&mut cx, &abi::hex(&[0xb2; 32]));
    assert!(cx.has_text("In block 12"), "{:?}", cx.texts());
    search(&mut cx, "#3");
    assert!(cx.has_text("account 3   Person   1 device"));
    search(&mut cx, "chat");
    assert!(cx.has_text("Transactions · chat") && cx.has_text("Post in #design"));
    assert!(!cx.has_text("mystery · 4 bytes"));
    // a block hash in the window opens it; one outside is asked of the node
    search(&mut cx, &abi::hex(&[105; 32]));
    assert!(cx.has_text(&abi::hex(&[105; 32])), "{:?}", cx.texts());
    search(&mut cx, &abi::hex(&[0xee; 32]));
    assert!(
        cx.has_text("No block has this hash, and no transaction in the last 13 blocks does."),
        "{:?}",
        cx.texts()
    );
    search(&mut cx, "1,000");
    assert!(cx.has_text("No block 1,000"), "{:?}", cx.texts());
    assert_eq!(
        cx.host().requests::<ChainBlock>(),
        vec![BlockRef::Id([0xee; 32]), BlockRef::Height(1000)]
    );
    search(&mut cx, "nobody");
    assert!(cx.has_text("Nothing here is called “nobody”."));
}

#[test]
fn the_search_field_holds_only_what_is_being_typed() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let explorer = cx.open::<Explorer>();
    cx.run_until_parked();
    let field = |_: &TestAppContext| explorer.read(|view| view.search.clone());
    cx.simulate_input("explorer-search", "11");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert!(cx.has_text("Post in #design"), "block 11 opened");
    assert_eq!(field(&cx), "", "a search that lands clears the field");
    cx.simulate_input("explorer-search", "half typed");
    cx.simulate_click("explorer-next");
    cx.run_until_parked();
    assert_eq!(field(&cx), "", "prev/next clears it");
    cx.simulate_input("explorer-search", "half typed");
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert_eq!(field(&cx), "", "a tab clears it");
    cx.simulate_input("explorer-search", "nobody");
    cx.simulate_submit("explorer-search");
    cx.run_until_parked();
    assert_eq!(field(&cx), "nobody", "a search that finds nothing keeps it");
}

#[test]
fn a_link_opens_the_page_it_names() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    let (_, routes) = node(&mut cx, Rc::new(RefCell::new(12)));
    cx.open::<Explorer>();
    cx.run_until_parked();
    let open = |cx: &mut TestAppContext, route: &str| {
        routes.send(route.to_string());
        cx.run_until_parked();
    };
    open(&mut cx, &format!("tx/{}", abi::hex(&[0xa1; 32])));
    assert!(cx.has_text("In block 11") && cx.has_text("hello there"));
    open(&mut cx, "block/12");
    assert!(cx.has_text(&abi::hex(&[112; 32])), "{:?}", cx.texts());
    open(&mut cx, &format!("block/{}", abi::hex(&[105; 32])));
    assert!(cx.has_text(&abi::hex(&[105; 32])), "a block by its hash");
    open(&mut cx, "account/3");
    assert!(cx.has_text("account 3   Person   1 device"));
    open(&mut cx, "program/chat");
    assert!(cx.has_text("Transactions · chat"));
    // a transaction the window does not hold says how far it looked
    open(&mut cx, &format!("tx/{}", abi::hex(&[0xee; 32])));
    assert!(cx.has_text("Transaction not found"));
    assert!(cx.has_text("It is not in the last 13 blocks this explorer reads."));
    open(&mut cx, "nowhere/1");
    assert!(cx.has_text("This link names nothing the Explorer shows: nowhere/1"));
}

#[test]
fn a_page_copies_its_link_once_the_session_names_a_chain() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    let (props, _routes) = node(&mut cx, Rc::new(RefCell::new(12)));
    let copied = Rc::new(RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host().handle::<ClipboardWrite>(move |text| {
        *seen.borrow_mut() = text;
        Ok(())
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-block-11");
    cx.run_until_parked();
    assert!(cx.find("explorer-copy-link").is_none(), "no chain, no link");
    props.send(Session {
        chain_id: "testkit#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    cx.simulate_click("explorer-copy-link");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        "duck://testkit-0a1b2c3d/explorer/block/11"
    );
    assert!(cx.has_text("Copied the link."));
    cx.assert_accessible();
}

#[test]
fn every_route_reads_back_from_its_path() {
    for route in [
        Route::Overview,
        Route::Blocks,
        Route::Block(0),
        Route::Block(6230),
        Route::Transactions(None),
        Route::Transactions(Some("chat".into())),
        Route::Tx([0xa1; 32]),
        Route::Accounts,
        Route::Account(3),
        Route::Programs,
    ] {
        assert_eq!(
            Route::from_path(&route.path()),
            Some(route.clone()),
            "{route:?}"
        );
    }
    for nothing in [
        "block/07",
        "block/x",
        "tx/zz",
        "program/",
        "account/-1",
        "blocks/1",
    ] {
        assert_eq!(Route::from_path(nothing), None, "{nothing}");
    }
}
