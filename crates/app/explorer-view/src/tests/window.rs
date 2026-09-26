use super::*;

#[test]
fn the_window_follows_the_head_and_stops_where_the_archive_does() {
    let mut window = Chain::default();
    let blocks = chain(30);
    assert_eq!(PAGE, 20, "the sizes below assume a page of 20");
    let ask = |before| BlockPage {
        before,
        limit: PAGE,
    };
    window.land(None, page(&blocks, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(30), 20));
    assert!(!window.complete, "a full page may have more below it");
    window.land(Some(11), page(&blocks, &ask(Some(11))));
    assert_eq!(window.blocks.len(), 31);
    assert!(window.complete);
    let more = chain(32);
    window.land(None, page(&more, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(32), 33));
    assert!(
        window
            .blocks
            .windows(2)
            .all(|pair| pair[0].height == pair[1].height + 1)
    );
    assert_eq!(window.txs.len(), 3, "no transaction is folded in twice");
    assert_eq!(window.block(11).map(|block| block.txs), Some(2));
    // a head that no longer joins the window starts it again
    let far = chain(400);
    window.land(None, page(&far, &ask(None)));
    assert_eq!((window.top(), window.blocks.len()), (Some(400), 20));
    assert!(!window.complete && window.txs.is_empty());
}

#[test]
fn a_pushed_head_reads_only_the_new_blocks() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    *tip.borrow_mut() = 14;
    heads.send(Head {
        height: 14,
        time: T0 + 14_000,
        id: [14; 32],
    });
    cx.run_until_parked();
    assert!(cx.has_text("13–14 · 2 empty blocks"), "{:?}", cx.texts());
    let explorer_asked = cx.host().requests::<ChainBlocks>();
    assert_eq!(explorer_asked.len(), 2, "{explorer_asked:?}");
    assert!(explorer_asked.iter().all(|ask| ask.before.is_none()));
    assert_eq!(
        cx.host().requests::<ChainStatus>().len(),
        1,
        "a head moves the status without a read"
    );
}

#[test]
fn a_refused_head_subscription_falls_back_to_polling() {
    let mut cx = TestAppContext::new();
    cx.host()
        .refuse::<ChainHeads>("unknown_request", "this host has no chain.heads");
    let ticks = cx.host().stream::<ClockTicks>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<ClockTicks>(), [TICK]);
    *tip.borrow_mut() = 14;
    ticks.send(());
    cx.run_until_parked();
    assert!(cx.has_text("13–14 · 2 empty blocks"), "{:?}", cx.texts());
}

#[test]
fn a_refused_window_says_why_and_retry_reads_again() {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip);
    cx.host()
        .refuse::<ChainBlocks>("not_found", "this node serves no blocks");
    cx.open::<Explorer>();
    cx.run_until_parked();
    assert!(cx.has_text("this node serves no blocks"));
    let chain_ = chain(12);
    cx.host()
        .handle::<ChainBlocks>(move |ask| Ok(page(&chain_, &ask)));
    cx.simulate_click("explorer-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Latest activity"));
}

#[test]
fn a_snapshot_restores_without_reading_the_window_again() {
    let (cx, _) = ready();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    quiet_host(&restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("Post in #design"));
    assert!(restored.host().requests::<ChainBlocks>().is_empty());
}

/// Every page over a full window stays inside the host's frame budgets, and
/// under a per-page regression guard on its bytes: the native proxy for a
/// render's fuel.
/// (Time is no proxy here: a debug build JSON-encodes the whole view around
/// every update to catch a missed `notify`.)
#[test]
fn a_full_window_renders_inside_the_frame_budget() {
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let mut sizes = vec![("overview", cx.frame_bytes())];
    for tab in ["blocks", "transactions", "accounts", "programs"] {
        cx.simulate_click(&format!("explorer-tab-{tab}"));
        cx.run_until_parked();
        sizes.push((tab, cx.frame_bytes()));
    }
    let mut big = [0xfe; 32];
    big[..8].copy_from_slice(&(WINDOW as u64).to_le_bytes());
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&big)));
    cx.run_until_parked();
    assert!(cx.has_text("1048576 bytes · 50505050…5050"));
    sizes.push(("a 1 MB push", cx.frame_bytes()));
    // A regression guard, not a host limit: about 1.5x what each page drew
    // when measured. The host's own limits are the sanitize check inside
    // `frame_bytes`. Tighten when a page slims, raise only on purpose.
    const REGRESSION_GUARD: [(&str, usize); 6] = [
        ("overview", 76_000),
        ("blocks", 95_000),
        ("transactions", 196_000),
        ("accounts", 9_000),
        ("programs", 13_000),
        ("a 1 MB push", 16_000),
    ];
    for ((page, bytes), (_, guard)) in sizes.into_iter().zip(REGRESSION_GUARD) {
        assert!(
            bytes < guard,
            "{page} drew {bytes} bytes, over its guard {guard}"
        );
    }
}

/// The snapshot keeps each transaction's decoded op, not its payload: a
/// 1 MB push in the window does not ride along, and a restored row still
/// reads as its op.
#[test]
fn a_snapshot_keeps_ops_not_payloads() {
    let mut cx = TestAppContext::new();
    heavy(&mut cx);
    cx.open::<Explorer>();
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    assert!(bytes.len() < 1 << 20, "a snapshot of {} bytes", bytes.len());
    let mut restored = TestAppContext::new();
    heavy(&mut restored);
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    let mut big = [0xfe; 32];
    big[..8].copy_from_slice(&(WINDOW as u64).to_le_bytes());
    restored.simulate_click("explorer-tab-transactions");
    restored.run_until_parked();
    restored.simulate_click(&format!("explorer-tx-{}", abi::hex(&big)));
    restored.run_until_parked();
    assert!(restored.has_text("Push · app"), "{:?}", restored.texts());
}

#[test]
fn runs_of_empty_blocks_fold_into_one_line_and_the_list_reaches_back() {
    let block = |height: u64, txs: usize| BlockRow {
        height,
        txs,
        ..BlockRow::default()
    };
    // newest first: 20 empty, 19 busy, 18 empty, 17 busy, 16..=3 empty, 2 busy
    let mut blocks = vec![block(20, 0), block(19, 2), block(18, 0), block(17, 1)];
    blocks.extend((3..=16).rev().map(|height| block(height, 0)));
    blocks.push(block(2, 1));
    let lines = ui::lines(&blocks, 6);
    let shape: Vec<String> = lines
        .iter()
        .map(|line| match line {
            ui::Line::Block(block) => block.height.to_string(),
            ui::Line::Empty { newest, oldest } => format!("{oldest}-{newest}"),
        })
        .collect();
    // a lone empty block stays a row; a run folds; the fold lets six lines
    // reach back to block 2
    assert_eq!(shape, ["20", "19", "18", "17", "3-16", "2"]);
    assert_eq!(ui::lines(&blocks, 3).len(), 3);
}
