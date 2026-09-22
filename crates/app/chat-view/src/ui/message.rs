//! One message: the avatar rail, the byline, the body's blocks (rich text,
//! code, quotes, attachments), its reactions and the way into its thread.
use ducktape_view_guest::Context;
use ducktape_view_guest::wire::{self, ButtonPreset, Length, Node, kit, kit::Tone};

use crate::client::{ChatBlock, ChatMessage, SpanStyle, height_label};
use crate::{Chat, Mode, Pane};
use ducktape_view_guest::wire::kit::*;

/// The avatar plate beside an author's first message, and the gap to the
/// text. A continuation row keeps the same rail so bodies line up.
pub const AVATAR: f32 = 28.;
pub const RAIL_GAP: f32 = kit::spacing::MD as f32;
/// Where a message's text starts, from the row's left edge.
const PILL_HEIGHT: f32 = 24.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Plate {
    Plain,
    Selected,
    Ranged,
}

/// The card: the rail, then the byline and body. A chosen or ranged row
/// wears a wash.
pub fn card(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    plate: Plate,
    cx: &mut Context<Chat>,
) -> Node {
    let key = format!("message/{pane:?}/{}", message.id);
    let p = kit::palette();
    let rail = if message.show_author {
        avatar(format!("{key}/avatar"), &message.initial, message.agent)
    } else {
        kit::space(Some(Length::Fixed(AVATAR)), Some(Length::Fixed(4.)))
    };
    let contents = contents(chat, &key, message, pane, plate, cx);
    let row = aligned_x(
        kit::padded(
            kit::spaced(kit::row(format!("{key}/row"), [rail, contents]), RAIL_GAP),
            wire::Edges {
                top: if message.show_author {
                    kit::spacing::MD as f32
                } else {
                    3.
                },
                right: 16.,
                bottom: 3.,
                left: 16.,
            },
        ),
        wire::AlignX::Left,
    );
    let card = rounded(kit::container(key, row), kit::radius::CONTROL as f32);
    match plate {
        Plate::Plain => card,
        Plate::Selected => background(card, p.accent_soft),
        Plate::Ranged => background(card, p.surface_raised),
    }
}

fn contents(
    chat: &Chat,
    key: &str,
    message: &ChatMessage,
    pane: Pane,
    plate: Plate,
    cx: &mut Context<Chat>,
) -> Node {
    let key = format!("{key}/contents");
    let p = kit::palette();
    let mut children = Vec::new();
    if message.show_author {
        let mut header = vec![kit::nowrap(kit::strong(
            format!("{key}/author"),
            &message.author,
        ))];
        if message.agent {
            header.push(kit::badge(format!("{key}/agent"), "Agent", Tone::Agent));
        }
        if message.height > 0 {
            header.push(kit::nowrap(kit::colored(
                kit::text_size(
                    kit::mono(format!("{key}/height"), height_label(message.height)),
                    kit::type_scale::CAPTION as f32,
                ),
                p.faint,
            )));
        }
        children.push(kit::spaced(
            kit::centered_row(format!("{key}/header"), header),
            kit::spacing::XS as f32,
        ));
    }
    // a press chooses the message (shift grows the copy range)
    let seq = message.seq;
    let press = cx.listener(move |chat, _event: &(), _window, cx| {
        cx.notify();
        chat.press_message(pane, seq)
    });
    let on_link = cx.listener(|chat, event: &String, _window, cx| {
        let link = event.clone();
        cx.notify();
        chat.open_link(link, cx)
    });
    children.push(with_press(
        with_row_role(
            mouse_area(
                format!("{key}/select"),
                body(
                    chat,
                    format!("{key}/body"),
                    &message.blocks,
                    Some(on_link),
                    cx,
                ),
            ),
            format!(
                "Select message, shows its actions: {}: {}",
                message.author, message.body
            ),
            plate != Plate::Plain,
        ),
        press,
    ));
    if message.edited {
        children.push(kit::caption(format!("{key}/edited"), "edited"));
    }
    let writable = chat.may_write();
    let mut reactions = Vec::new();
    for reaction in &message.reactions {
        let (emoji, mine) = (reaction.emoji.clone(), reaction.reacted_by_me);
        let press = writable.then(|| {
            cx.listener(move |chat, _event: &(), _window, cx| {
                cx.notify();
                chat.react(seq, emoji.clone(), !mine, cx)
            })
        });
        reactions.push(pill(
            format!("{key}/reaction/{}", reaction.emoji),
            &reaction.emoji,
            Some(reaction.count),
            if mine {
                "Remove reaction"
            } else {
                "Add reaction"
            },
            mine,
            press,
        ));
    }
    if !reactions.is_empty() {
        let rev = message.rev;
        let open = writable.then(|| {
            cx.listener(move |chat, _event: &(), window, cx| {
                cx.notify();
                chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
            })
        });
        reactions.push(pill(
            format!("{key}/reaction/add"),
            "+",
            None,
            "Add reaction",
            false,
            open,
        ));
        children.push(kit::spaced(
            kit::wrapped_row(format!("{key}/reactions"), reactions),
            kit::spacing::XXS as f32,
        ));
    }
    // in the timeline the count is the way into the thread; in the thread
    // itself it is the rule between the root and its replies
    match (message.reply_count > 0, pane) {
        (false, _) => {}
        (true, Pane::Timeline) => {
            let open = cx.listener(move |chat, _event: &(), _window, cx| {
                cx.notify();
                chat.open_thread(seq, cx)
            });
            children.push(reply_link(
                format!("{key}/thread"),
                message.reply_count,
                open,
            ));
        }
        (true, Pane::Thread) => children.push(kit::spaced(
            kit::centered_row(
                format!("{key}/thread"),
                [
                    kit::nowrap(kit::caption(
                        format!("{key}/thread/count"),
                        plural(message.reply_count, "reply", "replies"),
                    )),
                    kit::container(
                        format!("{key}/thread/line"),
                        kit::divider(format!("{key}/thread/rule")),
                    ),
                ],
            ),
            kit::spacing::SM as f32,
        )),
    }
    if message.pending {
        children.push(kit::caption(format!("{key}/pending"), &message.meta));
    }
    kit::spaced(kit::column(key, children), 3.)
}

pub fn body(
    chat: &Chat,
    key: String,
    blocks: &[ChatBlock],
    on_link: Option<u32>,
    cx: &mut Context<Chat>,
) -> Node {
    let p = kit::palette();
    let mut children = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let scope = format!("{key}/block/{index}");
        let content = match block.kind.as_str() {
            "divider" => kit::divider(scope),
            "attachment" => match chat.pictures.get(&block.link) {
                Some(&(w, h)) if w > 0 && h > 0 => picture(scope, block, (w, h), cx),
                _ => attachment_card(scope, block, cx),
            },
            "code" => {
                let mut lines = Vec::new();
                if !block.lang.is_empty() {
                    lines.push(kit::caption(format!("{scope}/language"), &block.lang));
                }
                lines.push(plain_line(format!("{scope}/code"), &block.text, true));
                padded_all(
                    bordered(
                        background(
                            kit::container(
                                scope.clone(),
                                kit::spaced(
                                    kit::column(format!("{scope}/code-lines"), lines),
                                    kit::spacing::XXS as f32,
                                ),
                            ),
                            p.surface,
                        ),
                        Some(p.border),
                        Some(1.),
                        kit::radius::CONTROL as f32,
                    ),
                    kit::spacing::MD as f32,
                )
            }
            "quote" | "paragraph" => {
                let text = if block.rich {
                    rich_line(format!("{scope}/text"), block, on_link)
                } else {
                    plain_line(format!("{scope}/text"), &block.text, false)
                };
                if block.kind == "quote" {
                    kit::spaced(
                        kit::row(
                            scope.clone(),
                            [
                                kit::vertical_divider(format!("{scope}/bar")),
                                kit::colored(text, p.muted),
                            ],
                        ),
                        kit::spacing::MD as f32,
                    )
                } else {
                    text
                }
            }
            _ => continue,
        };
        children.push(content);
    }
    kit::spaced(kit::column(key, children), kit::spacing::XS as f32)
}

/// A picture that came with the message, in the flow at thumbnail size with
/// its name under it; pressing it opens the file.
fn picture(key: String, block: &ChatBlock, size: (i64, i64), cx: &mut Context<Chat>) -> Node {
    let p = kit::palette();
    let (w, h) = crate::files::picture_box(size.0, size.1);
    let surface = Node::Surface {
        key: format!("{key}/picture"),
        name: "picture".into(),
        args: vec![
            wire::SurfaceValue::Str(crate::files::PICTURE_SURFACE.into()),
            wire::SurfaceValue::Str(crate::files::attachment_file_path(&block.link)),
        ],
        on_event: None,
    };
    let frame = bordered(
        height(
            width(
                kit::container(format!("{key}/frame"), surface),
                Length::Fixed(w),
            ),
            Length::Fixed(h),
        ),
        Some(p.border),
        Some(1.),
        kit::radius::CARD as f32,
    );
    let link = block.link.clone();
    let open = cx.listener(move |chat, _event: &(), _window, cx| {
        cx.notify();
        chat.open_preview(link.clone(), cx)
    });
    let mut open = kit::button_child(
        format!("{key}/open"),
        frame,
        Some(open),
        ButtonPreset::Subtle,
    );
    if let Node::Button { label, padding, .. } = &mut open {
        *label = Some(format!("Open {}", block.text));
        *padding = Some(wire::Edges::all(0.));
    }
    let stack = kit::spaced(
        kit::column(
            format!("{key}/stack"),
            [
                kit::row(format!("{key}/hug"), [open]),
                kit::nowrap(kit::caption(format!("{key}/name"), &block.text)),
            ],
        ),
        3.,
    );
    kit::row(key, [stack])
}

/// A file that came with the message: its name over what it is, in a plate
/// that opens it.
fn attachment_card(key: String, block: &ChatBlock, cx: &mut Context<Chat>) -> Node {
    let p = kit::palette();
    let content = kit::spaced(
        kit::centered_row(
            format!("{key}/row"),
            [
                kit::text(format!("{key}/glyph"), "📄"),
                kit::spaced(
                    kit::column(
                        format!("{key}/name"),
                        [
                            kit::nowrap(kit::strong(format!("{key}/title"), &block.text)),
                            kit::nowrap(kit::caption(
                                format!("{key}/kind"),
                                crate::files::attachment_kind(&block.text),
                            )),
                        ],
                    ),
                    1.,
                ),
            ],
        ),
        kit::spacing::MD as f32,
    );
    let link = block.link.clone();
    let open = cx.listener(move |chat, _event: &(), _window, cx| {
        cx.notify();
        chat.open_preview(link.clone(), cx)
    });
    let mut card = kit::button_child(
        format!("{key}/card"),
        content,
        Some(open),
        ButtonPreset::Secondary,
    );
    if let Node::Button {
        label,
        padding,
        width,
        style,
        ..
    } = &mut card
    {
        *label = Some(format!("Open {}", block.text));
        *padding = Some(wire::Edges {
            top: kit::spacing::SM as f32,
            right: 14.,
            bottom: kit::spacing::SM as f32,
            left: kit::spacing::LG as f32,
        });
        *width = Some(Length::Shrink);
        style.active.background = Some(kit::rgba(p.surface));
        style.active.border = Some(wire::Border {
            color: Some(kit::rgba(p.border)),
            width: Some(1.),
            radius: Some([kit::radius::CARD as f32; 4]),
        });
    }
    kit::row(key, [card])
}

fn rich_text(key: String, spans: Vec<wire::RichSpan>, on_link: Option<u32>) -> Node {
    Node::RichText {
        key,
        spans,
        on_link,
        options: wire::TextOptions {
            wrapping: Some(wire::Wrapping::WordOrGlyph),
            ..Default::default()
        },
        size: None,
        color: None,
        font: Default::default(),
        width: Some(Length::Fill),
        align_x: None,
    }
}

/// An unmarked paragraph or a code block as ONE rich span: only `RichText`
/// carries the host's text-selection handle.
pub fn plain_line(key: String, text: &str, mono: bool) -> Node {
    rich_text(
        key,
        vec![wire::RichSpan {
            content: text.to_owned(),
            size: mono.then_some(kit::type_scale::MONO as f32),
            font: mono.then_some(wire::NamedFont {
                family: wire::FontFamily::Monospace,
                weight: wire::Weight::Normal,
                stretch: wire::FontStretch::Normal,
                style: wire::FontStyle::Normal,
            }),
            ..Default::default()
        }],
        None,
    )
}

pub fn rich_line(key: String, block: &ChatBlock, on_link: Option<u32>) -> Node {
    let p = kit::palette();
    let spans = block
        .spans
        .iter()
        .map(|span| {
            let (link, weight, italic) = match &span.style {
                SpanStyle::Plain => (None, wire::Weight::Normal, false),
                SpanStyle::Bold => (None, wire::Weight::Bold, false),
                SpanStyle::Italic => (None, wire::Weight::Normal, true),
                SpanStyle::BoldItalic => (None, wire::Weight::Bold, true),
                SpanStyle::Link(url) => (Some(url.clone()), wire::Weight::Medium, false),
                SpanStyle::Mention(account) => (Some(account.clone()), wire::Weight::Medium, false),
            };
            let is_link = matches!(span.style, SpanStyle::Link(_));
            let decorated = weight != wire::Weight::Normal || italic;
            wire::RichSpan {
                content: span.text.clone(),
                link: link.filter(|l| !l.is_empty()),
                underline: is_link,
                color: (is_link || matches!(span.style, SpanStyle::Mention(_)))
                    .then_some(kit::rgba(p.link)),
                font: decorated.then_some(wire::NamedFont {
                    family: wire::FontFamily::SansSerif,
                    weight,
                    stretch: wire::FontStretch::Normal,
                    style: if italic {
                        wire::FontStyle::Italic
                    } else {
                        wire::FontStyle::Normal
                    },
                }),
                ..Default::default()
            }
        })
        .collect();
    rich_text(key, spans, on_link)
}

/// A reaction as a pill: the emoji and its count inside a hairline, the
/// reader's own in the accent wash. Without a count it is the "add one" chip.
fn pill(
    key: String,
    emoji: &str,
    count: Option<u64>,
    label: &str,
    mine: bool,
    on_press: Option<u32>,
) -> Node {
    let p = kit::palette();
    let mut parts = vec![kit::nowrap(kit::tall_glyph(
        format!("{key}/emoji"),
        emoji,
        kit::type_scale::BODY as f32,
        PILL_HEIGHT,
    ))];
    if let Some(count) = count {
        parts.push(kit::nowrap(kit::weighted(
            kit::colored(
                kit::text_size(
                    kit::text(format!("{key}/count"), count.to_string()),
                    kit::type_scale::SECONDARY as f32,
                ),
                if mine { p.accent_foreground } else { p.muted },
            ),
            wire::Weight::Medium,
        )));
    }
    let content = kit::spaced(
        kit::centered_row(format!("{key}/label"), parts),
        kit::spacing::XXS as f32,
    );
    let mut button = kit::button_child(key.clone(), content, on_press, ButtonPreset::Subtle);
    if let Node::Button {
        checked,
        label: accessible,
        description,
        padding,
        height,
        ..
    } = &mut button
    {
        *checked = Some(mine);
        *accessible = Some(label.into());
        *description = Some(emoji.into());
        *height = Some(Length::Fixed(PILL_HEIGHT));
        *padding = Some(wire::Edges {
            top: 0.,
            right: kit::spacing::SM as f32,
            bottom: 0.,
            left: kit::spacing::XS as f32,
        });
    }
    width(
        background(
            bordered(
                kit::container(format!("{key}/pill"), button),
                Some(if mine { p.accent } else { p.border }),
                Some(1.),
                kit::radius::PILL as f32,
            ),
            if mine { p.accent_soft } else { p.surface },
        ),
        Length::Shrink,
    )
}

/// The way into a message's thread: an outlined chip with the reply count
/// in the accent and the invitation beside it.
fn reply_link(key: String, replies: u64, open: u32) -> Node {
    let p = kit::palette();
    let content = kit::spaced(
        kit::centered_row(
            format!("{key}/row"),
            [
                kit::nowrap(kit::weighted(
                    kit::colored(
                        kit::text(format!("{key}/count"), plural(replies, "reply", "replies")),
                        p.link,
                    ),
                    wire::Weight::Medium,
                )),
                kit::nowrap(kit::caption(format!("{key}/hint"), "View thread ›")),
            ],
        ),
        kit::spacing::SM as f32,
    );
    let mut button = kit::button_child(key.clone(), content, Some(open), ButtonPreset::Secondary);
    if let Node::Button {
        label,
        padding,
        height,
        ..
    } = &mut button
    {
        *label = Some("Open thread".into());
        *height = Some(Length::Fixed(kit::height::ROW as f32));
        *padding = Some(wire::Edges {
            top: 0.,
            right: kit::spacing::MD as f32,
            bottom: 0.,
            left: kit::spacing::MD as f32,
        });
    }
    kit::padded(
        kit::row(format!("{key}/hug"), [button]),
        wire::Edges {
            top: 2.,
            right: 0.,
            bottom: 0.,
            left: 0.,
        },
    )
}

/// The avatar beside a message: the kit's plate grown to the rail's 28px, a
/// rounded square rather than a pill.
pub fn avatar(key: String, initials: &str, agent: bool) -> Node {
    let tone = if agent { Tone::Agent } else { Tone::Neutral };
    let mut avatar = kit::avatar(key, initials, tone);
    if let Node::Container {
        width,
        height,
        border,
        content,
        ..
    } = &mut avatar
    {
        *width = Some(Length::Fixed(AVATAR));
        *height = Some(Length::Fixed(AVATAR));
        *border = Some(wire::Border {
            color: None,
            width: None,
            radius: Some([kit::radius::CARD as f32; 4]),
        });
        if let Node::Text { size, .. } = content.as_mut() {
            *size = Some(11.5);
        }
    }
    avatar
}
