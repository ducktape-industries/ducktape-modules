//! The room pane: its header, the search results or the message stream
//! (intro, older pages, unread marker, floating actions, copy range, jump to
//! latest, the edit under it), and the composer or the reason there is none.
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::wire::{self, Length, Node, kit, kit::Tone};

use super::message;
pub use super::timeline::list;
use super::timeline::stream;
use crate::Chat;
use crate::composer::Target;
use ducktape_view_guest::wire::kit::*;

/// The room around a composer: the timeline's 16px sides, and air under it.
const COMPOSER_MARGIN: wire::Edges = wire::Edges {
    top: kit::spacing::XXS as f32,
    right: 16.,
    bottom: kit::spacing::LG as f32,
    left: 16.,
};

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> Node {
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
    if info.is_some_and(crate::chat::members_only) {
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
    let details = cx.listener(|chat, _event: &(), _window, cx| {
        cx.notify();
        chat.toggle_details()
    });
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
        let dismiss = cx.listener(|chat, _event: &(), _window, cx| {
            cx.notify();
            chat.notice.clear()
        });
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
fn no_room(chat: &Chat, key: &str, cx: &mut Context<Chat>) -> Node {
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
            let open = cx.listener(|chat, _event: &(), _window, cx| {
                cx.notify();
                chat.create = Some(crate::ChannelCreate::default())
            });
            kit::empty_state_action(
                &key,
                "No channels yet",
                "This network has no channel to read. The first one you create is there for everyone on it.",
                gated(
                    kit::primary(format!("{key}/create"), "Create a channel", Some(open)),
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
fn huddle(chat: &Chat, key: &str, info: &crate::chat::ChannelInfo, cx: &mut Context<Chat>) -> Node {
    let key = format!("{key}/huddle");
    let session = &chat.session;
    if session.huddle_joined && session.huddle_channel == info.channel.id {
        let elapsed = mmss(session.huddle_now - session.huddle_joined_at);
        let mut children = vec![kit::badge(
            format!("{key}/live"),
            format!("Live {elapsed}"),
            Tone::Success,
        )];
        if session.call_muted {
            children.push(kit::badge(format!("{key}/muted"), "Muted", Tone::Neutral));
        }
        let show = cx.listener(|_, _event: &(), _window, cx| {
            cx.notify();
            cx.host().notify::<crate::api::ShowHuddle>(())
        });
        let leave = cx.listener(|_, _event: &(), _window, cx| {
            cx.notify();
            cx.host().notify::<crate::api::LeaveHuddle>(())
        });
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
        cx.listener(move |_, _event: &(), _window, cx| {
            cx.notify();
            cx.host()
                .notify::<crate::api::JoinVoice>(serde_json::json!({"id": id}))
        })
    } else {
        cx.listener(|_, _event: &(), _window, cx| {
            cx.notify();
            cx.host().notify::<crate::api::JoinHuddle>(())
        })
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

fn search_results(chat: &Chat, key: &str, cx: &mut Context<Chat>) -> Node {
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
                let open = cx.listener(move |chat, _event: &(), window, cx| {
                    cx.notify();
                    chat.open_hit(channel.clone(), seq, window, cx)
                });
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
                let more = (!chat.search.more_loading).then(|| {
                    cx.listener(|chat, _event: &(), _window, cx| {
                        cx.notify();
                        chat.search_more(cx)
                    })
                });
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

pub fn selection_bar(chat: &Chat, key: &str, cx: &mut Context<Chat>) -> Node {
    let key = format!("{key}/copy-range");
    let count = chat.copy_count();
    let label = match count {
        1 => "1 message selected".to_owned(),
        n => format!("{n} messages selected"),
    };
    let clear = cx.listener(|chat, _event: &(), _window, cx| {
        cx.notify();
        chat.copy = None
    });
    let copy = cx.listener(|chat, _event: &(), _window, cx| {
        cx.notify();
        chat.copy_range(cx)
    });
    padded_xy(
        kit::notice(
            key.clone(),
            kit::centered_row(
                format!("{key}/row"),
                [
                    fill_width(kit::strong(format!("{key}/count"), label)),
                    subtle(format!("{key}/clear"), "Clear", Some(clear)),
                    kit::primary(format!("{key}/copy"), "Copy", Some(copy)),
                ],
            ),
            Tone::Accent,
        ),
        16.,
        kit::spacing::XXS as f32,
    )
}

/// What stands where the composer would: why the reader may not post here.
fn gate(chat: &Chat, key: &str, refusal: &str, cx: &mut Context<Chat>) -> Node {
    let key = format!("{key}/refusal");
    match refusal {
        "channel_archived" => {
            let reopen = (!chat.session.busy).then(|| {
                cx.listener(|chat, _event: &(), _window, cx| {
                    cx.notify();
                    chat.set_archived(false, cx)
                })
            });
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
    cx: &mut Context<Chat>,
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
        crate::ATTACHMENTS,
        &choices,
        cx,
        move |chat, event, window, cx| chat.composer(target.clone(), event, window, cx),
    )
}
