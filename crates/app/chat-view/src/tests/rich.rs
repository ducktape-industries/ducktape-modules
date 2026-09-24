use super::*;

#[test]
fn rich_message_keeps_styles_and_dispatches_each_link_by_value() {
    let (mut cx, view) = opened();
    let spans = vec![
        chat::Span {
            text: "bold ".into(),
            marks: vec![chat::Mark::Bold],
        },
        chat::Span {
            text: "italic ".into(),
            marks: vec![chat::Mark::Italic],
        },
        chat::Span {
            text: "first ".into(),
            marks: vec![chat::Mark::Link("https://one.example".into())],
        },
        chat::Span {
            text: "reviewer".into(),
            marks: vec![chat::Mark::Mention(chat::Party::Account(8))],
        },
    ];
    view.update(&mut cx, |chat, _, cx| {
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(MsgRow {
                channel_id: "general".into(),
                seq: 3,
                message_id: "rich".into(),
                author: "acct:7".into(),
                height: 2,
                blocks: vec![
                    chat::Block::Paragraph(spans),
                    chat::Block::Code {
                        lang: Some("rust".into()),
                        text: "fn main() {}".into(),
                    },
                    chat::Block::Quote(vec![chat::Span {
                        text: "quoted".into(),
                        marks: vec![chat::Mark::Italic],
                    }]),
                ],
                text: "rich".into(),
                ..MsgRow::default()
            });
        cx.notify();
    });
    cx.run_until_parked();
    let key = "chat-message-rich-block-0";
    let Some(wire::Node::RichText {
        text,
        runs,
        clickable_ranges,
        ..
    }) = cx.find(key)
    else {
        panic!("message paragraph is one rich text node");
    };
    assert_eq!(text, "bold italic first @reviewer");
    let wire::RichTextRuns::Highlights(highlights) = runs else {
        panic!("chat authors highlight ranges");
    };
    assert_eq!(highlights.len(), 4);
    assert!(highlights[0].1.font_weight.is_some());
    assert!(highlights[1].1.font_style.is_some());
    assert_eq!(clickable_ranges.len(), 2);
    assert!(cx.has_text("rust"));
    assert!(cx.has_text("fn main() {}"));
    assert!(cx.has_text("quoted"));
    let seq = view.read(|chat| {
        chat.rows(Pane::Timeline)
            .iter()
            .find(|row| row.message_id == "rich")
            .map(|row| row.seq)
            .expect("the rich row")
    });
    super::message::hover(&mut cx, &view, seq);
    assert!(matches!(
        cx.find("chat-message-rich-more"),
        Some(wire::Node::Container (ducktape_view_guest::wire::ContainerNode { interactivity, .. }))
            if interactivity.aria.label.as_deref() == Some("More message actions")
    ));

    cx.simulate_rich_click(key, 0);
    cx.simulate_rich_click(key, 1);
    assert_eq!(
        cx.host().opened_links(),
        vec![
            "duck://testnet-0a1b2c3d/chat/general",
            "https://one.example",
            "duck://testnet-0a1b2c3d/identity/8",
        ]
    );
}

#[test]
fn a_header_keeps_its_grouped_block_number() {
    let (mut cx, view) = opened();
    view.update(&mut cx, |chat, _, cx| {
        // Start a new author run so the header shows its block number.
        let mut late = row(3, "acct:7", "late");
        late.message_id = "late".into();
        late.height = 12_345;
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(late);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("block 12,345"));
}
