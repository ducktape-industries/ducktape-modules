//! Message cards and their native GPUI actions.

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, ParentElement, Styled, StyledText, Theme, UnderlineStyle, Window, div, px,
    surface, wire,
};

use crate::client::{ChatBlock, ChatMessage, SpanStyle};
use crate::ui::badge;
use crate::{Chat, Mode, Pane};
mod rich;
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
        cx.notify();
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
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
    let avatar = if message.show_author {
        div()
            .id(format!("chat-message-{id}-avatar"))
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
        div().w_7().h(px(4.)).flex_shrink_0().into_any_element()
    };
    let card = div()
        .id(format!("chat-message-{id}"))
        .relative()
        .flex()
        .gap(px(10.))
        .px_4()
        .pt(px(if message.show_author { 12. } else { 3. }))
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
        .child(avatar)
        .child(content(chat, message.clone(), pane, cx, theme));
    // Controls are siblings of the selection target: their native click must
    // not also replace the opened menu with the message-selection toolbar.
    let mut outer = div().relative().w_full().group(group.clone()).child(card);
    if !message.pending && !message.deleted {
        let rev = message.rev;
        let writable = chat.may_write();
        let mut actions = div()
            .id(format!("chat-message-{id}-actions"))
            .absolute()
            .right_2()
            .top_1()
            .flex()
            .gap_1()
            .bg(theme.background)
            // GPUI dispatches a click to every interactive element whose
            // hitbox contains it, not just the topmost one: without this,
            // a click on a button here also lands on `card`'s row-select
            // handler beneath it, which clobbers whatever this row just
            // set (e.g. `open_menu`'s mode) back to `Mode::Toolbar`.
            .occlude()
            .invisible()
            .group_hover(group, |style| style.visible());
        if chosen {
            actions = actions.visible();
        }
        if pane == Pane::Timeline && message.reply_count == 0 {
            let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
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
        let thumbs = cx.listener(move |chat, _: &ClickEvent, _, cx| {
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
        outer = outer.child(actions);
    }
    outer
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
        let mut header = div()
            .id(format!("chat-message-{}-header", message.id))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_size(px(13.))
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
                    .text_size(px(11.))
                    .text_color(theme.muted)
                    .font_family("JetBrains Mono")
                    .child(crate::client::height_label(message.height)),
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
                .id(format!("chat-message-{}-text", message.id))
                .child(message.body.clone()),
        );
    }
    if message.edited {
        body = body.child(
            div()
                .text_size(px(11.))
                .text_color(theme.muted)
                .child("edited"),
        );
    }
    if message.pending {
        body = body.child(
            div()
                .id(format!("chat-message-{}-pending", message.id))
                .text_size(px(11.))
                .text_color(theme.muted)
                .child("sending…"),
        );
    }
    if !message.reactions.is_empty() {
        let reaction_seq = message.seq;
        let mut reactions = div()
            .id(format!("chat-message-{}-reactions", message.id))
            .flex()
            .flex_wrap()
            .gap_1();
        for reaction in &message.reactions {
            let emoji = reaction.emoji.clone();
            let description = emoji.clone();
            let add = !reaction.reacted_by_me;
            let mine = reaction.reacted_by_me;
            let click = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.react(reaction_seq, emoji.clone(), add, cx)
            });
            let id = format!("chat-message-{}-reaction-{}", message.id, reaction.emoji);
            let label = format!("{} {}", reaction.emoji, reaction.count);
            reactions = reactions.child(reaction_button(
                id,
                label,
                description,
                mine,
                theme,
                chat.may_write(),
                click,
            ));
        }
        let rev = message.rev;
        let open = cx.listener(move |chat, event: &ClickEvent, window, cx| {
            let position = event.position();
            chat.layout.press = (position.x.into(), position.y.into());
            chat.open_menu(pane, reaction_seq, rev, Mode::Reactions, window, cx);
            cx.notify();
        });
        reactions = reactions.child(action_button(
            format!("chat-message-{}-reaction-add", message.id),
            "+",
            "Add reaction",
            theme,
            chat.may_write(),
            open,
        ));
        body = body.child(reactions);
    }
    if message.reply_count > 0 && pane == Pane::Timeline {
        let root = message.seq;
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.open_thread(root, cx)
        });
        body = body.child(
            div()
                .id(format!("chat-message-{}-replies", message.id))
                .flex()
                .items_center()
                .gap_1()
                .pt_1()
                .text_size(px(12.))
                .text_color(theme.accent_foreground)
                .role(ducktape_view_guest::Role::Button)
                .focusable()
                .on_click(open)
                .child(format!(
                    "{} · View thread ›",
                    plural(message.reply_count, "reply", "replies")
                )),
        );
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
                        .text_size(px(11.))
                        .text_color(theme.muted)
                        .child(plural(message.reply_count, "reply", "replies")),
                )
                .child(div().h(px(1.)).flex_1().bg(theme.border)),
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
    let id = ElementId::from(format!("chat-message-{}-block-{index}", message.id));
    match block.kind.as_str() {
        "divider" => div()
            .id(id)
            .h(px(1.))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
        "code" => {
            let mut code = div()
                .id(id)
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .bg(theme.surface);
            if !block.lang.is_empty() {
                code = code.child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.muted)
                        .child(block.lang.clone()),
                );
            }
            code.child(plain_line(
                format!("chat-message-{}-block-{index}-code", message.id).into(),
                &block.text,
                true,
            ))
            .into_any_element()
        }
        "quote" => div()
            .border_l_2()
            .border_color(theme.border_strong)
            .pl_2()
            .text_color(theme.muted)
            .child(rich_line(id.clone(), block, cx, theme))
            .into_any_element(),
        "attachment" => {
            let link = block.link.clone();
            let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.open_preview(link.clone(), cx);
            });
            let card = div()
                .id(id)
                .hover(|s| s.bg(theme.surface_raised))
                .role(ducktape_view_guest::Role::Button)
                .aria_label(format!("Open {}", block.text))
                .focusable()
                .on_click(open);
            if let Some(&(width, height)) = chat.pictures.get(&block.link)
                && width > 0
                && height > 0
            {
                let (width, height) = crate::files::picture_box(width, height);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div().flex().child(
                            card.child(
                                div()
                                    .w(px(width))
                                    .h(px(height))
                                    .overflow_hidden()
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(surface(
                                        format!(
                                            "chat-message-{}-block-{index}-picture",
                                            message.id
                                        ),
                                        "picture",
                                        vec![
                                            wire::SurfaceValue::Str(
                                                crate::files::PICTURE_SURFACE.into(),
                                            ),
                                            wire::SurfaceValue::Str(
                                                crate::files::attachment_file_path(&block.link),
                                            ),
                                        ],
                                    )),
                            ),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(block.text.clone()),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .child(
                        card.flex()
                            .items_center()
                            .gap(px(12.))
                            .py(px(8.))
                            .pl(px(16.))
                            .pr(px(14.))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .child("📄")
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(block.text.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(theme.muted)
                                            .child(crate::files::attachment_kind(&block.text)),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
        }
        _ => rich_line(id, block, cx, theme).into_any_element(),
    }
}

fn action_button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    accessible: &str,
    theme: &Theme,
    enabled: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    let control = div()
        .id(id)
        .px_1()
        .py_0p5()
        .bg(theme.surface)
        .hover(|s| s.bg(theme.surface_raised))
        .role(ducktape_view_guest::Role::Button)
        .aria_label(accessible)
        .aria_disabled(!enabled)
        .text_size(px(11.))
        .child(label.into());
    if enabled {
        control.focusable().on_click(click)
    } else {
        control
    }
}

fn reaction_button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    emoji: impl Into<String>,
    mine: bool,
    theme: &Theme,
    enabled: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    let control = div()
        .id(id)
        .px_1()
        .py_0p5()
        .bg(if mine {
            theme.accent_soft
        } else {
            theme.surface
        })
        .text_color(if mine {
            theme.accent_foreground
        } else if enabled {
            theme.foreground
        } else {
            theme.muted
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_label(if mine {
            "Remove reaction"
        } else {
            "Add reaction"
        })
        .aria_description(emoji.into())
        .aria_toggled(mine.into())
        .aria_disabled(!enabled)
        .text_size(px(11.))
        .child(label.into());
    if enabled {
        control
            .focusable()
            .hover(|style| style.bg(theme.surface_raised))
            .on_click(click)
    } else {
        control
    }
}

fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
