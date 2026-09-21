use abi::Scheme;
use base64::Engine as _;
use borsh::BorshDeserialize;
use ducktape_view_guest::testing::{answer, assert_accessible, has_text, press, refuse, type_into};
use ducktape_view_guest::view::Shell;
use ducktape_view_guest::{Driver, wire};

use super::*;

/// The queries this view sent to `target`, read the way the host door reads
/// one: the JSON envelope, the base64 body, the program's own borsh query.
fn asked<Q: BorshDeserialize>(frame: &wire::Frame, target: &str) -> Vec<(u64, Q)> {
    frame
        .requests
        .iter()
        .filter(|request| request.kind == "rpc.query_bytes")
        .filter_map(|request| {
            let envelope: serde_json::Value = serde_json::from_slice(&request.payload).unwrap();
            (envelope["target"] == target).then(|| {
                let body = base64::engine::general_purpose::STANDARD
                    .decode(envelope["body_b64"].as_str().unwrap())
                    .unwrap();
                (request.id, Q::try_from_slice(&body).unwrap())
            })
        })
        .collect()
}

fn one<Q: BorshDeserialize>(frame: &wire::Frame, target: &str) -> (u64, Q) {
    let mut asked = asked(frame, target);
    assert_eq!(asked.len(), 1, "one query to {target} per read");
    asked.remove(0)
}

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

fn accounts() -> Vec<u8> {
    abi::encode(&identity::Reply::Accounts(vec![
        person(7, "eddy", b"\x01\x02"),
        program(8, "chat"),
    ]))
}

fn memberships() -> Vec<u8> {
    abi::encode(&valset::Reply::Memberships(vec![membership(
        b"\x01\x02",
        valset::Standing::Validator,
    )]))
}

/// Boots, answers the roster and the validator set, and hands back the ready
/// frame with the id of the open `rpc.live` watch. The two reads are
/// sequential: the join needs the accounts first.
fn ready() -> (Driver<Shell<Members>>, wire::Frame, u64) {
    let mut driver = Driver::<Shell<Members>>::new();
    let frame = driver.tick(vec![]);
    assert!(has_text(&frame, "Reading the roster…"));
    let live = frame
        .requests
        .iter()
        .find(|request| request.kind == "rpc.live")
        .expect("the view watches the identity program")
        .id;
    let (list, query) = one::<identity::Query>(&frame, identity::PROGRAM);
    assert_eq!(query, identity::Query::List { page: Page::all() });

    let frame = driver.tick(vec![answer(list, &accounts())]);
    let (set, query) = one::<valset::Query>(&frame, valset::PROGRAM);
    assert_eq!(query, valset::Query::Memberships);
    let frame = driver.tick(vec![answer(set, &memberships())]);
    (driver, frame, live)
}

#[test]
fn the_roster_lists_each_account_with_its_standing() {
    let (_driver, frame, _) = ready();
    for text in ["eddy", "chat", "#7", "#8", "Person", "Program", "Validator"] {
        assert!(
            has_text(&frame, text),
            "{:?}",
            ducktape_view_guest::testing::texts(&frame)
        );
    }
    assert!(has_text(&frame, "2 accounts"));
    // the program account holds no key of its own, so it wears no standing
    assert_eq!(
        ducktape_view_guest::testing::texts(&frame)
            .iter()
            .filter(|text| *text == "Validator")
            .count(),
        1
    );
}

#[test]
fn a_roster_with_nobody_in_it_says_so() {
    let mut driver = Driver::<Shell<Members>>::new();
    let frame = driver.tick(vec![]);
    let (list, _) = one::<identity::Query>(&frame, identity::PROGRAM);
    let frame = driver.tick(vec![answer(
        list,
        &abi::encode(&identity::Reply::Accounts(vec![])),
    )]);
    let (set, _) = one::<valset::Query>(&frame, valset::PROGRAM);
    let frame = driver.tick(vec![answer(
        set,
        &abi::encode(&valset::Reply::Memberships(vec![])),
    )]);
    assert!(has_text(&frame, "No accounts"));
}

#[test]
fn a_refusal_shows_its_sentence_and_retry_asks_again() {
    let mut driver = Driver::<Shell<Members>>::new();
    let frame = driver.tick(vec![]);
    let (list, _) = one::<identity::Query>(&frame, identity::PROGRAM);
    let frame = driver.tick(vec![refuse(list, "identity is not running here")]);
    assert!(has_text(&frame, "identity is not running here"));

    let frame = driver.tick(press(&frame, "Retry"));
    let (list, _) = one::<identity::Query>(&frame, identity::PROGRAM);
    let frame = driver.tick(vec![answer(list, &accounts())]);
    let (set, _) = one::<valset::Query>(&frame, valset::PROGRAM);
    let frame = driver.tick(vec![answer(set, &memberships())]);
    assert!(has_text(&frame, "eddy"));
}

#[test]
fn the_filter_narrows_the_list_without_asking_again() {
    let (mut driver, frame, _) = ready();
    let frame = driver.tick(type_into(&frame, "members/filter", "ed"));
    assert!(has_text(&frame, "eddy") && !has_text(&frame, "chat"));
    assert!(asked::<identity::Query>(&frame, identity::PROGRAM).is_empty());

    let frame = driver.tick(type_into(&frame, "members/filter", "8"));
    assert!(has_text(&frame, "chat") && !has_text(&frame, "eddy"));

    let frame = driver.tick(type_into(&frame, "members/filter", "nobody"));
    assert!(has_text(&frame, "Nothing matches"));
}

#[test]
fn a_live_bump_re_reads_and_a_snapshot_restores_the_screen() {
    let (mut driver, _frame, live) = ready();
    let frame = driver.tick(vec![ducktape_view_guest::testing::item(live, b"")]);
    let (list, _) = one::<identity::Query>(&frame, identity::PROGRAM);
    // the rows already on screen stay there while the re-read runs
    assert!(has_text(&frame, "eddy"));
    let frame = driver.tick(vec![answer(
        list,
        &abi::encode(&identity::Reply::Accounts(vec![person(
            9, "newcomer", b"\x09",
        )])),
    )]);
    let (set, _) = one::<valset::Query>(&frame, valset::PROGRAM);
    let frame = driver.tick(vec![answer(set, &memberships())]);
    assert!(has_text(&frame, "newcomer") && !has_text(&frame, "eddy"));

    // a snapshot keeps the rows and the filter, and the restored view
    // re-reads and re-opens its watch
    let frame = driver.tick(type_into(&frame, "members/filter", "new"));
    let _ = frame;
    let bytes = driver.snapshot().unwrap();
    let mut restored = Driver::<Shell<Members>>::from_snapshot(&bytes, false).unwrap();
    let frame = restored.tick(vec![]);
    assert!(has_text(&frame, "newcomer"));
    assert_eq!(restored.app.state().filter, "new");
    assert_eq!(asked::<identity::Query>(&frame, identity::PROGRAM).len(), 1);
    assert!(
        frame
            .requests
            .iter()
            .any(|request| request.kind == "rpc.live")
    );
}

#[test]
fn the_ready_roster_is_accessible() {
    let (_driver, frame, _) = ready();
    assert_accessible(frame.root.as_ref().expect("a tree"));
}
