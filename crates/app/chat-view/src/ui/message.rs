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
            .id(ElementId::Name(format!("chat-message-{id}-avatar").into()))
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
            .text_color(if message.agent { theme.agent } else { theme.muted })
            .font_semibold()
            .text_size(px(11.5))
            .child(message.initial.clone())
            .into_any_element()
    } else {
        div().w_7().h(px(4.)).flex_shrink_0().into_any_element()
    };
    let card = div()
        .id(ElementId::Name(format!("chat-message-{id}").into()))
        .relative()
        .flex()
        .gap(px(10.))
        .px_4()
        .pt(px(if message.show_author { 12. } else { 3. }))
        .pb(px(3.))
        .rounded_md()
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
        .child(content(chat, message.clone(), cx, theme));
    // Controls are siblings of the selection target: their native click must
    // not also replace the opened menu with the message-selection toolbar.
    let mut outer = div().relative().w_full().group(group.clone()).child(card);
    if !message.pending && !message.deleted {
        let rev = message.rev;
        let writable = chat.may_write();
        let mut actions = div()
            .id(ElementId::Name(format!("chat-message-{id}-actions").into()))
            .absolute()
            .right_2()
            .top_1()
            .flex()
            .gap_1()
            .bg(theme.background)
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
                ElementId::Name(format!("chat-message-{id}-thread").into()),
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
            ElementId::Name(format!("chat-message-{id}-thumbs-up").into()),
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
            ElementId::Name(format!("chat-message-{id}-react").into()),
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
            ElementId::Name(format!("chat-message-{id}-more").into()),
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
            .child(div().text_size(px(13.)).font_medium().child(message.author.clone()));
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
                    .text_size(px(11.))
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
        body = body.child(div().text_size(px(11.)).text_color(theme.muted).child("edited"));
    }
    if message.pending {
        body = body.child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-pending", message.id).into(),
                ))
                .text_size(px(11.))
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
                cx.notify();
                chat.react(reaction_seq, emoji.clone(), add, cx)
            });
            let id = ElementId::Name(
                format!("chat-message-{}-reaction-{}", message.id, reaction.emoji).into(),
            );
            let label = format!("{} {}", reaction.emoji, reaction.count);
            reactions = if chat.may_write() {
                reactions.child(action_button(
                    id,
                    label,
                    if add {
                        "Add reaction"
                    } else {
                        "Remove reaction"
                    },
                    theme,
                    true,
                    click,
                ))
            } else {
                reactions.child(
                    div()
                        .id(id)
                        .px_1()
                        .py_0p5()
                        .rounded_sm()
                        .bg(theme.surface)
                        .text_size(px(11.))
                        .child(label),
                )
            };
        }
        body = body.child(reactions);
    }
    if message.reply_count > 0 {
        let root = message.seq;
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.open_thread(root, cx)
        });
        body = body.child(
            div()
                .id(ElementId::Name(
                    format!("chat-message-{}-replies", message.id).into(),
                ))
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
        "code" => {
            let mut code = div()
                .id(id)
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
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
                ElementId::Name(format!("chat-message-{}-block-{index}-code", message.id).into()),
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
            let mut card = div()
                .id(id)
                .p_2()
                .rounded_md()
                .bg(theme.surface)
                .hover(|s| s.bg(theme.surface_raised))
                .role(ducktape_view_guest::Role::Button)
                .focusable()
                .on_click(open)
                .child(format!("📄 {}", block.text));
            if let Some(&(width, height)) = chat.pictures.get(&block.link)
                && width > 0
                && height > 0
            {
                let (width, height) = crate::files::picture_box(width, height);
                card = card.child(div().w(px(width)).h(px(height)).overflow_hidden().border_1().border_color(theme.border).rounded_md().child(surface(
                    ElementId::Name(
                        format!("chat-message-{}-block-{index}-picture", message.id).into(),
                    ),
                    "picture",
                    vec![
                        wire::SurfaceValue::Str(crate::files::PICTURE_SURFACE.into()),
                        wire::SurfaceValue::Str(crate::files::attachment_file_path(&block.link)),
                    ],
                )));
            }
            card.into_any_element()
        }
        _ => rich_line(id, block, cx, theme).into_any_element(),
    }
}

fn plain_line(id: ElementId, text: &str, mono: bool) -> InteractiveText {
    let styled = StyledText::new(text.to_owned());
    let mut text = InteractiveText::new(id, styled).w_full();
    if mono {
        text = text.font_family("JetBrains Mono").text_size(px(12.));
    }
    text
}

fn rich_line(
    id: ElementId,
    block: &ChatBlock,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> InteractiveText {
    if block.spans.is_empty() {
        return plain_line(id, &block.text, false);
    }
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut clickable = Vec::new();
    let mut targets = Vec::new();
    for span in &block.spans {
        let start = text.len();
        text.push_str(&span.text);
        let range = start..text.len();
        let mut style = HighlightStyle::default();
        match &span.style {
            SpanStyle::Plain => {}
            SpanStyle::Bold => style.font_weight = Some(FontWeight::BOLD),
            SpanStyle::Italic => style.font_style = Some(FontStyle::Italic),
            SpanStyle::BoldItalic => {
                style.font_weight = Some(FontWeight::BOLD);
                style.font_style = Some(FontStyle::Italic);
            }
            SpanStyle::Link(target) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
                if !target.is_empty() {
                    clickable.push(range.clone());
                    targets.push(target.clone());
                }
            }
            SpanStyle::Mention(account) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                if !account.is_empty() {
                    clickable.push(range.clone());
                    targets.push(account.clone());
                }
            }
        }
        highlights.push((range, style));
    }
    let styled = StyledText::new(text).with_highlights(highlights);
    let open = cx.processor(move |chat, index: usize, _window, cx| {
        if let Some(target) = targets.get(index) {
            cx.notify();
            chat.open_link(target.clone(), cx);
        }
    });
    InteractiveText::new(id, styled)
        .w_full()
        .on_click(clickable, open)
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
        .rounded_sm()
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

fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
