//! Frozen compatibility fixture for the original six-field `__state` encoding.
mod support;

use futures::executor::block_on;
use sdk::{Module, StateRoot};
use std::{ffi::OsStr, fs, path::PathBuf};
use support::{module, source};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn root_hex(root: StateRoot) -> String {
    root.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_root() -> StateRoot {
    let hex = fs::read_to_string(fixture("state_v0.root")).unwrap();
    let hex = hex.trim();
    assert_eq!(hex.len(), 64, "state_v0.root must contain 32 hex bytes");
    let mut root = [0; 32];
    for (byte, pair) in root.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    StateRoot(root)
}

#[test]
fn the_frozen_v0_snapshot_is_what_this_source_encodes() {
    block_on(async {
        let (bytes, root, _) = source().await;
        let (second_bytes, second_root, _) = source().await;
        assert_eq!(bytes, second_bytes, "snapshot bytes must be deterministic");
        assert_eq!(root, second_root, "snapshot root must be deterministic");

        if std::env::var_os("RUNS_WRITE_STATE_V0").as_deref() == Some(OsStr::new("1")) {
            fs::write(fixture("state_v0.bin"), &bytes).unwrap();
            fs::write(fixture("state_v0.root"), format!("{}\n", root_hex(root))).unwrap();
        } else {
            assert_eq!(bytes, fs::read(fixture("state_v0.bin")).unwrap());
            assert_eq!(
                root_hex(root),
                fs::read_to_string(fixture("state_v0.root")).unwrap().trim()
            );
        }
    });
}

#[test]
fn the_frozen_v0_snapshot_installs_and_answers_every_query() {
    block_on(async {
        let bytes = fs::read(fixture("state_v0.bin")).unwrap();
        let root = read_root();
        let (_, _, network) = source().await;
        let mut restored = module();

        restored.install(&bytes, root).unwrap();
        assert_eq!(restored.snapshot(), bytes);
        assert_eq!(restored.root(), root);

        for query in [
            runs::RunsQuery::PendingRuns,
            runs::RunsQuery::AgentSessions,
            runs::RunsQuery::Model {
                query: runs::ModelQuery::Agents,
            },
        ] {
            let query = runs::encode_query(&query);
            let restored_reply = restored.query(&query).await.unwrap();
            assert_eq!(
                restored_reply,
                network.host.query("runs", &query).await.unwrap()
            );
            match runs::decode_reply(&restored_reply).unwrap() {
                runs::RunsReply::PendingRuns(items) => assert!(!items.is_empty()),
                runs::RunsReply::AgentSessions(items) => assert!(!items.is_empty()),
                runs::RunsReply::Model(runs::ModelReply::Agents(items)) => {
                    assert!(!items.is_empty())
                }
                reply => panic!("unexpected reply: {reply:?}"),
            }
        }
    });
}
