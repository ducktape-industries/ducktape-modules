//! Message cards and their native GPUI actions.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, ParentElement, Styled, Theme, Window, div, img, px,
};

use crate::client::{ChatBlock, ChatMessage, SpanStyle};
use crate::ui::badge;
use crate::{Chat, Mode, Pane};

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
    let press =
        cx.listener(move |chat, _: &ClickEvent, _window, _cx| chat.press_message(pane, seq));
    let mut card = div()
        .id(ElementId::Name(format!("chat-message-{}", id).into()))
        .relative()
        .flex()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(
            if chat
                .menu
                .as_ref()
                .is_some_and(|menu| menu.pane == pane && menu.seq == seq)
            {
                theme.accent_soft
            } else {
                theme.background
            },
        )
        .hover(|s| s.bg(theme.surface_raised))
        .role(ducktape_view_guest::Role::Button).focusable().on_click(press)
        .child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-avatar", id).into(),
                ))
                .size_7()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(if message.agent {
                    theme.agent_soft
                } else {
                    theme.surface
                })
                .text_xs()
                .child(message.initial.clone()),
        )
        .child(content(chat, message.clone(), pane, cx, theme));

    if !message.pending && !message.deleted {
        let seq = message.seq;
        let rev = message.rev;
        let react = cx.listener(move |chat, _: &ClickEvent, window, cx| {
            chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx)
        });
        let more = cx.listener(move |chat, _: &ClickEvent, window, cx| {
            chat.open_menu(pane, seq, rev, Mode::More, window, cx)
        });
        let mut actions = div()
            .id(ElementId::Name(
                format!("chat-message-{}-actions", message.id).into(),
            ))
            .absolute()
            .right_2()
            .top_1()
            .flex()
            .gap_1()
            .bg(theme.background)
            .child(action_button(
                ElementId::Name(format!("chat-message-{}-react", message.id).into()),
                "😀",
                "Manage reactions",
                theme,
                react,
            ))
            .child(action_button(
                ElementId::Name(format!("chat-message-{}-more", message.id).into()),
                "⋯",
                "More message actions",
                theme,
                more,
            ));
        if pane == Pane::Timeline && message.reply_count == 0 {
            let open =
                cx.listener(move |chat, _: &ClickEvent, _window, cx| chat.open_thread(seq, cx));
            actions = actions.child(action_button(
                ElementId::Name(format!("chat-message-{}-thread", message.id).into()),
                "💬",
                "Open thread",
                theme,
                open,
            ));
        }
        card = card.child(actions);
    }
    card
}

fn content(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let mut body = div()
        .id(ElementId::Name(
            format!("chat-message-{}-contents", message.id).into(),
        ))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap_1();
    if message.show_author {
        let mut header = div()
            .id(ElementId::Name(
                format!("chat-message-{}-header", message.id).into(),
            ))
            .flex()
            .items_center()
            .gap_1()
            .child(div().text_sm().child(message.author.clone()));
        if message.agent {
            header = header.child(badge(
                ElementId::Name(format!("chat-message-{}-agent", message.id).into()),
                "Agent",
                theme.agent,
                theme.agent_soft,
            ));
        }
        if message.height > 0 {
            header = header.child(
                div()
                    .text_xs()
                    .text_color(theme.muted)
                    .child(format!("block {}", message.height)),
            );
        }
        body = body.child(header);
    }
    for (index, block) in message.blocks.iter().enumerate() {
        body = body.child(block_view(chat, &message, index, block, cx, theme));
    }
    if message.blocks.is_empty() {
        body = body.child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-text", message.id).into(),
                ))
                .child(message.body.clone()),
        );
    }
    if message.edited {
        body = body.child(div().text_xs().text_color(theme.muted).child("edited"));
    }
    if message.pending {
        body = body.child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-pending", message.id).into(),
                ))
                .text_xs()
                .text_color(theme.muted)
                .child("sending…"),
        );
    }
    if !message.reactions.is_empty() {
        let reaction_seq = message.seq;
        let mut reactions = div()
            .id(ElementId::Name(
                format!("chat-message-{}-reactions", message.id).into(),
            ))
            .flex()
            .flex_wrap()
            .gap_1();
        for reaction in &message.reactions {
            let emoji = reaction.emoji.clone();
            let add = !reaction.reacted_by_me;
            let click = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.react(reaction_seq, emoji.clone(), add, cx)
            });
            reactions = reactions.child(action_button(
                ElementId::Name(
                    format!("chat-message-{}-reaction-{}", message.id, reaction.emoji).into(),
                ),
                format!("{} {}", reaction.emoji, reaction.count),
                "reaction",
                theme,
                click,
            ));
        }
        body = body.child(reactions);
    }
    if message.reply_count > 0 {
        let root = message.seq;
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| chat.open_thread(root, cx));
        body = body.child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-replies", message.id).into(),
                ))
                .flex()
                .items_center()
                .gap_1()
                .pt_1()
                .text_sm()
                .text_color(theme.accent_foreground)
                .role(ducktape_view_guest::Role::Button).focusable().on_click(open)
                .child(format!(
                    "{} · View thread ›",
                    plural(message.reply_count, "reply", "replies")
                )),
        );
    }
    body
}

fn block_view(
    chat: &Chat,
    message: &ChatMessage,
    index: usize,
    block: &ChatBlock,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = ElementId::Name(format!("chat-message-{}-block-{index}", message.id).into());
    match block.kind.as_str() {
        "divider" => div()
            .id(id)
            .h(px(1.))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
        "code" => div()
            .id(id)
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_md()
            .bg(theme.surface)
            .font_family("JetBrains Mono")
            .text_sm()
            .child(block.text.clone())
            .into_any_element(),
        "quote" => div()
            .id(id)
            .border_l_2()
            .border_color(theme.border_strong)
            .pl_2()
            .text_color(theme.muted)
            .child(block.text.clone())
            .into_any_element(),
        "attachment" => {
            let link = block.link.clone();
            let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.open_preview(link.clone(), cx);
            });
            let mut card = div()
                .id(id)
                .p_2()
                .rounded_md()
                .bg(theme.surface)
                .hover(|s| s.bg(theme.surface_raised))
                .role(ducktape_view_guest::Role::Button).focusable().on_click(open)
                .child(format!("📄 {}", block.text));
            if let Some(&(width, height)) = chat.pictures.get(&block.link)
                && width > 0
                && height > 0
            {
                card = card.child(
                    div()
                        .max_w(px(360.))
                        .max_h(px(240.))
                        .child(img(crate::files::attachment_file_path(&block.link))),
                );
            }
            card.into_any_element()
        }
        _ => {
            let mut text = div().id(id).child(block_text(block));
            if let Some(link) = block.spans.iter().find_map(|span| match &span.style {
                SpanStyle::Link(link) => Some(link.clone()),
                SpanStyle::Mention(account) => Some(account.clone()),
                _ => None,
            }) {
                let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                    chat.open_link(link.clone(), cx);
                });
                text = text.text_color(theme.link).role(ducktape_view_guest::Role::Button).focusable().on_click(open);
            }
            text.into_any_element()
        }
    }
}

fn block_text(block: &ChatBlock) -> String {
    if block.spans.is_empty() {
        return block.text.clone();
    }
    block.spans.iter().map(|span| span.text.as_str()).collect()
}

fn action_button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    _accessible: &str,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_1()
        .py_0p5()
        .rounded_sm()
        .bg(theme.surface)
        .hover(|s| s.bg(theme.surface_raised))
        .role(ducktape_view_guest::Role::Button).focusable().on_click(click)
        .text_xs()
        .child(label.into())
}

fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
