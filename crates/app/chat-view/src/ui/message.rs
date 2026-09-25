//! Message cards and their native GPUI actions.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, ParentElement, Styled, StyledText, Theme, UnderlineStyle, Window, div, px,
};

use crate::message::{ChatMessage, SpanStyle};
use crate::names::NameDirectory;
use crate::ui::badge;
use crate::{Chat, Mode, Pane};
use chat::{Block, Span};
mod controls;
mod rich;
use controls::{Face, action_button, reaction_button, replies_button};
use rich::{plain_line, rich_line};

pub fn card(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    _window: &mut Window,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let id = message.id.clone();
    let seq = message.seq;
    let press = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
        let position = event.position();
        let at = (position.x.into(), position.y.into());
        // a reaction, the replies link or the toolbar took this click first
        if chat.was_claimed(at) {
            return;
        }
        cx.notify();
        chat.layout.press = at;
        chat.press_message(pane, seq);
    });
    let chosen = !message.deleted
        && seq > 0
        && chat
            .menu
            .as_ref()
            .is_some_and(|menu| menu.pane == pane && menu.seq == seq);
    let ranged = !message.deleted && chat.copy.is_some_and(|range| range.holds(pane, seq));
    let group: ducktape_view_guest::SharedString = format!("chat-message-{id}").into();
    let card = div()
        .id(format!("chat-message-{id}"))
        .relative()
        .flex()
        .gap(design::space::MD)
        .px_4()
        .pt(if message.show_author {
            design::space::LG
        } else {
            px(3.)
        })
        .pb(px(3.))
        .bg(if chosen {
            theme.accent_soft
        } else if ranged {
            theme.surface_raised
        } else {
            theme.background
        })
        .hover(|style| style.bg(theme.surface_raised))
        .role(ducktape_view_guest::Role::Button)
        .aria_label(format!(
            "Select message, shows its actions: {}: {}",
            message.author, message.body
        ))
        .focusable()
        .on_click(press)
        .child(avatar(&message, theme))
        .child(content(chat, message.clone(), pane, cx, theme));
    // Controls are siblings of the selection target: their native click must
    // not also replace the opened menu with the message-selection toolbar.
    // The row says when the pointer is over it, and only that row (and a
    // chosen one) carries the action strip: drawn invisible under every
    // row, the strips were most of each frame the view sends — over half
    // its bytes in a busy room — and every frame is paid for in fuel.
    let key = (pane, seq);
    let row_hover = cx.listener(move |chat, over: &bool, _window, cx| {
        if *over && chat.hovered != Some(key) {
            chat.hovered = Some(key);
            cx.notify();
        } else if !*over && chat.hovered == Some(key) {
            chat.hovered = None;
            cx.notify();
        }
    });
    let mut outer = div()
        .id(format!("chat-message-{id}-row"))
        .relative()
        .w_full()
        .group(group.clone())
        .on_hover(row_hover)
        .child(card);
    if !message.pending && !message.deleted && (chosen || chat.hovered == Some(key)) {
        outer = outer.child(action_strip(chat, &message, pane, group, chosen, cx, theme));
    }
    outer
}

/// A run's first message wears its author's initials; the rest keep
/// the column.
fn avatar(message: &ChatMessage, theme: &Theme) -> AnyElement {
    if message.show_author {
        div()
            .id(format!("chat-message-{}-avatar", message.id))
            .size_7()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .bg(if message.agent {
                theme.agent_soft
            } else {
                theme.surface_raised
            })
            .text_color(if message.agent {
                theme.agent
            } else {
                theme.muted
            })
            .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
            .text_size(px(11.5))
            .child(message.initial.clone())
            .into_any_element()
    } else {
        div()
            .w_7()
            .h(design::space::XXS)
            .flex_shrink_0()
            .into_any_element()
    }
}

/// The strip over a row the pointer is on (or a chosen one): thread,
/// 👍, the picker and the "More" menu.
fn action_strip(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    group: ducktape_view_guest::SharedString,
    chosen: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let (id, seq) = (&message.id, message.seq);
    let rev = message.rev;
    let writable = chat.may_write();
    let mut actions = div()
        .id(format!("chat-message-{id}-actions"))
        .absolute()
        .right_2()
        // 22px tall at 2px: inside even a compact row (3 + 20 + 3), so
        // the bar never hangs into the next row, which paints over it
        // and is outside this row's hover
        .top(design::space::HAIR)
        .flex()
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        // No occlude: an occluding bar took the row's hover away the
        // moment the pointer reached it, hid itself, and was never
        // clickable. GPUI hands a click here to the card beneath too;
        // each button claims it (`Chat::claim`) so the card stands down.
        .invisible()
        .group_hover(group, |style| style.visible());
    if chosen {
        actions = actions.visible();
    }
    if pane == Pane::Timeline && message.reply_count == 0 {
        let open = cx.listener(move |chat, event: &ClickEvent, _, cx| {
            chat.claim(event);
            cx.notify();
            chat.open_thread(seq, cx);
        });
        actions = actions.child(action_button(
            format!("chat-message-{id}-thread"),
            "💬",
            "Open thread",
            theme,
            true,
            open,
        ));
    }
    let thumbs = cx.listener(move |chat, event: &ClickEvent, _, cx| {
        chat.claim(event);
        cx.notify();
        chat.react(seq, "👍".into(), true, cx);
    });
    actions = actions.child(action_button(
        format!("chat-message-{id}-thumbs-up"),
        "👍",
        "React with 👍",
        theme,
        writable,
        thumbs,
    ));
    let react = cx.listener(move |chat, event: &ClickEvent, window, cx| {
        chat.claim(event);
        cx.notify();
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
        chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx);
    });
    actions = actions.child(action_button(
        format!("chat-message-{id}-react"),
        "😀",
        "Manage reactions",
        theme,
        writable,
        react,
    ));
    let more = cx.listener(move |chat, event: &ClickEvent, window, cx| {
        chat.claim(event);
        cx.notify();
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
        chat.open_menu(pane, seq, rev, Mode::More, window, cx);
    });
    actions = actions.child(action_button(
        format!("chat-message-{id}-more"),
        "⋯",
        "More message actions",
        theme,
        true,
        more,
    ));
    actions
}

fn content(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let mut body = div()
        .id(format!("chat-message-{}-contents", message.id))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap_1();
    if message.show_author {
        body = body.child(header(&message, theme));
    }
    let empty = NameDirectory::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    for (index, block) in message.blocks.iter().enumerate() {
        body = body.child(block_view(&message, index, block, names, cx, theme));
    }
    if message.blocks.is_empty() {
        body = body.child(
            div()
                .id(format!("chat-message-{}-text", message.id))
                .child(message.body.clone()),
        );
    }
    if message.edited {
        body = body.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child("edited"),
        );
    }
    if message.pending {
        body = body.child(
            div()
                .id(format!("chat-message-{}-pending", message.id))
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child("sending…"),
        );
    }
    if !message.reactions.is_empty() {
        body = body.child(reactions(chat, &message, pane, cx, theme));
    }
    if message.reply_count > 0 && pane == Pane::Timeline {
        let root = message.seq;
        let open = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
            chat.claim(event);
            cx.notify();
            chat.open_thread(root, cx)
        });
        body = body.child(div().flex().pt_1().child(replies_button(
            format!("chat-message-{}-replies", message.id),
            message.reply_count,
            theme,
            open,
        )));
    } else if message.reply_count > 0 {
        body = body.child(
            div()
                .id(format!("chat-message-{}-reply-separator", message.id))
                .flex()
                .items_center()
                .gap_2()
                .pt_1()
                .child(
                    div()
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                        .child(design::plural(message.reply_count, "reply", "replies")),
                )
                .child(div().h(px(1.)).flex_1().bg(theme.border)),
        );
    }
    body
}

/// A run's first message names its author, their agent badge and block.
fn header(message: &ChatMessage, theme: &Theme) -> impl IntoElement {
    let mut header = div()
        .id(format!("chat-message-{}-header", message.id))
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_size(design::text::BODY)
                .font_weight(ducktape_view_guest::FontWeight::MEDIUM)
                .child(message.author.clone()),
        );
    if message.agent {
        header = header.child(badge(
            format!("chat-message-{}-agent", message.id),
            "Agent",
            theme.agent,
            theme.agent_soft,
        ));
    }
    if message.height > 0 {
        header = header.child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .font_family(design::fonts::FAMILY_MONO)
                .child(crate::message::height_label(message.height)),
        );
    }
    header
}

/// The reactions under a message, and the `+` that opens the picker.
fn reactions(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let reaction_seq = message.seq;
    let mut reactions = div()
        .id(format!("chat-message-{}-reactions", message.id))
        .flex()
        .flex_wrap()
        .gap_1();
    for reaction in &message.reactions {
        let emoji = reaction.emoji.clone();
        let add = !reaction.reacted_by_me;
        let mine = reaction.reacted_by_me;
        let click = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
            chat.claim(event);
            cx.notify();
            chat.react(reaction_seq, emoji.clone(), add, cx)
        });
        let id = format!("chat-message-{}-reaction-{}", message.id, reaction.emoji);
        let face = Face::Emoji {
            emoji: &reaction.emoji,
            count: reaction.count,
        };
        reactions = reactions.child(reaction_button(
            id,
            face,
            mine,
            theme,
            chat.may_write(),
            click,
        ));
    }
    let rev = message.rev;
    let open = cx.listener(move |chat, event: &ClickEvent, window, cx| {
        // the card under this button would otherwise take the same
        // click and put the row's toolbar over the picker just opened
        chat.claim(event);
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
        chat.open_menu(pane, reaction_seq, rev, Mode::Reactions, window, cx);
        cx.notify();
    });
    reactions = reactions.child(reaction_button(
        format!("chat-message-{}-reaction-add", message.id),
        Face::Add,
        false,
        theme,
        chat.may_write(),
        open,
    ));
    reactions
}

fn block_view(
    message: &ChatMessage,
    index: usize,
    block: &Block,
    names: &NameDirectory,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = ElementId::from(format!("chat-message-{}-block-{index}", message.id));
    match block {
        Block::Divider => div()
            .id(id)
            .h(px(1.))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
        Block::Code { lang, text } => {
            let mut code = div()
                .id(id)
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .bg(theme.surface);
            if let Some(lang) = lang.as_ref().filter(|lang| !lang.is_empty()) {
                code = code.child(
                    div()
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                        .child(lang.clone()),
                );
            }
            code.child(plain_line(
                format!("chat-message-{}-block-{index}-code", message.id).into(),
                text,
                true,
            ))
            .into_any_element()
        }
        Block::Quote(spans) => div()
            .border_l_2()
            .border_color(theme.border_strong)
            .pl_2()
            .text_color(theme.muted)
            .child(rich_line(id.clone(), spans, names, cx, theme))
            .into_any_element(),
        Block::Paragraph(spans) => rich_line(id, spans, names, cx, theme).into_any_element(),
    }
}
