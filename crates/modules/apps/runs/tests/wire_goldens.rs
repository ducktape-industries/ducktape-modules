//! Frozen at SDK `b66f47f1f4b0c869786ce195e382f2e83fd15277` before moving
//! `runs-wire` into this crate. JSON fixtures use `sdk::wire`; the assigned
//! event feed and request/query/reply records are not interchangeable with
//! Borsh storage. The SDK36 source tree `8c764093d5974b4328c0b90de157d72597540020`
//! has identical Runs producer blobs; its later `4f8bce7` fixture commit is
//! only provenance for these captured bytes.

use runs::{
    AgentResponse, ReplyBlock, RunEvent, RunFact, RunOutcome, RunRecord, RunsMsg, RunsQuery,
    RunsReply, RunsViewQuery,
};
use std::{collections::BTreeMap, fs};

fn fixture(name: &str) -> Vec<u8> {
    fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn runs_owned_boundary_goldens() {
    // Coverage: one request, query, reply, assigned RunEvent, stored RunRecord
    // nested in a reply, model response, and derived-index query. This is a
    // representative sample, not an assertion over every enum variant.
    let msg = RunsMsg::RequestRun {
        agent_id: "agent-1".into(),
        channel_id: "general".into(),
        anchor_seq: 7,
        demands: BTreeMap::from([(String::from("cpu"), 2)]),
        skills: vec!["review".into()],
    };
    let msg_bytes = fixture("msg_request_run.json");
    assert_eq!(runs::encode_msg(&msg), msg_bytes);
    assert_eq!(runs::decode_msg(&msg_bytes).unwrap(), msg);

    let query = RunsQuery::ConversationEvents {
        conversation_id: "conv-1".into(),
        from: 3,
        limit: 64,
    };
    let query_bytes = fixture("query_conversation_events.json");
    assert_eq!(runs::encode_query(&query), query_bytes);
    assert_eq!(runs::decode_query(&query_bytes).unwrap(), query);

    let reply = RunsReply::NextConversationInputDue(None);
    let reply_bytes = fixture("reply_next_input_none.json");
    assert_eq!(runs::encode_reply(&reply), reply_bytes);
    assert_eq!(runs::decode_reply(&reply_bytes).unwrap(), reply);

    let assigned = vec![RunEvent {
        run_id: "run-1".into(),
        fact: RunFact::Settled {
            outcome: RunOutcome::ActionRejected,
            reason: Some("refused".into()),
            degraded: true,
            executing_node: "node-1".into(),
            output_ref: None,
            pr: None,
        },
    }];
    let assigned_bytes = fixture("assigned_settled_refused.json");
    assert_eq!(runs::encode_assigned(&assigned), assigned_bytes);
    assert_eq!(runs::decode_assigned(&assigned_bytes).unwrap(), assigned);

    let recent = RunsReply::RecentRuns(vec![RunRecord {
        run_id: "run-1".into(),
        agent_id: "agent-1".into(),
        channel_id: "general".into(),
        anchor_seq: 7,
        outcome: RunOutcome::Failed,
        degraded: false,
        created_at: 10,
        delivered_at: 12,
        executing_node: "unknown".into(),
        output_ref: None,
        pr: None,
    }]);
    let recent_bytes = fixture("reply_recent_run.json");
    assert_eq!(runs::encode_reply(&recent), recent_bytes);
    assert_eq!(runs::decode_reply(&recent_bytes).unwrap(), recent);

    let response = AgentResponse {
        reply_blocks: vec![ReplyBlock {
            kind: "paragraph".into(),
            text: "done".into(),
            lang: None,
        }],
        actions: vec![],
        commit_message: None,
    };
    let response_bytes = fixture("model_response.json");
    assert_eq!(runs::encode_response(&response), response_bytes);
    assert_eq!(runs::decode_response(&response_bytes).unwrap(), response);

    let index_query = RunsViewQuery::Recent {
        agent_id: Some("agent-1".into()),
        limit: Some(10),
    };
    let index_bytes = fixture("index_query_recent.json");
    assert_eq!(serde_json::to_vec(&index_query).unwrap(), index_bytes);
    assert!(serde_json::from_slice::<RunsViewQuery>(&index_bytes).is_ok());

    assert!(runs::decode_msg(&fixture("malformed_request.json")).is_err());
}

#[test]
fn owned_helper_pins() {
    assert_eq!(
        runs::MAX_PAGE_TITLE_LEN,
        pages::MAX_PAGE_TITLE_LEN,
        "the catalog publishes this limit before a pages.post write",
    );

    let chain: duck_address::ChainId = "dognet-b5b6ea90".parse().unwrap();
    let run = runs::RunAddress {
        digest: runs::dispatch_id_for("chat\u{1f}general\u{1f}7\u{1f}bot"),
    };
    let printed = run.address(chain).unwrap();
    assert_eq!(runs::RunAddress::try_from(&printed), Ok(run));
}
