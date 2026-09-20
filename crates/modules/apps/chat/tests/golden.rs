//! Frozen codec proof: source SDK b66, `sdk::wire` serde-JSON plus the
//! module's serde-JSON store and Borsh composite-key codecs.

use chat::{
    Channel, ChatAssigned, ChatEvent, ChatMsg, ChatQuery, ChatReply, Party, PostPolicy,
    decode_assigned, decode_event, decode_msg, decode_query, decode_reply, encode_assigned,
    encode_event, encode_msg, encode_query, encode_reply,
};

fn fixture(name: &str) -> Vec<u8> {
    let text = match name {
        "request-create-channel" => include_str!("fixtures/request-create-channel.hex"),
        "query-messages-range" => include_str!("fixtures/query-messages-range.hex"),
        "reply-channel-none" => include_str!("fixtures/reply-channel-none.hex"),
        "event-message-posted" => include_str!("fixtures/event-message-posted.hex"),
        "assigned-posted" => include_str!("fixtures/assigned-posted.hex"),
        "store-channel-json" => include_str!("fixtures/store-channel-json.hex"),
        "party-account-borsh" => include_str!("fixtures/party-account-borsh.hex"),
        "malformed-request-trailing-byte" => {
            include_str!("fixtures/malformed-request-trailing-byte.hex")
        }
        _ => panic!("unknown fixture {name}"),
    };
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn frozen_b66_wire_bytes_match_and_decode() {
    let request = ChatMsg::CreateChannel {
        channel_id: "general".into(),
        name: "General".into(),
        post_policy: PostPolicy::Open,
    };
    assert_eq!(encode_msg(&request), fixture("request-create-channel"));
    assert_eq!(
        decode_msg(&fixture("request-create-channel")).unwrap(),
        request
    );

    let query = ChatQuery::MessagesRange {
        channel_id: "general".into(),
        from_seq: 1,
        limit: 20,
    };
    assert_eq!(encode_query(&query), fixture("query-messages-range"));
    assert_eq!(
        decode_query(&fixture("query-messages-range")).unwrap(),
        query
    );

    let reply = ChatReply::Channel(None);
    assert_eq!(encode_reply(&reply), fixture("reply-channel-none"));
    assert_eq!(decode_reply(&fixture("reply-channel-none")).unwrap(), reply);

    let event = ChatEvent::MessagePosted {
        channel_id: "general".into(),
        seq: 3,
        thread_root: None,
        author: Party::Account(7),
        mentions: vec![11],
    };
    assert_eq!(encode_event(&event), fixture("event-message-posted"));
    assert_eq!(
        decode_event(&fixture("event-message-posted")).unwrap(),
        event
    );

    let assigned = ChatAssigned::Posted {
        seq: 3,
        actor: Party::Account(7),
        key_mentions: vec![11],
    };
    assert_eq!(encode_assigned(&assigned), fixture("assigned-posted"));
    assert_eq!(
        decode_assigned(&fixture("assigned-posted")).unwrap(),
        assigned
    );
}

#[test]
fn frozen_b66_store_codecs_match() {
    let channel = Channel {
        id: "general".into(),
        name: "General".into(),
        created_at: 42,
        head_seq: 3,
        post_policy: PostPolicy::Open,
        hooks: vec!["runs".into()],
        pinned: vec![2],
        voice: false,
        owner: Party::Account(7),
        archived: false,
        revision: 1,
    };
    let store_bytes = serde_json::to_vec(&channel).unwrap();
    assert_eq!(store_bytes, fixture("store-channel-json"));
    assert_eq!(
        serde_json::from_slice::<Channel>(&fixture("store-channel-json")).unwrap(),
        channel
    );
    assert_eq!(
        borsh::to_vec(&Party::Account(7)).unwrap(),
        fixture("party-account-borsh")
    );
}

#[test]
fn frozen_malformed_request_is_rejected() {
    assert!(decode_msg(&fixture("malformed-request-trailing-byte")).is_err());
}
