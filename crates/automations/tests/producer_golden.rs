//! Golden producer proof. Fixtures were captured before this relocation from
//! SDK `b66f47f1f4b0c869786ce195e382f2e83fd15277`'s `automations-wire`:
//! JSON uses `sdk::wire`; Rule/RunRecord storage uses canonical Borsh.
use automations::{
    Action, AutomationsMsg, AutomationsQuery, AutomationsReply, Rule, RunRecord, Trigger,
    decode_msg, decode_query, decode_reply, encode_msg, encode_query, encode_reply,
};
use borsh::{from_slice, to_vec};

fn fixture(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

fn rule() -> Rule {
    Rule {
        rule_id: "r1".into(),
        owner: 7,
        enabled: true,
        trigger: Trigger {
            channel_id: Some("general".into()),
            mention: Some("acct:7".into()),
            text_contains: Some("hello".into()),
        },
        action: Action::PostMessage {
            channel_id: "general".into(),
            template: "reply {text}".into(),
        },
        created_at: 11,
        fire_count: 2,
    }
}

fn run() -> RunRecord {
    RunRecord {
        rule_id: "r1".into(),
        channel_id: "general".into(),
        seq: 3,
        height: 9,
        action_ok: true,
        detail: "posted".into(),
    }
}

#[test]
fn producer_encodings_match_frozen_bytes() {
    let rule = rule();
    let run = run();
    let msg = AutomationsMsg::CreateRule {
        rule_id: "r1".into(),
        trigger: rule.trigger.clone(),
        action: rule.action.clone(),
    };
    let query = AutomationsQuery::RunHistory {
        rule_id: "r1".into(),
        limit: 4,
    };
    let reply = AutomationsReply::History(vec![run.clone()]);
    let msg_bytes = fixture(include_str!("fixtures/msg.hex"));
    let query_bytes = fixture(include_str!("fixtures/query.hex"));
    let reply_bytes = fixture(include_str!("fixtures/reply.hex"));
    let rule_bytes = fixture(include_str!("fixtures/rule-borsh.hex"));
    let run_bytes = fixture(include_str!("fixtures/run-borsh.hex"));

    assert_eq!(encode_msg(&msg), msg_bytes);
    assert_eq!(decode_msg(&msg_bytes).unwrap(), msg);
    assert_eq!(encode_query(&query), query_bytes);
    assert_eq!(decode_query(&query_bytes).unwrap(), query);
    assert_eq!(encode_reply(&reply), reply_bytes);
    assert_eq!(decode_reply(&reply_bytes).unwrap(), reply);
    assert_eq!(to_vec(&rule).unwrap(), rule_bytes);
    assert_eq!(from_slice::<Rule>(&rule_bytes).unwrap(), rule);
    assert_eq!(to_vec(&run).unwrap(), run_bytes);
    assert_eq!(from_slice::<RunRecord>(&run_bytes).unwrap(), run);
}

#[test]
fn producer_rejects_malformed_fixture() {
    let malformed = fixture(include_str!("fixtures/malformed.hex"));
    assert!(decode_msg(&malformed).is_err());
    assert!(decode_query(&malformed).is_err());
    assert!(decode_reply(&malformed).is_err());
}
