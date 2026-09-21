//! Frozen producer bytes captured before the move from SDK b66.

use agent::{
    AgentAssigned, AgentMsg, AgentQuery, AgentReply, InvocationPage, Program, Step,
    decode_assigned, decode_msg, decode_query, decode_reply, encode_assigned, encode_msg,
    encode_query, encode_reply,
};

fn bytes(hex: &str) -> Vec<u8> {
    hex.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// Source for the unchanged fixtures: ducktape-sdk b66f47f1f4b0c869786ce195e382f2e83fd15277.
// JSON directions use sdk::wire; the persisted program uses Borsh. SDK36 changed
// AgentReply::Invocations to InvocationPage, so the old b66 reply bytes are not
// used as proof of the new page encoding.
#[test]
fn b66_producer_encodings_are_immutable_and_decode() {
    let program = Program {
        steps: vec![Step::Finish],
    };
    let msg = AgentMsg::Provision {
        request_id: "req-1".into(),
        name: "bot".into(),
        program: program.clone(),
    };
    let query = AgentQuery::Binding { account: 7 };
    let reply = AgentReply::Invocations(InvocationPage {
        entries: Vec::new(),
        has_more: false,
        next_after: None,
    });
    let assigned = AgentAssigned::Provisioned { account: 7 };
    let msg_bytes = bytes(include_str!("fixtures/request.json.hex"));
    let query_bytes = bytes(include_str!("fixtures/query.json.hex"));
    let assigned_bytes = bytes(include_str!("fixtures/assigned.json.hex"));
    assert_eq!(encode_msg(&msg), msg_bytes);
    assert_eq!(encode_query(&query), query_bytes);
    assert_eq!(encode_assigned(&assigned), assigned_bytes);
    assert_eq!(decode_msg(&msg_bytes).unwrap(), msg);
    assert_eq!(decode_query(&query_bytes).unwrap(), query);
    assert_eq!(decode_reply(&encode_reply(&reply)).unwrap(), reply);
    assert_eq!(decode_assigned(&assigned_bytes).unwrap(), assigned);
    assert_eq!(
        borsh::to_vec(&program).unwrap(),
        bytes(include_str!("fixtures/program.borsh.hex"))
    );
    assert!(decode_msg(&msg_bytes[..msg_bytes.len() - 1]).is_err());
}
