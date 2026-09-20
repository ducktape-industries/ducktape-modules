//! Frozen compatibility fixture for the original six-field `__state` encoding.
mod support;

use futures::executor::block_on;
use sdk::{Ctx, Env, Error, Event, Module, Msg, Origin, StateRoot};
use std::{fs, path::PathBuf};
use support::{module, source};

struct NoopCtx {
    env: Env,
    messages: Vec<Msg>,
}

impl NoopCtx {
    fn external() -> Self {
        Self {
            env: Env {
                height: 1,
                consensus_time: 1,
                origin: Origin::External(vec![1; 32]),
                me: "runs".into(),
                cause: sdk::Cause::Direct,
            },
            messages: Vec::new(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Ctx for NoopCtx {
    fn env(&self) -> &Env {
        &self.env
    }

    fn module_root(&self, _: &str) -> Option<StateRoot> {
        None
    }

    async fn query(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, Error> {
        Err(Error::QueryUnsupported)
    }

    fn emit_msg(&mut self, msg: Msg) {
        self.messages.push(msg);
    }

    fn emit_event(&mut self, _: Event) {}
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
fn the_frozen_v0_snapshot_installs_and_answers_every_query() {
    block_on(async {
        let bytes = fs::read(fixture("state_v0.bin")).unwrap();
        let root = read_root();
        let (_, _, network) = source().await;
        let mut restored = module();

        restored.install(&bytes, root).unwrap();
        assert_eq!(restored.snapshot(), bytes);
        assert_eq!(restored.root(), root);
        let pending_items = restored.pending_items().await.unwrap();

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

        let mut before = Vec::new();
        for query in [
            runs::encode_query(&runs::RunsQuery::PendingRuns),
            runs::encode_query(&runs::RunsQuery::AgentSessions),
            runs::encode_query(&runs::RunsQuery::Model {
                query: runs::ModelQuery::Agents,
            }),
        ] {
            before.push(restored.query(&query).await.unwrap());
        }
        let mut ctx = NoopCtx::external();
        restored
            .execute(
                &mut ctx,
                &Msg {
                    target: "runs".into(),
                    payload: runs::encode_msg(&runs::RunsMsg::EnableJobWorker { enabled: true }),
                },
            )
            .await
            .unwrap();
        restored.abort_block().await.unwrap();
        assert_eq!(restored.snapshot(), bytes);
        assert_eq!(restored.root(), root);
        assert_eq!(restored.pending_items().await.unwrap(), pending_items);

        restored
            .execute(
                &mut ctx,
                &Msg {
                    target: "runs".into(),
                    payload: runs::encode_msg(&runs::RunsMsg::EnableJobWorker { enabled: true }),
                },
            )
            .await
            .unwrap();
        restored.commit_block().await.unwrap();
        assert_ne!(restored.snapshot(), bytes);
        let mut after = Vec::new();
        for query in [
            runs::encode_query(&runs::RunsQuery::PendingRuns),
            runs::encode_query(&runs::RunsQuery::AgentSessions),
            runs::encode_query(&runs::RunsQuery::Model {
                query: runs::ModelQuery::Agents,
            }),
        ] {
            after.push(restored.query(&query).await.unwrap());
        }
        assert_eq!(before, after);
        let agent = runs::encode_query(&runs::RunsQuery::Model {
            query: runs::ModelQuery::Agent {
                agent_id: "builder".into(),
            },
        });
        assert!(matches!(
            runs::decode_reply(&restored.query(&agent).await.unwrap()).unwrap(),
            runs::RunsReply::Model(runs::ModelReply::Agent(Some(_)))
        ));
    });
}
