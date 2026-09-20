//! Frozen producer bytes captured before the move from SDK b66.

use boards::{Board, Change, Kind, Operation, Query, Reply, Shape};

fn bytes(hex: &str) -> Vec<u8> {
    hex.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// Source: ducktape-sdk b66f47f1f4b0c869786ce195e382f2e83fd15277.
// All directions use sdk::wire JSON; board.json.hex is stored JSON.
#[test]
fn b66_producer_encodings_are_immutable_and_decode() {
    let operation = Operation::Edit {
        board: "main".into(),
        change: Change::Create {
            id: "card".into(),
            shape: Shape::default(),
        },
    };
    let query = Query::Get { id: "main".into() };
    let board = Board::new("Main".into(), "owner".into()).unwrap();
    let reply = Reply::Board(Some(board.clone()));
    let operation_bytes = bytes(include_str!("fixtures/request.json.hex"));
    let query_bytes = bytes(include_str!("fixtures/query.json.hex"));
    let reply_bytes = bytes(include_str!("fixtures/reply.json.hex"));
    assert_eq!(sdk::wire::encode(&operation), operation_bytes);
    assert_eq!(sdk::wire::encode(&query), query_bytes);
    assert_eq!(sdk::wire::encode(&reply), reply_bytes);
    assert_eq!(
        sdk::wire::decode::<Operation>(&operation_bytes).unwrap(),
        operation
    );
    assert_eq!(sdk::wire::decode::<Query>(&query_bytes).unwrap(), query);
    assert_eq!(sdk::wire::decode::<Reply>(&reply_bytes).unwrap(), reply);
    assert_eq!(
        serde_json::to_vec(&board).unwrap(),
        bytes(include_str!("fixtures/board.json.hex"))
    );
    assert!(sdk::wire::decode::<Operation>(&operation_bytes[..operation_bytes.len() - 1]).is_err());
    assert_eq!(Kind::Note, Shape::default().kind);
}
