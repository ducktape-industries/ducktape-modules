//! Message cards and their native GPUI actions.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, ParentElement, Styled, StyledText, Theme, UnderlineStyle, Window, div, px,
};

use crate::chat::{Block, Span};
use crate::client::{ChatMessage, NameDirectory, SpanStyle};
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
        let rev = message.rev;
        let writable = chat.may_write();
        let mut actions = div()
            .id(format!("chat-message-{id}-actions"))
            .absolute()
            .right_2()
            // 22px tall at 2px: inside even a compact row (3 + 20 + 3), so
            // the bar never hangs into the next row, which paints over it
            // and is outside this row's hover
            .top(px(2.))
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
                    .child(crate::client::height_label(message.height)),
            );
        }
        body = body.child(header);
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
            let click = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
                chat.claim(event);
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
            "+",
            "",
            false,
            theme,
            chat.may_write(),
            open,
        ));
        body = body.child(reactions);
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
        .w(px(26.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background)
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.faint
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_label(accessible)
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY)
        .child(label.into());
    if enabled {
        control
            .focusable()
            .cursor_pointer()
            .hover(|s| s.bg(theme.surface_raised))
            .focus_visible(|s| s.bg(theme.surface_raised))
            .on_click(click)
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
    let emoji = emoji.into();
    // "+" has no emoji: it is the picker's door, not a toggle
    let add = emoji.is_empty();
    let mut control = div()
        .id(id)
        .h(px(22.))
        .px(px(6.))
        .flex()
        .items_center()
        .gap_1()
        .border_1()
        // the reader's own wear the strong line: accent_soft is the
        // hover grey in the calm palette, so a fill alone can't say "mine"
        .border_color(if mine { theme.accent } else { theme.border })
        .bg(if mine {
            theme.accent_soft
        } else {
            theme.background
        })
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.muted
        })
        .role(ducktape_view_guest::Role::Button)
        .aria_label(if add {
            "Add reaction"
        } else if mine {
            "Remove reaction"
        } else {
            "Add reaction"
        })
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY);
    if !add {
        control = control.aria_description(emoji).aria_toggled(mine.into());
    }
    let label: String = label.into();
    control = match label.split_once(' ') {
        // "🎉 3": the count in the data face, as every count here is
        Some((glyph, count)) => control.child(glyph.to_owned()).child(
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .child(count.to_owned()),
        ),
        None => control.child(label),
    };
    if enabled {
        control
            .focusable()
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(theme.surface_raised)
                    .border_color(theme.border_strong)
            })
            .focus_visible(|style| style.border_color(theme.accent))
            .on_click(click)
    } else {
        control
    }
}

/// Under a message with replies: how many, and the way into them, drawn as
/// the button it is.
fn replies_button(
    id: impl Into<ElementId>,
    count: u64,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static,
) -> impl IntoElement {
    let noun = if count == 1 { "reply" } else { "replies" };
    div()
        .id(id)
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(theme.surface_raised)
                .border_color(theme.border_strong)
        })
        .active(|style| style.bg(theme.accent_soft))
        .focus_visible(|style| style.border_color(theme.accent))
        .role(ducktape_view_guest::Role::Button)
        .aria_label(format!("Open thread, {count} {noun}"))
        .focusable()
        .on_click(click)
        .child(
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .child(count.to_string()),
        )
        .child(noun)
        .child(div().text_color(theme.muted).child("Open thread →"))
}
