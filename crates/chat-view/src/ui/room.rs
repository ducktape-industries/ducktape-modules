//! The room pane: its header, the search results or the message stream
//! (intro, older pages, unread marker, floating actions, copy range, jump to
//! latest, the edit under it), and the composer or the reason there is none.
use ducktape_view_guest::view::{Cx, Loaded};
use ducktape_view_guest::wire::{self, AlignX, AlignY, Length, Node, kit, kit::Tone};

use super::controls::*;
use super::message::{self, Plate};
use super::{gives_way, pane_header};
use crate::client::ChatMessage;
use crate::composer::send::Target;
use crate::{Chat, Mode, Pane};

/// The room around a composer: the timeline's 16px sides, and air under it.
const COMPOSER_MARGIN: wire::Edges = wire::Edges {
    top: kit::spacing::XXS as f32,
    right: 16.,
    bottom: kit::spacing::LG as f32,
    left: 16.,
};

pub fn render(chat: &Chat, cx: &mut Cx<Chat>) -> Node {
    let key = "chat/room";
    let Some(room) = &chat.room else {
        return no_room(chat, key, cx);
    };
    let info = chat.room_info();
    let name = info
        .map(|i| i.channel.name.clone())
        .unwrap_or_else(|| kit::short_id(&room.id, 8));
    let dm = super::sidebar::dm_peer(chat);
    let p = kit::palette();
    let mut title = Vec::new();
    match &dm {
        Some((peer, agent)) => title.push(kit::spaced(
            kit::centered_row(
                format!("{key}/dm-header"),
                [
                    message::avatar(format!("{key}/dm-avatar"), &kit::initials(peer), *agent),
                    kit::heading(format!("{key}/dm-name"), peer.clone()),
                ],
            ),
            kit::spacing::SM as f32,
        )),
        None => {
            title.push(kit::nowrap(kit::colored(
                kit::heading(format!("{key}/hash"), "#"),
                p.muted,
            )));
            title.push(gives_way(
                &format!("{key}/name-box"),
                kit::heading(format!("{key}/name"), &name),
            ));
        }
    }
    let archived = info.is_some_and(|i| i.channel.archived);
    if archived {
        title.push(kit::badge(
            format!("{key}/archived"),
            "Archived",
            Tone::Neutral,
        ));
    }
    if info.is_some_and(|i| i.members_only()) {
        title.push(kit::badge(
            format!("{key}/private"),
            "Members only",
            Tone::Neutral,
        ));
    }
    let mut header = vec![fill_width(kit::spaced(
        kit::centered_row(format!("{key}/title"), title),
        kit::spacing::SM as f32,
    ))];
    header.extend(info.map(|info| huddle(chat, key, info, cx)));
    let details = cx.on(|chat, _| chat.toggle_details());
    header.push(glyph(
        format!("{key}/details"),
        "Details",
        "Channel details",
        Some(details),
    ));
    let mut children = vec![
        pane_header(&format!("{key}/header"), header),
        kit::divider(format!("{key}/header-rule")),
    ];
    if !chat.notice.is_empty() {
        let dismiss = cx.on(|chat, _| chat.notice.clear());
        children.push(padded_all(
            kit::notice(
                format!("{key}/error"),
                kit::spaced(
                    kit::centered_row(
                        format!("{key}/error/row"),
                        [
                            fill_width(kit::wrapping(kit::text(
                                format!("{key}/error/text"),
                                &chat.notice,
                            ))),
                            subtle(format!("{key}/error/dismiss"), "Dismiss", Some(dismiss)),
                        ],
                    ),
                    kit::spacing::SM as f32,
                ),
                Tone::Danger,
            ),
            kit::spacing::LG as f32,
        ));
    }
    if !chat.search.query.is_empty() {
        children.push(search_results(chat, key, cx));
    } else {
        children.extend(stream(
            chat,
            key,
            &name,
            dm.as_ref().map(|d| d.0.as_str()),
            cx,
        ));
    }
    let refusal = chat.write_refusal();
    if !refusal.is_empty() {
        children.push(kit::padded(
            gate(chat, key, refusal, cx),
            wire::Edges {
                top: kit::spacing::SM as f32,
                ..COMPOSER_MARGIN
            },
        ));
    } else {
        let target = Target::Post {
            channel: room.id.clone(),
            thread: None,
        };
        let hint = match &dm {
            Some((peer, _)) => format!("Message {peer}"),
            None => format!("Message #{name}"),
        };
        let editable = !chat.session.loading && chat.session.connected;
        children.push(kit::padded(
            composer(chat, target, &hint, editable, cx),
            COMPOSER_MARGIN,
        ));
    }
    fill(kit::spaced(kit::column(key, children), 0.))
}

/// The pane with no room open: nothing here pretends to be one.
fn no_room(chat: &Chat, key: &str, cx: &mut Cx<Chat>) -> Node {
    let key = format!("{key}/no-room");
    let plate = match &chat.channels {
        Loaded::Idle | Loaded::Loading(_) => kit::empty_state(
            &key,
            "Loading channels…",
            "Waiting for this network's channel list.",
        ),
        Loaded::Failed(refusal) => {
            kit::empty_state(&key, "Couldn’t read the channels", refusal.sentence.clone())
        }
        Loaded::Ready(rooms) if !rooms.is_empty() => kit::empty_state(
            &key,
            "No channel open",
            "Pick a room from the list to read it.",
        ),
        Loaded::Ready(_) => {
            let open = cx.on(|chat, _| chat.create = Some(crate::ChannelCreate::default()));
            kit::empty_state_action(
                &key,
                "No channels yet",
                "This network has no channel to read. The first one you create is there for everyone on it.",
                gated(
                    super::controls::primary(
                        format!("{key}/create"),
                        "Create a channel",
                        Some(open),
                    ),
                    chat.session.holds_account(),
                    "Create an account to create a channel",
                ),
            )
        }
    };
    fill(plate)
}

/// The header speaks for THIS room's huddle: seated elsewhere, the room on
/// screen still offers its own to join.
fn huddle(chat: &Chat, key: &str, info: &crate::chat::ChannelInfo, cx: &mut Cx<Chat>) -> Node {
    let key = format!("{key}/huddle");
    let session = &chat.session;
    if session.huddle_joined && session.huddle_channel == info.channel.id {
        let elapsed = crate::client::mmss(session.huddle_now - session.huddle_joined_at);
        let mut children = vec![kit::badge(
            format!("{key}/live"),
            format!("Live {elapsed}"),
            Tone::Success,
        )];
        if session.call_muted {
            children.push(kit::badge(format!("{key}/muted"), "Muted", Tone::Neutral));
        }
        let show = cx.on(|_, cx| cx.notify::<crate::api::ShowHuddle>(()));
        let leave = cx.on(|_, cx| cx.notify::<crate::api::LeaveHuddle>(()));
        children.push(with_label(
            subtle(format!("{key}/show"), "Show", Some(show)),
            "Show call",
        ));
        children.push(with_label(
            subtle(format!("{key}/leave"), "Leave", Some(leave)),
            "Leave call",
        ));
        return width(
            kit::spaced(kit::centered_row(key, children), kit::spacing::XS as f32),
            Length::Shrink,
        );
    }
    let allowed = session.holds_account() && !info.channel.archived;
    let join = if info.channel.voice {
        let id = info.channel.id.clone();
        cx.on(move |_, cx| cx.notify::<crate::api::JoinVoice>(serde_json::json!({"id": id})))
    } else {
        cx.on(|_, cx| cx.notify::<crate::api::JoinHuddle>(()))
    };
    let label = if info.channel.huddle.is_empty() {
        "Call"
    } else {
        "Join"
    };
    gated(
        with_label(subtle(key, label, Some(join)), "Start a call"),
        allowed,
        "Create an account to start a call",
    )
}

fn search_results(chat: &Chat, key: &str, cx: &mut Cx<Chat>) -> Node {
    let key = format!("{key}/search-results");
    let children = match &chat.search.hits {
        Loaded::Idle | Loaded::Loading(_) => vec![loading(format!("{key}/loading"))],
        Loaded::Failed(refusal) => vec![kit::empty_state(
            format!("{key}/failed"),
            "Search didn’t go through",
            refusal.sentence.clone(),
        )],
        Loaded::Ready(hits) if hits.rows.is_empty() => vec![kit::empty_state(
            format!("{key}/empty"),
            "No messages match",
            if hits.capped {
                "Results capped; narrow your search for more."
            } else {
                "Try other words, or clear the search to see the room again."
            },
        )],
        Loaded::Ready(hits) => {
            let query = &chat.search.query;
            let mut summary = match hits.rows.len() {
                1 => format!("1 result for “{query}”"),
                n => format!("{n} results for “{query}”"),
            };
            if hits.capped {
                summary.push_str(" · results capped; narrow your search");
            } else if hits.has_more {
                summary.push_str(" · more results available");
            }
            let mut rows = vec![kit::padded(
                kit::container(
                    format!("{key}/summary-box"),
                    kit::label(format!("{key}/summary"), summary),
                ),
                wire::Edges {
                    top: kit::spacing::SM as f32,
                    right: kit::spacing::SM as f32,
                    bottom: kit::spacing::XXS as f32,
                    left: kit::spacing::SM as f32,
                },
            )];
            let names = chat.names.ready().cloned().unwrap_or_default();
            for hit in &hits.rows {
                let hit_key = format!("{key}/{}/{}", hit.channel_id, hit.seq);
                let room = chat.info(&hit.channel_id).map_or_else(
                    || format!("#{}", hit.channel_id),
                    |info| format!("#{}", info.channel.name),
                );
                let text = if hit.text.is_empty() {
                    crate::client::message_body(&hit.blocks, &names)
                } else {
                    hit.text.clone()
                };
                let (channel, seq) = (hit.channel_id.clone(), hit.seq);
                let open = cx.on(move |chat, cx| chat.open_hit(channel.clone(), seq, cx));
                let content = kit::spaced(
                    kit::column(
                        format!("{hit_key}/content"),
                        [
                            kit::spaced(
                                kit::centered_row(
                                    format!("{hit_key}/byline"),
                                    [
                                        kit::nowrap(kit::strong(
                                            format!("{hit_key}/author"),
                                            crate::client::author_display(&hit.author, &names),
                                        )),
                                        kit::nowrap(kit::caption(format!("{hit_key}/room"), room)),
                                        kit::nowrap(kit::caption(
                                            format!("{hit_key}/meta"),
                                            format!("message {seq}"),
                                        )),
                                    ],
                                ),
                                kit::spacing::XS as f32,
                            ),
                            kit::wrapping(kit::secondary(format!("{hit_key}/text"), text.clone())),
                        ],
                    ),
                    2.,
                );
                let mut button = kit::list_row(hit_key, content, false, Some(open));
                if let Node::Button { label, padding, .. } = &mut button {
                    *label = Some(text);
                    *padding = Some(wire::Edges {
                        top: kit::spacing::XS as f32,
                        right: kit::spacing::SM as f32,
                        bottom: kit::spacing::XS as f32,
                        left: kit::spacing::SM as f32,
                    });
                }
                rows.push(button);
            }
            if hits.has_more {
                let more =
                    (!chat.search.more_loading).then(|| cx.on(|chat, cx| chat.search_more(cx)));
                let label = if chat.search.more_loading {
                    "Loading more results…"
                } else {
                    "Load more results"
                };
                rows.push(padded_all(
                    subtle(format!("{key}/load-more"), label, more),
                    kit::spacing::SM as f32,
                ));
            }
            rows
        }
    };
    kit::scroll(
        key.clone(),
        padded_all(
            kit::spaced(kit::column(format!("{key}/rows"), children), 2.),
            kit::spacing::SM as f32,
        ),
    )
}

pub fn loading(key: String) -> Node {
    kit::empty_state(key, "Loading messages…", "The newest arrive first.")
}

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
fn stream(chat: &Chat, key: &str, name: &str, dm: Option<&str>, cx: &mut Cx<Chat>) -> Vec<Node> {
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
        let older = (!room.older_loading && !chat.session.busy)
            .then(|| cx.on(|chat, cx| chat.load_older(cx)));
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
        let latest = cx.on(move |chat, cx| chat.open(id.clone(), cx));
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
    cx: &mut Cx<Chat>,
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
    let live = chat.live_runs();
    let thread_root = chat
        .room
        .as_ref()
        .and_then(|r| r.thread.as_ref())
        .map_or(0, |t| t.root);
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
        // a run in flight answers into the thread: the timeline counts it
        let answering = if thread {
            0
        } else {
            live.iter().filter(|run| run.in_thread(message.seq)).count() as u64
        };
        let counted;
        let message = if answering > 0 {
            counted = ChatMessage {
                reply_count: message.reply_count + answering,
                ..message.clone()
            };
            &counted
        } else {
            message
        };
        let card = message::card(chat, message, pane, plate, cx);
        if !message.pending && !message.deleted {
            let (seq, rev) = (message.seq, message.rev);
            let mut controls = Vec::new();
            if !thread && message.reply_count == 0 {
                let open = cx.on(move |chat, cx| chat.open_thread(seq, cx));
                controls.push(glyph(
                    format!("{scope}/thread"),
                    "💬",
                    "Open thread",
                    Some(open),
                ));
            }
            let thumbs =
                writable.then(|| cx.on(move |chat, cx| chat.react(seq, "👍".into(), true, cx)));
            controls.push(glyph(
                format!("{scope}/thumbs-up"),
                "👍",
                "React with 👍",
                thumbs,
            ));
            let react = writable.then(|| {
                cx.on(move |chat, cx| chat.open_menu(pane, seq, rev, Mode::Reactions, cx))
            });
            controls.push(glyph(
                format!("{scope}/react"),
                "😀",
                "Manage reactions",
                react,
            ));
            let more = cx.on(move |chat, cx| chat.open_menu(pane, seq, rev, Mode::More, cx));
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
    // the runs answering into this thread ride at its tail, once each
    if thread {
        for run in live.iter().filter(|run| run.in_thread(thread_root)) {
            let answered = messages
                .iter()
                .any(|m| m.agent && m.author == run.seed.agent && m.seq > run.seed.anchor_seq);
            if answered {
                continue;
            }
            let live_message = crate::live::run_message(run);
            let run_key = format!("{key}/run/{}", run.seed.run_id);
            let card = message::card(chat, &live_message, pane, Plate::Plain, cx);
            let dispatch = run.seed.dispatch_id.clone();
            let open = cx.on(move |chat, _| chat.open_run(&dispatch));
            let run_id = run.seed.run_id.clone();
            let stop = cx.on(move |chat, cx| chat.cancel_run(run_id.clone(), cx));
            let actions = kit::padded(
                kit::spaced(
                    kit::row(
                        format!("{run_key}/actions"),
                        [
                            subtle(format!("{run_key}/open"), "View run", Some(open)),
                            subtle(format!("{run_key}/stop"), "Stop", Some(stop)),
                        ],
                    ),
                    kit::spacing::XS as f32,
                ),
                wire::Edges {
                    top: 0.,
                    right: 16.,
                    bottom: kit::spacing::XXS as f32,
                    left: message::RAIL,
                },
            );
            rows.push(kit::spaced(kit::column(run_key, [card, actions]), 0.));
            let hash = run
                .seed
                .run_id
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |acc, b| {
                    (acc ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                });
            keys.push(wire::ListKey::from(-((hash >> 1) as i64).max(1)));
        }
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
                cx.on_value(|chat, (_, _, _, ry): (f32, f32, f32, f32), cx| chat.scrolled(ry, cx)),
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

pub fn selection_bar(chat: &Chat, key: &str, cx: &mut Cx<Chat>) -> Node {
    let key = format!("{key}/copy-range");
    let count = chat.copy_count();
    let label = match count {
        1 => "1 message selected".to_owned(),
        n => format!("{n} messages selected"),
    };
    let clear = cx.on(|chat, _| chat.copy = None);
    let copy = cx.on(|chat, cx| chat.copy_range(cx));
    padded_xy(
        kit::notice(
            key.clone(),
            kit::centered_row(
                format!("{key}/row"),
                [
                    fill_width(kit::strong(format!("{key}/count"), label)),
                    subtle(format!("{key}/clear"), "Clear", Some(clear)),
                    super::controls::primary(format!("{key}/copy"), "Copy", Some(copy)),
                ],
            ),
            Tone::Accent,
        ),
        16.,
        kit::spacing::XXS as f32,
    )
}

/// What stands where the composer would: why the reader may not post here.
fn gate(chat: &Chat, key: &str, refusal: &str, cx: &mut Cx<Chat>) -> Node {
    let key = format!("{key}/refusal");
    match refusal {
        "channel_archived" => {
            let reopen =
                (!chat.session.busy).then(|| cx.on(|chat, cx| chat.set_archived(false, cx)));
            kit::notice(
                key.clone(),
                kit::spaced(
                    kit::centered_row(
                        format!("{key}/row"),
                        [
                            fill_width(kit::wrapping(kit::text(
                                format!("{key}/text"),
                                "This channel is archived. It keeps its history and takes no new messages.",
                            ))),
                            with_label(
                                action(format!("{key}/unarchive"), "Unarchive", reopen),
                                "Unarchive channel",
                            ),
                        ],
                    ),
                    kit::spacing::LG as f32,
                ),
                Tone::Neutral,
            )
        }
        "members_only" => kit::notice(
            key.clone(),
            kit::wrapping(kit::text(
                format!("{key}/text"),
                "This channel is members-only and your key is not on its roster. Ask a member to add your key from Channel details.",
            )),
            Tone::Warning,
        ),
        _ => kit::notice(
            key.clone(),
            kit::wrapping(kit::text(
                format!("{key}/text"),
                "To send messages, create or join an account in Settings → Account. You can read this channel without an account.",
            )),
            Tone::Warning,
        ),
    }
}

/// The composer for one target: its draft, keyed by the target so a
/// half-finished body waits where it was written.
pub fn composer(
    chat: &Chat,
    target: Target,
    hint: &str,
    editable: bool,
    cx: &mut Cx<Chat>,
) -> Node {
    let key = crate::draft_key(&target);
    let choices = chat.mention_choices();
    let empty = crate::composer::Draft::default();
    let draft = chat.drafts.get(&key).unwrap_or(&empty);
    crate::composer::view(
        draft,
        &key,
        hint,
        editable,
        &choices,
        cx,
        move |chat, event, cx| chat.composer(target.clone(), event, cx),
    )
}
