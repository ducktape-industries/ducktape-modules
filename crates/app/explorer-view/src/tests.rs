use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use abi::BlobId;
use ducktape_view_guest::methods::Session;
use ducktape_view_guest::methods::{ChainStatus, Tx, Value};
use ducktape_view_guest::testing::{Feed, TestAppContext};

const ADA: [u8; 32] = [1; 32];
const STRANGER: [u8; 32] = [2; 32];
const VALIDATOR: [u8; 32] = [9; 32];
/// Block times, milliseconds: block `h` lands at `T0 + h` seconds.
const T0: u64 = 1_790_000_000_000;

fn post(channel: &str, text: &str) -> Vec<u8> {
    borsh::to_vec(&chat::Op::PostMessage {
        channel_id: channel.into(),
        message_id: "m1".into(),
        blocks: chat::parse_message(text),
        thread: None,
    })
    .unwrap()
}

fn tx(seed: u8, signer: [u8; 32], target: &str, payload: Vec<u8>) -> Tx {
    Tx {
        hash: [seed; 32],
        signer: signer.to_vec(),
        seq: seed as u64,
        target: target.into(),
        payload,
    }
}

/// Blocks 0..=`tip`: 11 carries Ada's post and her DM to account 7, 12 a
/// stranger's op to a program that describes nothing.
fn chain(tip: u64) -> Vec<Block> {
    (0..=tip)
        .map(|height| Block {
            height,
            id: [(height as u8).wrapping_add(100); 32],
            parent: [(height as u8).wrapping_add(99); 32],
            time: T0 + height * 1000,
            epoch: height / 10,
            proposer: Some(VALIDATOR.to_vec()),
            txs: match height {
                11 => vec![
                    tx(0xa1, ADA, "chat", post("design", "hello there")),
                    tx(0xc3, ADA, "chat", post(&chat::dm_channel_id(7, 3), "ping")),
                ],
                12 => vec![tx(0xb2, STRANGER, "mystery", vec![1, 2, 3, 4])],
                _ => Vec::new(),
            },
        })
        .collect()
}

fn page(chain: &[Block], ask: &BlockPage) -> Vec<Block> {
    let top = chain.len() as u64 - 1;
    let Some(start) = ask.before.map_or(Some(top), |before| before.checked_sub(1)) else {
        return Vec::new();
    };
    (0..=start.min(top))
        .rev()
        .take(ask.limit as usize)
        .map(|height| chain[height as usize].clone())
        .collect()
}

fn status(height: u64) -> NodeStatus {
    NodeStatus {
        network: "test#1".into(),
        block_time_ms: 1000,
        epoch_length: 10,
        height,
        epoch: (height + 1) / 10,
        ..NodeStatus::default()
    }
}

fn ada() -> identity::Account {
    identity::Account {
        number: 3,
        name: "Ada".into(),
        control: identity::Control::Keys(vec![identity::Key {
            scheme: abi::Scheme::Ed25519,
            key: ADA.to_vec(),
            label: Some("laptop".into()),
            added_at: 0,
        }]),
        avatar: None,
        bio: None,
        updated_at: 0,
    }
}

/// A node at `tip`, whose tip the test may move.
fn node(cx: &mut TestAppContext, tip: Rc<RefCell<u64>>) -> (Feed<HostSession>, Feed<HostRoute>) {
    let feeds = (
        cx.host().stream::<HostSession>(),
        cx.host().stream::<HostRoute>(),
    );
    let host = cx.host();
    let head = tip.clone();
    host.handle::<ChainStatus>(move |()| Ok(status(*head.borrow())));
    let head = tip.clone();
    host.handle::<ChainBlocks>(move |ask| Ok(page(&chain(*head.borrow()), &ask)));
    host.handle::<ChainBlock>(move |by| {
        let chain = chain(*tip.borrow());
        Ok(match by {
            BlockRef::Height(height) => chain.get(height as usize).cloned(),
            BlockRef::Id(id) => chain.into_iter().find(|block| block.id == id),
        })
    });
    host.handle::<Query<Identity>>(|query| match query {
        identity::Query::List { .. } => {
            Ok(identity::Reply::Accounts(module_registry::PageResponse {
                height: 1,
                items: vec![ada()],
                next: None,
            }))
        }
        other => panic!("unexpected identity query: {other:?}"),
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
    feeds
}

fn entry(program: &str, code: u8) -> registry::Entry {
    registry::Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: vec![1, 2, 3],
    }
}

fn respond(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Registry>>(|query| {
        Ok(match query {
            registry::Query::At(0) => {
                registry::Reply::Programs(vec![entry("chat", 0xab), entry("identity", 0xcd)])
            }
            registry::Query::Views(0) => registry::Reply::Views(vec![registry::View {
                name: "explorer".into(),
                view: BlobId::Sha256([0xef; 32]),
            }]),
            registry::Query::Scheduled { .. } => {
                registry::Reply::Scheduled(module_registry::PageResponse {
                    height: 1,
                    items: vec![registry::Scheduled {
                        height: 120,
                        change: registry::Change::Remove("forge".into()),
                    }],
                    next: None,
                })
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

fn ready() -> (TestAppContext, Rc<RefCell<u64>>) {
    let mut cx = TestAppContext::new();
    cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    (cx, tip)
}

// ---------- decoding ----------

/// The host's `program.describe`, as each program's describe module would
/// answer: its own `describe` over the op, `None` for a program without one.
fn describes(cx: &mut TestAppContext) {
    fn with<T: borsh::BorshDeserialize>(
        op: &[u8],
        describe: fn(&T) -> Description,
    ) -> Option<Description> {
        borsh::from_slice(op).ok().map(|op| describe(&op))
    }
    cx.host().handle::<ProgramDescribe>(|(program, op)| {
        Ok(match program.as_str() {
            "chat" => with(&op, chat::describe),
            "forge" => with(&op, forge::describe),
            "identity" => with(&op, identity::describe),
            "valset" => with(&op, valset::describe),
            registry::MODULE => with(&op, registry::describe),
            _ => None,
        })
    });
}

#[test]
fn an_op_reads_as_its_program_describes_it_through_the_host() {
    let (mut cx, _) = ready();
    // Ada's post, as chat described it
    assert!(cx.has_text("Post in #design"), "{:?}", cx.texts());
    // one the host could not describe reads as its bytes
    assert!(cx.has_text("mystery · 4 bytes"), "{:?}", cx.texts());
    let asked = cx.host().asked::<ProgramDescribe>();
    assert!(asked.contains(&("mystery".to_owned(), vec![1, 2, 3, 4])));

    // a dm post: its title, the two accounts by name and link
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&[0xc3; 32])));
    cx.run_until_parked();
    let texts = cx.texts();
    assert!(cx.has_text("Direct message"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("↔")), "{texts:?}");
    assert!(cx.has_text("between") && cx.has_text("Ada") && cx.has_text("account 7"));
    cx.simulate_click("explorer-value-1-0");
    cx.run_until_parked();
    assert!(
        cx.has_text("laptop"),
        "the account link opens Ada: {:?}",
        cx.texts()
    );
    cx.assert_accessible();
}

#[test]
fn an_undescribed_op_shows_its_size_and_bytes() {
    let (mut cx, _) = ready();
    cx.simulate_click(&format!("explorer-tx-{}", abi::hex(&[0xb2; 32])));
    cx.run_until_parked();
    assert!(cx.has_text("mystery · 4 bytes"), "{:?}", cx.texts());
    assert!(cx.has_text("bytes") && cx.has_text("01020304"));
    assert_eq!(decode::bytes("chat", &[0xff; 3]).title, "chat · 3 bytes");
}

#[test]
fn values_read_as_a_person_reads_them() {
    assert_eq!(decode::preview(0, &[]), "0 bytes");
    let push = |len: usize| match Value::bytes(&vec![7; len]) {
        Value::Bytes { len, preview } => decode::preview(len, &preview),
        _ => unreachable!(),
    };
    assert_eq!(push(100), "100 bytes · 07070707…0707");
    assert_eq!(push(1 << 20), "1048576 bytes · 07070707…0707");
    assert_eq!(push(4), "07070707");
    assert_eq!(decode::amount(123_456_789, 2), "1,234,567.89");
    assert_eq!(decode::amount(5, 3), "0.005");
    assert_eq!(decode::amount(42, 0), "42");
}

#[test]
fn numbers_hashes_and_times_read_as_a_person_reads_them() {
    assert_eq!(decode::grouped(6230), "6,230");
    assert_eq!(decode::grouped(1_000_000), "1,000,000");
    assert_eq!(decode::grouped(12), "12");
    assert_eq!(decode::short(&[0xab; 32]), "abababab…abab");
    assert_eq!(decode::ago(10_000, 8_000), "2s");
    assert_eq!(decode::ago(4_000_000, 0), "1h");
    assert_eq!(decode::date(0), "1 Jan 1970, 00:00:00");
    assert_eq!(decode::date(1_790_236_327_000), "24 Sep 2026, 07:52:07");
    assert_eq!(decode::date(951_782_400_000), "29 Feb 2000, 00:00:00");
}

// ---------- the window ----------

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

// ---------- pages ----------

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
        cx.host().asked::<ChainBlocks>(),
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
        cx.host().asked::<ChainBlock>().is_empty(),
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
    assert!(cx.has_text("account 3   1 device"), "{texts:?}");
    assert!(
        cx.has_text("laptop") && cx.has_text("last used 1s ago"),
        "{texts:?}"
    );
    assert!(cx.has_text("Programs used") && cx.has_text("2 tx"));
    assert!(!cx.has_text("mystery · 4 bytes"), "not Ada's");
    cx.assert_accessible();
}

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
    assert!(cx.has_text("account 3   1 device"));
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
        cx.host().asked::<ChainBlock>(),
        vec![BlockRef::Id([0xee; 32]), BlockRef::Height(1000)]
    );
    search(&mut cx, "nobody");
    assert!(cx.has_text("Nothing here is called “nobody”."));
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
    heads.push(Head {
        height: 14,
        time: T0 + 14_000,
        id: [14; 32],
    });
    cx.run_until_parked();
    assert!(cx.has_text("13–14 · 2 empty blocks"), "{:?}", cx.texts());
    let explorer_asked = cx.host().asked::<ChainBlocks>();
    assert_eq!(explorer_asked.len(), 2, "{explorer_asked:?}");
    assert!(explorer_asked.iter().all(|ask| ask.before.is_none()));
    assert_eq!(
        cx.host().asked::<ChainStatus>().len(),
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
    assert_eq!(cx.host().asked::<ClockTicks>(), [TICK]);
    *tip.borrow_mut() = 14;
    ticks.push(());
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
    assert!(cx.has_text("Latest blocks"));
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
fn a_snapshot_restores_without_reading_the_window_again() {
    let (cx, _) = ready();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<ChainHeads>();
    restored.host().never::<ChainStatus>();
    restored.host().never::<HostSession>();
    restored.host().never::<HostRoute>();
    restored.host().never::<ChainBlocks>();
    restored.host().never::<Query<Identity>>();
    restored.host().never::<Query<Valset>>();
    restored.host().never::<Query<Registry>>();
    restored.host().never::<ProgramDescribe>();
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("Post in #design"));
    assert!(restored.host().asked::<ChainBlocks>().is_empty());
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
        routes.push(route.to_string());
        cx.run_until_parked();
    };
    open(&mut cx, &format!("tx/{}", abi::hex(&[0xa1; 32])));
    assert!(cx.has_text("In block 11") && cx.has_text("hello there"));
    open(&mut cx, "block/12");
    assert!(cx.has_text(&abi::hex(&[112; 32])), "{:?}", cx.texts());
    open(&mut cx, &format!("block/{}", abi::hex(&[105; 32])));
    assert!(cx.has_text(&abi::hex(&[105; 32])), "a block by its hash");
    open(&mut cx, "account/3");
    assert!(cx.has_text("account 3   1 device"));
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
    props.push(Session {
        chain: "testkit#0a1b2c3d".into(),
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

#[test]
fn the_scheduled_changes_survive_a_snapshot() {
    let (mut cx, _) = ready();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    restored.host().stream::<ChainHeads>();
    restored.host().never::<ChainStatus>();
    restored.host().never::<HostSession>();
    restored.host().never::<HostRoute>();
    restored.host().never::<ChainBlocks>();
    restored.host().never::<Query<Identity>>();
    restored.host().never::<Query<Valset>>();
    restored.host().never::<Query<Registry>>();
    restored.restore::<Explorer>(&bytes).unwrap();
    restored.run_until_parked();
    assert!(restored.has_text("Remove") && restored.has_text("at 120"));
}

// ---------- frame budgets ----------

/// A full window: 1,000 blocks, each with a post, and a 1 MB push at the tip.
fn heavy(cx: &mut TestAppContext) {
    let tip = WINDOW as u64;
    let chain: Vec<Block> = (0..=tip)
        .map(|height| {
            let seed = (height % 250) as u8;
            let mut tx = if height == tip {
                let push = forge::Op::Push {
                    repo: "app".into(),
                    request: vec![0x50; 1 << 20],
                };
                tx(0xfe, ADA, forge::MODULE, borsh::to_vec(&push).unwrap())
            } else {
                let text = format!("message {height}");
                tx(seed, ADA, chat::MODULE, post("design", &text))
            };
            tx.hash[..8].copy_from_slice(&height.to_le_bytes());
            Block {
                height,
                id: [seed; 32],
                parent: [seed.wrapping_sub(1); 32],
                time: T0 + height * 1000,
                epoch: height / 10,
                proposer: Some(VALIDATOR.to_vec()),
                txs: vec![tx],
            }
        })
        .collect();
    let host = cx.host();
    host.stream::<ChainHeads>();
    host.stream::<HostSession>();
    host.stream::<HostRoute>();
    host.handle::<ChainStatus>(move |()| Ok(status(tip)));
    let blocks = chain.clone();
    host.handle::<ChainBlocks>(move |ask| Ok(page(&blocks, &ask)));
    host.handle::<ChainBlock>(move |by| {
        Ok(match by {
            BlockRef::Height(height) => chain.get(height as usize).cloned(),
            BlockRef::Id(id) => chain.iter().find(|block| block.id == id).cloned(),
        })
    });
    host.handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(module_registry::PageResponse {
            height: 1,
            items: vec![ada()],
            next: None,
        }))
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
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
