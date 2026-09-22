use abi::BlobId;
use base64::Engine as _;
use borsh::BorshDeserialize;
use ducktape_view_guest::testing::{answer, assert_accessible, has_text, item, press, refuse};
use ducktape_view_guest::view::Shell;
use ducktape_view_guest::{Driver, wire};

use super::*;

/// The queries this view sent, read the way the host door reads one: the
/// JSON envelope, the base64 body, the program's own borsh query.
fn asked(frame: &wire::Frame) -> Vec<(u64, registry::Query)> {
    frame
        .requests
        .iter()
        .filter(|request| request.kind == "rpc.query_bytes")
        .filter_map(|request| {
            let envelope: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
            (envelope["target"] == registry::PROGRAM).then(|| {
                let body = base64::engine::general_purpose::STANDARD
                    .decode(envelope["body_b64"].as_str().unwrap())
                    .unwrap();
                (request.id, registry::Query::try_from_slice(&body).unwrap())
            })
        })
        .collect()
}

fn one(frame: &wire::Frame, expected: registry::Query) -> u64 {
    let asked = asked(frame);
    assert_eq!(asked.len(), 1, "one query per step");
    assert_eq!(asked[0].1, expected);
    asked[0].0
}

fn entry(program: &str, code: u8) -> registry::Entry {
    registry::Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: vec![1, 2, 3],
    }
}

fn programs() -> Vec<u8> {
    abi::encode(&registry::Reply::Programs(vec![
        entry("identity", 0xab),
        entry("valset", 0xcd),
    ]))
}

fn scheduled() -> Vec<u8> {
    abi::encode(&registry::Reply::Scheduled(vec![
        registry::Scheduled {
            height: 120,
            change: registry::Change::Set(entry("chat", 0xef)),
        },
        registry::Scheduled {
            height: 200,
            change: registry::Change::Remove("forge".into()),
        },
    ]))
}

/// Boots and answers both reads; hands back the ready frame and the id of
/// the open `rpc.live` watch.
fn ready() -> (Driver<Shell<Explorer>>, wire::Frame, u64) {
    let mut driver = Driver::<Shell<Explorer>>::new();
    let frame = driver.tick(vec![]);
    assert!(has_text(&frame, "Reading the registry…"));
    let live = frame
        .requests
        .iter()
        .find(|request| request.kind == "rpc.live")
        .expect("the view watches the registry")
        .id;
    let at = one(&frame, registry::Query::At(0));
    let frame = driver.tick(vec![answer(at, &programs())]);
    let pending = one(&frame, registry::Query::Scheduled);
    let frame = driver.tick(vec![answer(pending, &scheduled())]);
    (driver, frame, live)
}

#[test]
fn the_registry_lists_what_runs_and_what_is_scheduled() {
    let (_driver, frame, _) = ready();
    let texts = ducktape_view_guest::testing::texts(&frame);
    assert!(has_text(&frame, "2 programs"), "{texts:?}");
    assert!(has_text(&frame, "identity") && has_text(&frame, "valset"));
    assert!(has_text(&frame, "Running") && has_text(&frame, "Scheduled"));
    // a scheduled change says what it does, to what, and when
    assert!(has_text(&frame, "Set") && has_text(&frame, "chat") && has_text(&frame, "at 120"));
    assert!(has_text(&frame, "Remove") && has_text(&frame, "forge") && has_text(&frame, "at 200"));
    // the code blob reaches the screen shortened, never as 64 hex chars
    assert!(
        texts.iter().any(|text| text == "abababababab…"),
        "{texts:?}"
    );
    assert!(has_text(&frame, "3 param bytes"));
}

#[test]
fn an_empty_registry_says_so() {
    let mut driver = Driver::<Shell<Explorer>>::new();
    let frame = driver.tick(vec![]);
    let at = one(&frame, registry::Query::At(0));
    let frame = driver.tick(vec![answer(
        at,
        &abi::encode(&registry::Reply::Programs(vec![])),
    )]);
    let pending = one(&frame, registry::Query::Scheduled);
    let frame = driver.tick(vec![answer(
        pending,
        &abi::encode(&registry::Reply::Scheduled(vec![])),
    )]);
    assert!(has_text(&frame, "No programs"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut driver = Driver::<Shell<Explorer>>::new();
    let frame = driver.tick(vec![]);
    let at = one(&frame, registry::Query::At(0));
    let frame = driver.tick(vec![refuse(at, "the registry is not running here")]);
    assert!(has_text(&frame, "the registry is not running here"));

    let frame = driver.tick(press(&frame, "Retry"));
    let at = one(&frame, registry::Query::At(0));
    let frame = driver.tick(vec![answer(at, &programs())]);
    let pending = one(&frame, registry::Query::Scheduled);
    let frame = driver.tick(vec![answer(pending, &scheduled())]);
    assert!(has_text(&frame, "identity"));
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let (mut driver, _frame, live) = ready();
    let frame = driver.tick(vec![item(live, b"")]);
    // what is on screen stays while the re-read runs
    assert!(has_text(&frame, "identity"));
    let at = one(&frame, registry::Query::At(0));
    let frame = driver.tick(vec![answer(
        at,
        &abi::encode(&registry::Reply::Programs(vec![entry("forge", 0x11)])),
    )]);
    let pending = one(&frame, registry::Query::Scheduled);
    let frame = driver.tick(vec![answer(
        pending,
        &abi::encode(&registry::Reply::Scheduled(vec![])),
    )]);
    assert!(has_text(&frame, "forge") && !has_text(&frame, "identity"));
    assert!(has_text(
        &frame,
        "Nothing is scheduled against the registry."
    ));

    let bytes = driver.snapshot().unwrap();
    let mut restored = Driver::<Shell<Explorer>>::from_snapshot(&bytes, false).unwrap();
    let frame = restored.tick(vec![]);
    assert!(has_text(&frame, "forge"), "a restore keeps the programs");
    assert_eq!(asked(&frame).len(), 1, "and reads them again");
    assert!(
        frame
            .requests
            .iter()
            .any(|request| request.kind == "rpc.live")
    );
}

#[test]
fn the_ready_registry_is_accessible() {
    let (_driver, frame, _) = ready();
    assert_accessible(frame.root.as_ref().expect("a tree"));
}
