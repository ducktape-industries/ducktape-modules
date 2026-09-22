//! The mirrored wire, pinned to the program's own bytes.
//!
//! `src/contract.rs` is a copy of forge's borsh contract, because a view may
//! not link a program crate. This replays **every** committed fixture — the
//! exact `Respond`/`Output` bytes and the exact request the harness captured
//! — through the copy and requires a byte-identical re-encode in both
//! directions. A field added, reordered or retyped upstream fails here.
use crate::contract::{Op, OpReply, Query, Reply};

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../forge-harness/fixtures")
}

fn sidecars() -> Vec<(String, serde_json::Value)> {
    let mut all: Vec<(String, serde_json::Value)> = std::fs::read_dir(fixtures())
        .expect("the fixture directory is beside this crate")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_owned();
            let text = std::fs::read(&path).ok()?;
            Some((name, serde_json::from_slice(&text).ok()?))
        })
        .collect();
    all.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(all.len() > 60, "every fixture sidecar is read");
    all
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "a borsh hex has whole bytes");
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}

/// Re-encoding what the program said must give back what it said.
fn round_trip<T: borsh::BorshDeserialize + borsh::BorshSerialize>(name: &str, bytes: &[u8]) {
    let value: T = borsh::from_slice(bytes)
        .unwrap_or_else(|error| panic!("{name}: the mirrored wire cannot read it: {error}"));
    assert_eq!(
        borsh::to_vec(&value).expect("borsh encodes"),
        bytes,
        "{name}: the mirrored wire re-encodes differently"
    );
}

#[test]
fn every_committed_reply_and_receipt_round_trips_through_the_mirror() {
    let mut replies = 0;
    let mut receipts = 0;
    for (name, sidecar) in sidecars() {
        if sidecar["codec"].as_str() != Some("borsh") {
            continue;
        }
        let bytes = crate::tests::bytes(&name);
        assert_eq!(
            bytes.len() as u64,
            sidecar["bytes"].as_u64().expect("a recorded length"),
            "{name}: the sidecar and the bytes disagree"
        );
        if name.starts_with("op-") {
            round_trip::<OpReply>(&name, &bytes);
            receipts += 1;
        } else {
            round_trip::<Reply>(&name, &bytes);
            replies += 1;
        }
    }
    assert!(replies > 40 && receipts >= 8, "{replies} replies, {receipts} receipts");
}

#[test]
fn every_committed_request_round_trips_through_the_mirror() {
    let mut queries = 0;
    let mut ops = 0;
    for (name, sidecar) in sidecars() {
        if sidecar["codec"].as_str() != Some("borsh") {
            continue;
        }
        let Some(hex) = sidecar["request_borsh_hex"].as_str() else {
            panic!("{name}: no recorded request");
        };
        let bytes = unhex(hex);
        if name.starts_with("op-") {
            round_trip::<Op>(&name, &bytes);
            ops += 1;
        } else {
            round_trip::<Query>(&name, &bytes);
            queries += 1;
        }
    }
    assert!(queries > 40 && ops >= 8, "{queries} queries, {ops} ops");
}

/// The three git-protocol fixtures are not this contract and must not be
/// decoded as one: they carry Git's own framing.
#[test]
fn the_git_protocol_fixtures_stay_outside_the_borsh_contract() {
    let git: Vec<String> = sidecars()
        .into_iter()
        .filter(|(_, sidecar)| sidecar["codec"].as_str() == Some("git"))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        git,
        ["advertise-receive", "advertise-upload", "upload-refs"],
        "the smart-HTTP fixtures are the only non-borsh ones"
    );
    for name in git {
        assert!(
            borsh::from_slice::<Reply>(&crate::tests::bytes(&name)).is_err()
                || borsh::to_vec(&borsh::from_slice::<Reply>(&crate::tests::bytes(&name)).unwrap())
                    .unwrap()
                    != crate::tests::bytes(&name),
            "{name} is not a UI reply"
        );
    }
}
