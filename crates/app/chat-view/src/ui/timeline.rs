//! The room timeline and virtual message lists.
use super::message::{self, Plate};
use super::room::{loading, selection_bar};
use crate::client::ChatMessage;
use crate::{Chat, Mode, Pane};
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire::kit::*;
use ducktape_view_guest::wire::{self, AlignX, AlignY, Length, Node, kit, kit::Tone};

/// The room's beginning: its name as a title and what this place is.
fn intro(key: &str, name: &str, dm: Option<&str>) -> Node {
    let (title, detail) = match dm {
        Some(peer) => (
            peer.to_owned(),
            format!("This is the very beginning of your conversation with {peer}."),
        ),
        None => (
            format!("#{name}"),
            format!(
                "This is the very beginning of #{name}. Say hello, or pin what the room is for."
            ),
        ),
    };
    kit::padded(
        kit::spaced(
            kit::column(
                key,
                [
                    kit::title(format!("{key}/name"), title),
                    kit::wrapping(kit::secondary(format!("{key}/detail"), detail)),
                    kit::gap(kit::spacing::XXS as f32),
                    kit::divider(format!("{key}/rule")),
                ],
            ),
            kit::spacing::XS as f32,
        ),
        wire::Edges {
            top: kit::spacing::XL as f32,
            right: 16.,
            bottom: kit::spacing::SM as f32,
            left: 16.,
        },
    )
}

/// The stream and what stands around it.
pub(super) fn stream(
    chat: &Chat,
    key: &str,
    name: &str,
    dm: Option<&str>,
    cx: &mut Context<Chat>,
) -> Vec<Node> {
    let room = chat.room.as_ref().expect("a room");
    let messages = chat.messages(Pane::Timeline);
    let mut children = Vec::new();
    match &room.messages {
        Loaded::Loading(_) if messages.is_empty() => {
            children.push(loading(format!("{key}/loading")))
        }
        Loaded::Failed(refusal) => children.push(padded_all(
            kit::notice(
                format!("{key}/failed"),
                kit::wrapping(kit::text(
                    format!("{key}/failed/text"),
                    refusal.sentence.clone(),
                )),
                Tone::Danger,
            ),
            kit::spacing::LG as f32,
        )),
        Loaded::Ready(_) if messages.is_empty() => {
            // an empty room opens on its beginning, down by the composer
            children.push(kit::space(None, Some(Length::Fill)));
            children.push(intro(&format!("{key}/empty"), name, dm));
            return children;
        }
        _ => {}
    }
    if room.has_older && !room.landed {
        let older = (!room.older_loading && !chat.session.busy).then(|| {
            cx.listener(|chat, _event: &(), _window, cx| {
                cx.notify();
                chat.load_older(cx)
            })
        });
        let label = if room.older_loading {
            "Loading older messages…"
        } else {
            "Load older messages"
        };
        children.push(padded_all(
            aligned_x(
                kit::column(
                    format!("{key}/older-row"),
                    [subtle(format!("{key}/older"), label, older)],
                ),
                AlignX::Center,
            ),
            kit::spacing::SM as f32,
        ));
    }
    if !messages.is_empty() {
        let whole_history = !room.has_older && !room.landed;
        let lead = whole_history.then(|| intro(&format!("{key}/intro"), name, dm));
        children.push(list(
            chat,
            super::super::room::STREAM_KEY,
            &messages,
            Pane::Timeline,
            lead,
            cx,
        ));
    }
    if chat.copy.is_some_and(|c| c.pane == Pane::Timeline) {
        children.push(selection_bar(chat, key, cx));
    }
    let behind_head = room.landed && !room.reaches_head;
    if !messages.is_empty() && (behind_head || !room.at_tail || room.landed) {
        let id = room.id.clone();
        let latest = cx.listener(move |chat, _event: &(), window, cx| {
            cx.notify();
            chat.open(id.clone(), window, cx)
        });
        children.push(padded_xy(
            aligned_x(
                kit::column(
                    format!("{key}/latest-row"),
                    [action(
                        format!("{key}/latest"),
                        "Jump to latest",
                        Some(latest),
                    )],
                ),
                AlignX::Center,
            ),
            16.,
            kit::spacing::XXS as f32,
        ));
    }
    children.extend(super::menu::editing(chat, Pane::Timeline, cx));
    children
}

/// The keyed, virtual list of one pane's messages: each row a card under a
/// hover bar of actions, a right press opening the same menu.
pub fn list(
    chat: &Chat,
    key: &str,
    messages: &[ChatMessage],
    pane: Pane,
    lead: Option<Node>,
    cx: &mut Context<Chat>,
) -> Node {
    let thread = pane == Pane::Thread;
    let mut keys = Vec::new();
    let mut rows = Vec::new();
    let boundary = chat.reads.boundary;
    let unread_marker = (!thread && boundary > 0)
        .then(|| {
            messages
                .iter()
                .find(|m| !m.pending && m.seq > boundary)
                .map(|m| m.seq)
        })
        .flatten();
    let writable = chat.may_write();
    for (index, message) in messages.iter().enumerate() {
        let scope = format!("{key}/message/{}", message.id);
        let ranged = chat.copy.is_some_and(|c| c.holds(pane, message.seq));
        let chosen = chat
            .menu
            .as_ref()
            .is_some_and(|m| m.pane == pane && m.seq == message.seq && message.seq > 0);
        let plate = match (message.deleted, chosen, ranged) {
            (true, _, _) | (false, false, false) => Plate::Plain,
            (false, true, _) => Plate::Selected,
            (false, false, true) => Plate::Ranged,
        };
        let mut children = Vec::new();
        if unread_marker == Some(message.seq) {
            children.push(unread_marker_node(format!("{scope}/unread")));
        }
        let card = message::card(chat, message, pane, plate, cx);
        if !message.pending && !message.deleted {
            let (seq, rev) = (message.seq, message.rev);
            let mut controls = Vec::new();
            if !thread && message.reply_count == 0 {
                let open = cx.listener(move |chat, _event: &(), _window, cx| {
                    cx.notify();
                    chat.open_thread(seq, cx)
                });
                controls.push(glyph(
                    format!("{scope}/thread"),
                    "💬",
                    "Open thread",
                    Some(open),
                ));
            }
            let thumbs = writable.then(|| {
                cx.listener(move |chat, _event: &(), _window, cx| {
                    cx.notify();
                    chat.react(seq, "👍".into(), true, cx)
                })
            });
            controls.push(glyph(
                format!("{scope}/thumbs-up"),
                "👍",
                "React with 👍",
                thumbs,
            ));
            let react = writable.then(|| {
                cx.listener(move |chat, _event: &(), window, cx| {
                    cx.notify();
                    chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
                })
            });
            controls.push(glyph(
                format!("{scope}/react"),
                "😀",
                "Manage reactions",
                react,
            ));
            let more = cx.listener(move |chat, _event: &(), window, cx| {
                cx.notify();
                chat.open_menu(pane, seq, rev, Mode::More, window, cx)
            });
            controls.push(glyph(
                format!("{scope}/more"),
                "⋯",
                "More message actions",
                Some(more),
            ));
            let mut wash = kit::palette().surface_raised;
            wash[3] = 0.6;
            let hover = Node::Hover {
                key: format!("{scope}/hover"),
                width: Some(Length::Fill),
                height: None,
                padding: None,
                background: None,
                border: None,
                tint: (!chosen).then_some(wire::Rgba(wash)),
                radius: 0.,
                open: chosen,
                children: vec![card, floating_actions(format!("{scope}/actions"), controls)],
            };
            children.push(hover);
            let content = kit::spaced(kit::column(format!("{scope}/content"), children), 0.);
            rows.push(with_right_press(mouse_area(scope, content), more));
        } else {
            children.push(card);
            rows.push(kit::spaced(kit::column(scope, children), 0.));
        }
        let list_key = if message.pending {
            -(index as i64) - 1
        } else {
            message.seq as i64
        };
        keys.push(wire::ListKey::from(list_key));
    }
    let list = Node::KeyedColumn {
        key: format!("{key}/rows"),
        keys: Some(keys),
        children: rows,
        background: None,
        border: None,
        spacing: None,
        padding: Some(wire::Edges {
            top: kit::spacing::SM as f32,
            right: 0.,
            bottom: kit::spacing::SM as f32,
            left: 0.,
        }),
        width: Some(Length::Fill),
        height: None,
        max_width: None,
        align: None,
        virtual_row: Some(44.),
    };
    let content = match lead {
        Some(intro) => kit::spaced(kit::column(format!("{key}/lead"), [intro, list]), 0.),
        None => list,
    };
    let mut scroll = kit::scroll(key, content);
    if let Node::Scroll {
        virtual_rows,
        anchor_y,
        on_scroll,
        ..
    } = &mut scroll
    {
        *virtual_rows = true;
        // a room grows upward from its composer; a thread reads down
        *anchor_y = if thread {
            wire::ScrollAnchor::Start
        } else {
            wire::ScrollAnchor::End
        };
        if !thread {
            *on_scroll = Some(
                cx.listener(|chat, event: &(f32, f32, f32, f32), _window, cx| {
                    let (_, _, _, ry) = *event;
                    cx.notify();
                    chat.scrolled(ry, cx)
                }),
            );
        }
    }
    scroll
}

/// The bar of quiet actions that floats over a message's top-right.
fn floating_actions(key: String, controls: Vec<Node>) -> Node {
    let p = kit::palette();
    let bar = width(
        padded_all(
            bordered(
                background(
                    kit::spaced(kit::row(format!("{key}/bar"), controls), 0.),
                    p.background,
                ),
                Some(p.border),
                Some(1.),
                kit::radius::CONTROL as f32,
            ),
            2.,
        ),
        Length::Shrink,
    );
    kit::padded(
        height(
            aligned_y(
                aligned_x(kit::container(key, bar), AlignX::Right),
                AlignY::Top,
            ),
            Length::Fill,
        ),
        wire::Edges {
            top: 0.,
            right: kit::spacing::LG as f32,
            bottom: 0.,
            left: 0.,
        },
    )
}

fn unread_marker_node(key: String) -> Node {
    let p = kit::palette();
    let mut rule = kit::divider(format!("{key}/rule"));
    if let Node::Rule { color, .. } = &mut rule {
        *color = Some(kit::rgba(p.accent));
    }
    padded_xy(
        kit::spaced(
            kit::centered_row(
                key.clone(),
                [
                    kit::container(format!("{key}/line"), rule),
                    kit::nowrap(kit::colored(
                        kit::caption(format!("{key}/label"), "New messages"),
                        p.accent_foreground,
                    )),
                ],
            ),
            kit::spacing::SM as f32,
        ),
        16.,
        kit::spacing::XS as f32,
    )
}
