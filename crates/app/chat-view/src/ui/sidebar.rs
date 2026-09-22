//! The list pane: search, the channels (with the door to a new one), the
//! voice rooms and the direct messages, each row marked unread when its
//! head moved past what the reader saw.
use ducktape_view_guest::Context;
use ducktape_view_guest::wire::{self, Length, Node, kit, kit::Tone};

use crate::chat::ChannelInfo;
use crate::{ChannelCreate, Chat};
use ducktape_view_guest::wire::kit::*;

pub fn render(chat: &Chat, cx: &mut Context<Chat>) -> Node {
    let key = "chat/sidebar";
    let typed = cx.listener(|chat, event: &String, _window, cx| {
        let text = event.clone();
        cx.notify();
        chat.search.draft = text
    });
    let submit = cx.listener(|chat, _event: &(), _window, cx| {
        cx.notify();
        chat.search_submit(cx)
    });
    let mut search = text_field(
        format!("{key}/search"),
        "Search messages",
        &chat.search.draft,
        typed,
        Some(submit),
        false,
    );
    if let Node::Input { placeholder, .. } = &mut search {
        *placeholder = "Search messages…".into();
    }
    let mut search_row = vec![fill_width(search)];
    if !chat.search.query.is_empty() || !chat.search.draft.trim().is_empty() {
        let clear = cx.listener(|chat, _event: &(), _window, cx| {
            cx.notify();
            chat.search_clear()
        });
        search_row.push(glyph(
            format!("{key}/clear-search"),
            "✕",
            "Clear message search",
            Some(clear),
        ));
    }
    let top = padded_all(
        kit::spaced(
            kit::centered_row(format!("{key}/search-row"), search_row),
            kit::spacing::XXS as f32,
        ),
        kit::spacing::SM as f32,
    );

    let (mark, name) = match chat.create {
        Some(_) => ("✕", "Close"),
        None => ("+", "New channel"),
    };
    let toggle = cx.listener(|chat, _event: &(), _window, cx| {
        cx.notify();
        chat.create = match chat.create.take() {
            Some(_) => None,
            None => Some(ChannelCreate::default()),
        }
    });
    let busy = chat.session.loading || chat.session.busy;
    let door = gated(
        glyph(
            format!("{key}/new-channel"),
            mark,
            name,
            (!busy).then_some(toggle),
        ),
        // closing an open form is a read: only the door is gated
        chat.create.is_some() || chat.session.holds_account(),
        "Create an account to create a channel",
    );
    let mut rows = vec![section_row(
        &format!("{key}/channels-header"),
        "Channels",
        Some(door),
    )];
    let channels: Vec<&ChannelInfo> = chat.channels.ready().into_iter().flatten().collect();
    if chat.channels.is_loading() && channels.is_empty() {
        rows.push(kit::caption(format!("{key}/loading"), "Loading rooms…"));
    }
    if let Some(refusal) = chat.channels.failed() {
        rows.push(kit::caption(
            format!("{key}/failed"),
            refusal.sentence.clone(),
        ));
    }
    let open = chat.room.as_ref().map(|room| room.id.as_str());
    let mine = chat.my_account();
    let mut dms = Vec::new();
    let mut voice = Vec::new();
    for info in channels {
        if crate::chat::dm_peers(&info.channel.id).is_some() {
            if let Some(peer) =
                mine.and_then(|mine| crate::client::dm_peer_of(mine, &info.channel.id))
            {
                dms.push((info, peer));
            }
            continue;
        }
        if info.channel.voice {
            voice.push(info);
            continue;
        }
        rows.push(channel_button(
            chat,
            key,
            info,
            open == Some(info.channel.id.as_str()),
            cx,
        ));
    }
    if !voice.is_empty() {
        rows.push(kit::gap(kit::spacing::SM as f32));
        rows.push(section_row(&format!("{key}/voice-header"), "Voice", None));
        for info in voice {
            rows.push(voice_button(chat, key, info, cx));
        }
    }
    if !dms.is_empty() {
        rows.push(kit::gap(kit::spacing::SM as f32));
        rows.push(section_row(
            &format!("{key}/dm-header"),
            "Direct messages",
            None,
        ));
        for (info, peer) in dms {
            rows.push(dm_button(
                chat,
                key,
                info,
                peer,
                open == Some(info.channel.id.as_str()),
                cx,
            ));
        }
    }
    let list = kit::scroll(
        format!("{key}/rooms"),
        kit::padded(
            kit::spaced(kit::column(format!("{key}/room-list"), rows), 2.),
            wire::Edges {
                top: kit::spacing::XXS as f32,
                right: kit::spacing::SM as f32,
                bottom: kit::spacing::LG as f32,
                left: kit::spacing::SM as f32,
            },
        ),
    );
    kit::pane(
        key,
        fill(kit::spaced(
            kit::column(format!("{key}/content"), [top, list]),
            0.,
        )),
        Length::Fixed(chat.layout.sidebar),
    )
}

/// A channel: the hash, the name, and what stands out about it.
fn channel_button(
    chat: &Chat,
    key: &str,
    info: &ChannelInfo,
    selected: bool,
    cx: &mut Context<Chat>,
) -> Node {
    let p = kit::palette();
    let key = format!("{key}/channel/{}", info.channel.id);
    let unread = chat.unread(info) && !selected;
    let name = if unread {
        kit::strong(format!("{key}/name"), &info.channel.name)
    } else {
        kit::text(format!("{key}/name"), &info.channel.name)
    };
    let mut children = vec![
        kit::nowrap(kit::colored(kit::text(format!("{key}/hash"), "#"), p.muted)),
        gives_way(&format!("{key}/name-box"), name),
    ];
    if !info.channel.huddle.is_empty() {
        children.push(kit::nowrap(kit::colored(
            kit::text_size(
                kit::text(
                    format!("{key}/huddle"),
                    format!("🔊 {}", info.channel.huddle.len()),
                ),
                kit::type_scale::CAPTION as f32,
            ),
            p.success,
        )));
    }
    if crate::chat::members_only(info) {
        children.push(kit::nowrap(kit::caption(
            format!("{key}/members-only"),
            "Members only",
        )));
    }
    if info.channel.archived {
        children.push(kit::nowrap(kit::caption(
            format!("{key}/archived"),
            "Archived",
        )));
    }
    if unread {
        children.push(kit::spacer());
        children.push(unread_dot(format!("{key}/unread")));
    }
    let id = info.channel.id.clone();
    let press = (!chat.session.busy).then(|| {
        cx.listener(move |chat, _event: &(), window, cx| {
            cx.notify();
            chat.choose(id.clone(), window, cx)
        })
    });
    let content = kit::spaced(
        kit::centered_row(format!("{key}/row"), children),
        kit::spacing::XS as f32,
    );
    let row = sidebar_row(
        kit::list_row(key.clone(), content, selected, press),
        &info.channel.name,
    );
    with_seats(chat, &key, row, info)
}

/// A voice room: pressing it joins its huddle; the row the reader sits in
/// is the selected one.
fn voice_button(chat: &Chat, key: &str, info: &ChannelInfo, cx: &mut Context<Chat>) -> Node {
    let p = kit::palette();
    let key = format!("{key}/voice/{}", info.channel.id);
    let mut children = vec![
        kit::nowrap(kit::colored(
            kit::text(format!("{key}/mark"), "🔊"),
            p.muted,
        )),
        kit::nowrap(kit::text(format!("{key}/name"), &info.channel.name)),
    ];
    if info.channel.archived {
        children.push(kit::nowrap(kit::caption(
            format!("{key}/archived"),
            "Archived",
        )));
    }
    let id = info.channel.id.clone();
    let press = (!chat.session.busy && !info.channel.archived).then(|| {
        cx.listener(move |_, _event: &(), _window, cx| {
            cx.notify();
            cx.host()
                .notify::<crate::api::JoinVoice>(serde_json::json!({"id": id}))
        })
    });
    let joined = chat.session.huddle_joined && chat.session.huddle_channel == info.channel.id;
    let content = kit::spaced(
        kit::centered_row(format!("{key}/row"), children),
        kit::spacing::XS as f32,
    );
    let row = sidebar_row(
        kit::list_row(key.clone(), content, joined, press),
        &info.channel.name,
    );
    with_seats(chat, &key, row, info)
}

/// The people in the room's huddle, under the room like a voice channel.
fn with_seats(chat: &Chat, key: &str, row: Node, info: &ChannelInfo) -> Node {
    if info.channel.huddle.is_empty() {
        return row;
    }
    let me = chat.me_key();
    let mut seats = vec![row];
    for (index, seat) in info.channel.huddle.iter().enumerate() {
        let key = format!("{key}/seat/{index}");
        let label = chat.names.ready().map_or_else(
            || seat.party.clone(),
            |names| names.member_label(&seat.party),
        );
        let is_you = !me.is_empty()
            && chat
                .names
                .ready()
                .is_some_and(|names| names.owns_handle(&seat.party, &me));
        let speaking = if is_you {
            chat.session.call_speaking
        } else {
            chat.session
                .call_peers
                .iter()
                .any(|peer| peer.peer == seat.node && peer.speaking && !peer.muted)
        };
        let tone = if speaking {
            Tone::Success
        } else {
            Tone::Neutral
        };
        let mut children = vec![
            kit::avatar(format!("{key}/avatar"), kit::initials(&label), tone),
            kit::nowrap(kit::secondary(format!("{key}/name"), &label)),
        ];
        let mine = match (is_you, chat.session.call_muted) {
            (true, true) => "you · muted",
            (true, false) => "you",
            (false, _) => "",
        };
        if !mine.is_empty() {
            children.push(kit::nowrap(kit::caption(format!("{key}/you"), mine)));
        }
        seats.push(kit::padded(
            kit::spaced(kit::centered_row(key, children), kit::spacing::SM as f32),
            wire::Edges {
                top: 2.,
                right: kit::spacing::SM as f32,
                bottom: 2.,
                left: 28.,
            },
        ));
    }
    kit::spaced(kit::column(format!("{key}/with-huddle"), seats), 2.)
}

/// A direct message: the peer's avatar and name, an Agent badge on software.
fn dm_button(
    chat: &Chat,
    key: &str,
    info: &ChannelInfo,
    peer: u64,
    selected: bool,
    cx: &mut Context<Chat>,
) -> Node {
    let key = format!("{key}/dm/{peer}");
    let names = chat.names.ready();
    let name = names.map_or_else(
        || format!("account {peer}"),
        |n| n.member_label(&format!("acct:{peer}")),
    );
    let unread = chat.unread(info) && !selected;
    let label = if unread {
        kit::strong(format!("{key}/name"), &name)
    } else {
        kit::text(format!("{key}/name"), &name)
    };
    let agent = names.is_some_and(|n| n.is_program(peer));
    let tone = if agent { Tone::Agent } else { Tone::Neutral };
    let mut children = vec![
        kit::avatar(format!("{key}/avatar"), kit::initials(&name), tone),
        gives_way(&format!("{key}/name-box"), label),
    ];
    if agent {
        children.push(kit::badge(format!("{key}/agent"), "Agent", Tone::Agent));
    }
    if unread {
        children.push(kit::spacer());
        children.push(unread_dot(format!("{key}/unread")));
    }
    let id = info.channel.id.clone();
    let press = (!chat.session.busy).then(|| {
        cx.listener(move |chat, _event: &(), window, cx| {
            cx.notify();
            chat.choose(id.clone(), window, cx)
        })
    });
    let content = kit::spaced(
        kit::centered_row(format!("{key}/row"), children),
        kit::spacing::SM as f32,
    );
    sidebar_row(kit::list_row(key, content, selected, press), &name)
}

/// The DM peer of the open room, when it is one.
pub fn dm_peer(chat: &Chat) -> Option<(String, bool)> {
    let room = chat.room.as_ref()?;
    let names = chat.names.ready()?;
    let peer = crate::client::dm_peer_of(chat.my_account()?, &room.id)?;
    Some((
        names.member_label(&format!("acct:{peer}")),
        names.is_program(peer),
    ))
}
