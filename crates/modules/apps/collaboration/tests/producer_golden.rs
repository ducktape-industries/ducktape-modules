//! Golden producer proof. Fixtures were captured before this relocation from
//! SDK `b66f47f1f4b0c869786ce195e382f2e83fd15277`'s `collaboration-wire`;
//! every direction uses the canonical `sdk::wire` JSON codec.
use collaboration::Party;
use collaboration::{
    CollaborationAssigned, CollaborationEvent, CollaborationMsg, CollaborationQuery,
    CollaborationReply, DeliverRequest, DenyReason, MessageKind, ProtectedRead, Request,
    decode_assigned, decode_event, decode_msg, decode_query, decode_reply, encode_assigned,
    encode_event, encode_msg, encode_query, encode_reply,
};

fn fixture(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

#[test]
fn producer_encodings_match_frozen_bytes() {
    let recipient = Party::Account(7);
    let msg = Request::new(
        "dognet",
        CollaborationMsg::Deliver(DeliverRequest {
            channel_id: "general".into(),
            message_id: "m1".into(),
            recipient: recipient.clone(),
            kind: MessageKind::Notice,
            task: None,
            references: Vec::new(),
            expires_at: 42,
        }),
    );
    let query = CollaborationQuery::Read {
        participant: recipient.clone(),
        via: None,
        read: ProtectedRead::Mailbox,
    };
    let reply = CollaborationReply::Denied(DenyReason::Unauthenticated);
    let event = CollaborationEvent::ChannelAdvanced {
        channel_id: "general".into(),
        seq: 3,
    };
    let assigned = CollaborationAssigned::Advanced {
        channel_id: "general".into(),
        seq: 3,
        actor: recipient,
    };
    let msg_bytes = fixture(include_str!("fixtures/msg.hex"));
    let query_bytes = fixture(include_str!("fixtures/query.hex"));
    let reply_bytes = fixture(include_str!("fixtures/reply.hex"));
    let event_bytes = fixture(include_str!("fixtures/event.hex"));
    let assigned_bytes = fixture(include_str!("fixtures/assigned.hex"));

    assert_eq!(encode_msg(&msg), msg_bytes);
    assert_eq!(decode_msg(&msg_bytes).unwrap(), msg);
    assert_eq!(encode_query(&query), query_bytes);
    assert_eq!(decode_query(&query_bytes).unwrap(), query);
    assert_eq!(encode_reply(&reply), reply_bytes);
    assert_eq!(decode_reply(&reply_bytes).unwrap(), reply);
    assert_eq!(encode_event(&event), event_bytes);
    assert_eq!(decode_event(&event_bytes).unwrap(), event);
    assert_eq!(encode_assigned(&assigned), assigned_bytes);
    assert_eq!(decode_assigned(&assigned_bytes).unwrap(), assigned);
}

#[test]
fn producer_rejects_malformed_fixture() {
    let malformed = fixture(include_str!("fixtures/malformed.hex"));
    assert!(decode_msg(&malformed).is_err());
    assert!(decode_query(&malformed).is_err());
    assert!(decode_reply(&malformed).is_err());
    assert!(decode_event(&malformed).is_err());
    assert!(decode_assigned(&malformed).is_err());
}
