//! Golden bytes captured from the pre-move producer at SDK `b66f47f1f4b0c869786ce195e382f2e83fd15277`.
//!
//! Coverage is deliberately representative, not exhaustive: one request, query, reply,
//! event detail, assignment, JSON canonical-store block, JSON index row, and Borsh index
//! feed row. The JSON cases use `sdk::wire`; the index feed case uses `index_guest` Borsh.

use borsh::{from_slice, to_vec};
use index_guest::{OpRow, OriginTag};
use pages::index::PageBlockRow;
use pages::{
    Block, BlockKind, Comment, DiscussionMutation, DiscussionThreadSnapshot,
    ManagedDiscussionSnapshot, NewBlock, PageAssigned, PageBlockPage, PageMsg, PageQuery,
    PageReply, Party, RelativeAnchor, SpanMark, decode_assigned, decode_msg, decode_query,
    decode_reply, encode_assigned, encode_msg, encode_query, encode_reply,
};

fn values() -> (
    PageAssigned,
    Block,
    PageMsg,
    PageQuery,
    PageReply,
    ManagedDiscussionSnapshot,
    PageBlockRow,
    OpRow,
) {
    let assigned = PageAssigned {
        actor: Party::Account(7),
    };
    let block = Block {
        author: Party::Account(7),
        id: "home".into(),
        parent: None,
        page: "home".into(),
        kind: BlockKind::Page,
        text: "Home".into(),
        marks: vec![SpanMark {
            start: 0,
            end: 4,
            kind: pages::InlineMark::Bold,
        }],
        checked: false,
        children: vec!["intro".into()],
    };
    let msg = PageMsg::CreatePage {
        page_id: "home".into(),
        title: "Home".into(),
        blocks: vec![NewBlock {
            id: "intro".into(),
            kind: BlockKind::Paragraph,
            text: "Welcome".into(),
            marks: Vec::new(),
        }],
    };
    let query = PageQuery::GetPage {
        page_id: "home".into(),
        after: Some("intro".into()),
        limit: 8,
    };
    let reply = PageReply::Page(Some(PageBlockPage {
        blocks: vec![block.clone()],
        next_after: Some("home".into()),
    }));
    let event = ManagedDiscussionSnapshot {
        mutation: DiscussionMutation::Created,
        collection_page_id: "home".into(),
        page_id: "home".into(),
        comment: Comment {
            id: "c1".into(),
            thread_id: "t1".into(),
            author: Party::Account(7),
            text: "note".into(),
            mentions: vec![9],
            created_at: 3,
            edited_at: None,
            deleted: false,
        },
        thread: DiscussionThreadSnapshot {
            id: "t1".into(),
            target: "home".into(),
            opener: Party::Account(7),
            created_at: 3,
            anchor: Some(RelativeAnchor { start: 1, end: 3 }),
            resolved: false,
            resolved_by: None,
        },
    };
    let index_row = PageBlockRow {
        author: Party::Account(7),
        block_id: "home".into(),
        page_id: "home".into(),
        parent: None,
        kind: BlockKind::Page,
        text: "Home".into(),
        marks: vec![SpanMark {
            start: 0,
            end: 4,
            kind: pages::InlineMark::Bold,
        }],
        checked: false,
        children: vec!["intro".into()],
        height: 12,
        time: 34,
    };
    let op = OpRow {
        height: 12,
        seq: 2,
        time: 34,
        origin: OriginTag::program(7),
        payload: encode_msg(&msg),
        assigned: encode_assigned(&assigned),
    };
    (assigned, block, msg, query, reply, event, index_row, op)
}

#[test]
fn moved_pages_producer_matches_frozen_b66_bytes() {
    let (assigned, block, msg, query, reply, event, index_row, op) = values();

    let bytes = include_str!("fixtures/pages-wire-b66/assigned.hex");
    let assigned_bytes = decode_hex(bytes);
    assert_eq!(encode_assigned(&assigned), assigned_bytes);
    assert_eq!(decode_assigned(&assigned_bytes).unwrap(), assigned);

    let request_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/request.hex"));
    assert_eq!(encode_msg(&msg), request_bytes);
    assert_eq!(decode_msg(&request_bytes).unwrap(), msg);

    let query_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/query.hex"));
    assert_eq!(encode_query(&query), query_bytes);
    assert_eq!(decode_query(&query_bytes).unwrap(), query);

    let reply_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/reply.hex"));
    assert_eq!(encode_reply(&reply), reply_bytes);
    assert_eq!(decode_reply(&reply_bytes).unwrap(), reply);

    let event_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/event.hex"));
    assert_eq!(sdk::wire::encode(&event), event_bytes);
    assert_eq!(
        sdk::wire::decode::<ManagedDiscussionSnapshot>(&event_bytes).unwrap(),
        event
    );

    let store_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/store-block.hex"));
    assert_eq!(sdk::wire::encode(&block), store_bytes);
    assert_eq!(sdk::wire::decode::<Block>(&store_bytes).unwrap(), block);

    let index_row_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/index-row.hex"));
    assert_eq!(serde_json::to_vec(&index_row).unwrap(), index_row_bytes);
    assert_eq!(
        serde_json::from_slice::<PageBlockRow>(&index_row_bytes).unwrap(),
        index_row
    );

    let index_op_bytes = decode_hex(include_str!("fixtures/pages-wire-b66/index-op.borsh.hex"));
    assert_eq!(to_vec(&op).unwrap(), index_op_bytes);
    assert_eq!(from_slice::<OpRow>(&index_op_bytes).unwrap(), op);

    let malformed = decode_hex(include_str!(
        "fixtures/pages-wire-b66/malformed-request.hex"
    ));
    assert!(decode_msg(&malformed).is_err());
}

fn decode_hex(text: &str) -> Vec<u8> {
    text.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
