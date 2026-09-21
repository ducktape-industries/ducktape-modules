//! The side panes: the open thread, and the channel's details (link, rename,
//! members, archive).
use ducktape_view_guest::view::{Cx, Loaded};
use ducktape_view_guest::wire::{self, Length, Node, kit, kit::Tone};

use super::controls::*;
use super::room::{composer, list, loading, selection_bar};
use super::{close_glyph, gives_way, pane_header};
use crate::composer::send::Target;
use crate::{Chat, Pane};

pub fn thread(chat: &Chat, cx: &mut Cx<Chat>) -> Node {
    let key = "chat/thread";
    let room = chat.room.as_ref().expect("a room");
    let thread = room.thread.as_ref().expect("a thread");
    let room_label = match super::sidebar::dm_peer(chat) {
        Some((peer, _)) => peer,
        None => format!(
            "#{}",
            chat.room_info()
                .map(|i| i.channel.name.clone())
                .unwrap_or_default()
        ),
    };
    let close = close_glyph(&format!("{key}/close"), "Close thread", cx, |chat, _| {
        chat.close_thread()
    });
    let mut children = vec![
        pane_header(
            &format!("{key}/header"),
            [
                fill_width(kit::spaced(
                    kit::centered_row(
                        format!("{key}/title-row"),
                        [
                            kit::nowrap(kit::heading(format!("{key}/title"), "Thread")),
                            gives_way(
                                &format!("{key}/room-box"),
                                kit::caption(format!("{key}/room"), room_label),
                            ),
                        ],
                    ),
                    kit::spacing::SM as f32,
                )),
                close,
            ],
        ),
        kit::divider(format!("{key}/header-rule")),
    ];
    let messages = chat.messages(Pane::Thread);
    match &thread.replies {
        Loaded::Loading(_) if messages.len() <= 1 => {
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
        _ => {}
    }
    children.push(list(
        chat,
        super::super::room::THREAD_KEY,
        &messages,
        Pane::Thread,
        None,
        cx,
    ));
    if thread.has_more {
        let more = (!thread.more_loading && !chat.session.busy)
            .then(|| cx.on(|chat, cx| chat.load_more_replies(cx)));
        children.push(padded_all(
            subtle(format!("{key}/more"), "Load more replies", more),
            kit::spacing::SM as f32,
        ));
    }
    if chat.copy.is_some_and(|c| c.pane == Pane::Thread) {
        children.push(selection_bar(chat, key, cx));
    }
    children.extend(super::menu::editing(chat, Pane::Thread, cx));
    if chat.may_write() {
        let target = Target::Post {
            channel: room.id.clone(),
            thread: Some(thread.root),
        };
        let editable = chat.session.connected && !thread.replies.is_loading();
        children.push(kit::padded(
            composer(chat, target, "Reply in thread", editable, cx),
            wire::Edges {
                top: kit::spacing::XXS as f32,
                right: 16.,
                bottom: kit::spacing::LG as f32,
                left: 16.,
            },
        ));
    }
    kit::pane(
        key,
        fill(kit::spaced(
            kit::column(format!("{key}/content"), children),
            0.,
        )),
        Length::Fixed(chat.layout.thread),
    )
}

pub fn details(chat: &Chat, cx: &mut Cx<Chat>) -> Node {
    let key = "chat/details";
    let details = chat.details.as_ref().expect("details open");
    let info = chat.room_info();
    let name = info.map(|i| i.channel.name.clone()).unwrap_or_default();
    let archived = info.is_some_and(|i| i.channel.archived);
    let busy = chat.session.busy;
    let mut title = vec![kit::nowrap(kit::heading(
        format!("{key}/name"),
        format!("#{name}"),
    ))];
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
    let link = crate::files::channel_link(chat.session.chain(), &chat.room_id(), None);
    let copy = (!link.is_empty())
        .then(|| cx.on(move |chat, cx| chat.copy_text(link.clone(), "Channel link copied", cx)));
    let about = kit::column(
        format!("{key}/about"),
        [
            kit::spaced(
                kit::centered_row(format!("{key}/title-row"), title),
                kit::spacing::SM as f32,
            ),
            kit::row(
                format!("{key}/link-row"),
                [glyph(
                    format!("{key}/link"),
                    "🔗 Copy link",
                    "Copy channel link",
                    copy,
                )],
            ),
        ],
    );

    let typed = cx.on_value(|chat, text: String, _| {
        if let Some(d) = &mut chat.details {
            d.name_draft = text;
        }
    });
    let rename = cx.on(|chat, cx| chat.rename(cx));
    let rename_ok = !busy && !details.name_draft.trim().is_empty();
    let rename_section = kit::column(
        format!("{key}/rename"),
        [
            kit::label(format!("{key}/name-label"), "Name"),
            kit::row(
                format!("{key}/rename-row"),
                [
                    field(
                        format!("{key}/name-input"),
                        "Channel name",
                        &details.name_draft,
                        typed,
                        Some(rename),
                        busy,
                    ),
                    action(
                        format!("{key}/rename-button"),
                        "Rename",
                        rename_ok.then_some(rename),
                    ),
                ],
            ),
        ],
    );

    let typed = cx.on_value(|chat, text: String, _| {
        if let Some(d) = &mut chat.details {
            d.member_draft = text;
        }
    });
    let add = cx.on(|chat, cx| {
        let text = chat
            .details
            .as_ref()
            .map(|d| d.member_draft.clone())
            .unwrap_or_default();
        chat.set_member(&text, true, cx);
    });
    let add_ok = !busy && !details.member_draft.trim().is_empty();
    let mut members = vec![
        kit::label(format!("{key}/members-label"), "Members"),
        kit::row(
            format!("{key}/add-row"),
            [
                field(
                    format!("{key}/member-input"),
                    "Member account or public key",
                    &details.member_draft,
                    typed,
                    Some(add),
                    busy,
                ),
                action(format!("{key}/add-member"), "Add", add_ok.then_some(add)),
            ],
        ),
    ];
    let roster = chat.members();
    if roster.is_empty() {
        members.push(kit::wrapping(kit::secondary(
            format!("{key}/no-members"),
            "No members added. An open channel needs none — membership only gates posting in a members-only channel.",
        )));
    }
    for member in roster {
        let row_key = format!("{key}/member/{}", member.key);
        let party = member.key.clone();
        let remove = (!busy).then(|| cx.on(move |chat, cx| chat.set_member(&party, false, cx)));
        let mut button = subtle(format!("{row_key}/remove"), "Remove", remove);
        if let Node::Button {
            description, label, ..
        } = &mut button
        {
            *label = Some("Remove member".into());
            *description = Some(member.label.clone());
        }
        members.push(kit::centered_row(
            row_key.clone(),
            [
                kit::wrapping(kit::text(format!("{row_key}/name"), member.label)),
                button,
            ],
        ));
    }

    let archive = (!busy).then(|| cx.on(move |chat, cx| chat.set_archived(!archived, cx)));
    let lifecycle = kit::column(
        format!("{key}/lifecycle"),
        [
            kit::wrapping(kit::secondary(
                format!("{key}/archive-help"),
                if archived {
                    "An archived channel keeps its history and takes no new messages."
                } else {
                    "Archiving keeps the history and closes the channel to new messages."
                },
            )),
            action(
                format!("{key}/archive"),
                if archived {
                    "Unarchive channel"
                } else {
                    "Archive channel"
                },
                archive,
            ),
        ],
    );
    let close = close_glyph(
        &format!("{key}/close"),
        "Close channel details",
        cx,
        |chat, _| chat.details = None,
    );
    let children = vec![
        pane_header(
            &format!("{key}/header"),
            [
                fill_width(kit::heading(format!("{key}/title"), "Channel details")),
                close,
            ],
        ),
        kit::divider(format!("{key}/header-rule")),
        kit::scroll(
            format!("{key}/scroll"),
            padded_all(
                kit::spaced(
                    kit::column(
                        format!("{key}/content"),
                        [
                            about,
                            kit::divider(format!("{key}/rule-1")),
                            rename_section,
                            kit::divider(format!("{key}/rule-2")),
                            kit::column(format!("{key}/members"), members),
                            kit::divider(format!("{key}/rule-3")),
                            lifecycle,
                        ],
                    ),
                    16.,
                ),
                16.,
            ),
        ),
    ];
    kit::pane(
        key,
        fill(kit::spaced(
            kit::column(format!("{key}/pane-content"), children),
            0.,
        )),
        Length::Fixed(chat.layout.details),
    )
}
