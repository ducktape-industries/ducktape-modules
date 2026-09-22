use super::*;

#[test]
fn attachment_preview_keeps_host_surfaces_and_markdown_link_events() {
    let (mut cx, view) = opened();
    let link = files::file_address("testnet#0a1b2c3d", "/readme.md").unwrap();
    view.update(&mut cx, |chat, _, cx| {
        chat.preview = Some(Preview {
            link,
            read: Loaded::Ready(files::Preview {
                text: "[Open](https://example.test)".into(),
                clipped: false,
                binary: false,
            }),
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(matches!(
        cx.find("chat-preview-markdown"),
        Some(wire::Node::Surface {
            name,
            on_event: Some(_),
            ..
        }) if name == "markdown"
    ));
    cx.simulate_surface(
        "chat-preview-markdown",
        wire::SurfaceValue::Str("https://example.test".into()),
    );
    assert_eq!(
        cx.host().opened_links(),
        vec![
            "duck://testnet-0a1b2c3d/chat/general",
            "https://example.test"
        ],
        "choosing the room informs the host before the preview link opens"
    );
}

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
    assert!(matches!(
        cx.find("chat-message-rich-more"),
        Some(wire::Node::Container { interactivity, .. })
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
fn picture_attachment_keeps_scoped_surface_and_respects_attachment_gate() {
    let (mut cx, view) = opened();
    let link = files::file_address("testnet#0a1b2c3d", "/shared/attachments/picture.png").unwrap();
    view.update(&mut cx, |chat, _, cx| {
        chat.pictures.insert(link.clone(), (640, 480));
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(MsgRow {
                channel_id: "general".into(),
                seq: 3,
                message_id: "picture".into(),
                author: "acct:7".into(),
                height: 2,
                blocks: vec![chat::Block::Paragraph(vec![chat::Span {
                    text: "picture.png".into(),
                    marks: vec![chat::Mark::Link(link.clone())],
                }])],
                text: "picture.png".into(),
                ..MsgRow::default()
            });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(matches!(
        cx.find("chat-message-picture-block-0-picture"),
        Some(wire::Node::Surface { name, args, .. })
            if name == "picture"
                && args.get(1) == Some(&wire::SurfaceValue::Str(
                    files::attachment_file_path(&link)
                ))
    ));
    cx.simulate_click("chat-message-picture-block-0");
    view.read(|chat| {
        assert!(
            chat.preview.is_none(),
            "attachment previews remain disabled"
        )
    });
    assert_eq!(
        cx.host().opened_links(),
        vec!["duck://testnet-0a1b2c3d/chat/general".to_owned(), link]
    );
}

#[test]
fn file_attachment_keeps_type_caption_and_grouped_block_number() {
    let (mut cx, view) = opened();
    let link = files::file_address("testnet#0a1b2c3d", "/shared/attachments/deck.pdf").unwrap();
    view.update(&mut cx, |chat, _, cx| {
        let mut file = row(3, "acct:8", "deck.pdf");
        file.message_id = "file".into();
        file.height = 12_345;
        file.blocks = vec![chat::Block::Paragraph(vec![chat::Span {
            text: "deck.pdf".into(),
            marks: vec![chat::Mark::Link(link)],
        }])];
        chat.room
            .as_mut()
            .unwrap()
            .messages
            .ready_mut()
            .unwrap()
            .push(file);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("block 12,345"));
    assert!(cx.has_text("PDF file"));
    assert!(matches!(cx.find("chat-message-file-block-0"),
        Some(wire::Node::Container { interactivity, .. })
            if interactivity.aria.label.as_deref() == Some("Open deck.pdf")));
}
