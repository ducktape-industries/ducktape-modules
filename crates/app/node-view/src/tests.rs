use base64::Engine as _;
use borsh::BorshDeserialize;
use ducktape_view_guest::testing::{answer, assert_accessible, has_text, item, press, refuse};
use ducktape_view_guest::view::Shell;
use ducktape_view_guest::{Driver, wire};

use super::*;

/// The queries this view sent, read the way the host door reads one: the
/// JSON envelope, the base64 body, the program's own borsh query.
fn asked(frame: &wire::Frame) -> Vec<(u64, valset::Query)> {
    frame
        .requests
        .iter()
        .filter(|request| request.kind == "rpc.query_bytes")
        .filter_map(|request| {
            let envelope: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
            (envelope["target"] == valset::PROGRAM).then(|| {
                let body = base64::engine::general_purpose::STANDARD
                    .decode(envelope["body_b64"].as_str().unwrap())
                    .unwrap();
                (request.id, valset::Query::try_from_slice(&body).unwrap())
            })
        })
        .collect()
}

fn one(frame: &wire::Frame, expected: valset::Query) -> u64 {
    let asked = asked(frame);
    assert_eq!(asked.len(), 1, "one query per step");
    assert_eq!(asked[0].1, expected);
    asked[0].0
}

fn membership(key: &[u8], address: &str, standing: valset::Standing) -> valset::Membership {
    valset::Membership {
        key: key.to_vec(),
        address: address.into(),
        standing,
    }
}

fn validators() -> Vec<u8> {
    abi::encode(&valset::Reply::Validators(vec![vec![0xab, 0xcd]]))
}

fn memberships() -> Vec<u8> {
    abi::encode(&valset::Reply::Memberships(vec![
        membership(b"\xab\xcd", "10.0.0.1:4000", valset::Standing::Validator),
        membership(b"\x01\x02", "10.0.0.2:4000", valset::Standing::Resident),
    ]))
}

/// Boots and answers both reads; hands back the ready frame and the id of
/// the open `rpc.live` watch.
fn ready() -> (Driver<Shell<Nodes>>, wire::Frame, u64) {
    let mut driver = Driver::<Shell<Nodes>>::new();
    let frame = driver.tick(vec![]);
    assert!(has_text(&frame, "Reading the validator set…"));
    let live = frame
        .requests
        .iter()
        .find(|request| request.kind == "rpc.live")
        .expect("the view watches valset")
        .id;
    let keys = one(&frame, valset::Query::Validators);
    let frame = driver.tick(vec![answer(keys, &validators())]);
    let set = one(&frame, valset::Query::Memberships);
    let frame = driver.tick(vec![answer(set, &memberships())]);
    (driver, frame, live)
}

#[test]
fn the_set_shows_its_validators_memberships_and_counts() {
    let (_driver, frame, _) = ready();
    let texts = ducktape_view_guest::testing::texts(&frame);
    assert!(has_text(&frame, "1 validator · 2 members"), "{texts:?}");
    assert!(has_text(&frame, "Validator set") && has_text(&frame, "Memberships"));
    assert!(has_text(&frame, "10.0.0.1:4000") && has_text(&frame, "10.0.0.2:4000"));
    assert!(has_text(&frame, "Validator") && has_text(&frame, "Resident"));
    // a key reaches the screen shortened, never raw
    assert!(texts.iter().any(|text| text == "abcd"), "{texts:?}");
}

#[test]
fn an_empty_set_says_so() {
    let mut driver = Driver::<Shell<Nodes>>::new();
    let frame = driver.tick(vec![]);
    let keys = one(&frame, valset::Query::Validators);
    let frame = driver.tick(vec![answer(
        keys,
        &abi::encode(&valset::Reply::Validators(vec![])),
    )]);
    let set = one(&frame, valset::Query::Memberships);
    let frame = driver.tick(vec![answer(
        set,
        &abi::encode(&valset::Reply::Memberships(vec![])),
    )]);
    assert!(has_text(&frame, "No members"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut driver = Driver::<Shell<Nodes>>::new();
    let frame = driver.tick(vec![]);
    let keys = one(&frame, valset::Query::Validators);
    let frame = driver.tick(vec![refuse(keys, "valset is not running here")]);
    assert!(has_text(&frame, "valset is not running here"));

    let frame = driver.tick(press(&frame, "Retry"));
    let keys = one(&frame, valset::Query::Validators);
    let frame = driver.tick(vec![answer(keys, &validators())]);
    let set = one(&frame, valset::Query::Memberships);
    let frame = driver.tick(vec![answer(set, &memberships())]);
    assert!(has_text(&frame, "10.0.0.1:4000"));
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let (mut driver, _frame, live) = ready();
    let frame = driver.tick(vec![item(live, b"")]);
    // what is on screen stays while the re-read runs
    assert!(has_text(&frame, "10.0.0.1:4000"));
    let keys = one(&frame, valset::Query::Validators);
    let frame = driver.tick(vec![answer(keys, &validators())]);
    let set = one(&frame, valset::Query::Memberships);
    let frame = driver.tick(vec![answer(
        set,
        &abi::encode(&valset::Reply::Memberships(vec![membership(
            b"\xab\xcd",
            "10.9.9.9:4000",
            valset::Standing::Validator,
        )])),
    )]);
    assert!(has_text(&frame, "10.9.9.9:4000") && !has_text(&frame, "10.0.0.2:4000"));

    let bytes = driver.snapshot().unwrap();
    let mut restored = Driver::<Shell<Nodes>>::from_snapshot(&bytes, false).unwrap();
    let frame = restored.tick(vec![]);
    assert!(has_text(&frame, "10.9.9.9:4000"), "a restore keeps the set");
    assert_eq!(asked(&frame).len(), 1, "and reads it again");
    assert!(
        frame
            .requests
            .iter()
            .any(|request| request.kind == "rpc.live")
    );
}

#[test]
fn the_ready_set_is_accessible() {
    let (_driver, frame, _) = ready();
    assert_accessible(frame.root.as_ref().expect("a tree"));
}
