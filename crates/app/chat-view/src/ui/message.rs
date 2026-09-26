//! Message cards and their native GPUI actions.

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, ParentElement, Styled, StyledText, Theme, UnderlineStyle, Window, div, px,
};

use crate::message::{ChatMessage, SpanStyle};
use crate::ui::badge;
use crate::{Chat, Mode, Pane};
use chat::view::Names;
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
    let press = press(pane, seq, cx);
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
            design::space::HAIR
        })
        .pb(design::space::HAIR)
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
    let row_hover = hovers(key, cx);
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

/// A press on the card selects the message, unless a control on it took
/// the click first.
fn press(
    pane: Pane,
    seq: u64,
    cx: &mut Context<Chat>,
) -> impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static {
    cx.listener(move |chat, event: &ClickEvent, _window, cx| {
        let position = event.position();
        let at = (position.x.into(), position.y.into());
        // a reaction, the replies link or the toolbar took this click first
        if chat.was_claimed(at) {
            return;
        }
        cx.notify();
        chat.layout.press = at;
        chat.press_message(pane, seq);
    })
}

/// The row keeps `hovered` on itself while the pointer is over it.
fn hovers(
    key: (Pane, u64),
    cx: &mut Context<Chat>,
) -> impl Fn(&bool, &mut Window, &mut ducktape_view_guest::App) + 'static {
    cx.listener(move |chat, over: &bool, _window, cx| {
        if *over && chat.hovered != Some(key) {
            chat.hovered = Some(key);
            cx.notify();
        } else if !*over && chat.hovered == Some(key) {
            chat.hovered = None;
            cx.notify();
        }
    })
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
            .rounded(design::space::XS)
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
            .text_size(design::text::CAPTION)
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
    let actions = div()
        .id(format!("chat-message-{id}-actions"))
        .absolute()
        .right_2()
        // 22px tall at 2px: inside even a compact row (2 + 20 + 2), so
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
        .group_hover(group, |style| style.visible())
        .when(chosen, |actions| actions.visible());
    let thread = (pane == Pane::Timeline && message.reply_count == 0).then(|| {
        let open = cx.listener(move |chat, event: &ClickEvent, _, cx| {
            chat.claim(event);
            cx.notify();
            chat.open_thread(seq, cx);
        });
        let id = format!("chat-message-{id}-thread");
        action_button(id, "💬", "Open thread", theme, true, open)
    });
    let thumbs = cx.listener(move |chat, event: &ClickEvent, _, cx| {
        chat.claim(event);
        cx.notify();
        chat.react(seq, "👍".into(), true, cx);
    });
    let react = opens_menu(pane, seq, rev, Mode::Reactions, cx);
    let more = opens_menu(pane, seq, rev, Mode::More, cx);
    actions
        .children(thread)
        .child(action_button(
            format!("chat-message-{id}-thumbs-up"),
            "👍",
            "React with 👍",
            theme,
            writable,
            thumbs,
        ))
        .child(action_button(
            format!("chat-message-{id}-react"),
            "😀",
            "Manage reactions",
            theme,
            writable,
            react,
        ))
        .child(action_button(
            format!("chat-message-{id}-more"),
            "⋯",
            "More message actions",
            theme,
            true,
            more,
        ))
}

/// A strip button that opens the message's menu in `mode` where it was
/// pressed.
fn opens_menu(
    pane: Pane,
    seq: u64,
    rev: u32,
    mode: Mode,
    cx: &mut Context<Chat>,
) -> impl Fn(&ClickEvent, &mut Window, &mut ducktape_view_guest::App) + 'static {
    cx.listener(move |chat, event: &ClickEvent, window, cx| {
        chat.claim(event);
        cx.notify();
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
        chat.open_menu(pane, seq, rev, mode, window, cx);
    })
}

fn content(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> impl IntoElement {
    let reactions = (!message.reactions.is_empty())
        .then(|| reactions(chat, &message, pane, cx, theme).into_any_element());
    div()
        .id(format!("chat-message-{}-contents", message.id))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap_1()
        .when(message.show_author, |body| {
            body.child(header(&message, cx, theme))
        })
        .children(blocks(chat, &message, cx, theme))
        .children(marks(&message, theme))
        .children(reactions)
        .children(replies(&message, pane, cx, theme))
}

/// The message's blocks, or its plain body where it has none.
fn blocks(
    chat: &Chat,
    message: &ChatMessage,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Vec<AnyElement> {
    if let Some((program, code)) = &message.system {
        return vec![program_post(chat, message, program, code, cx, theme)];
    }
    if message.blocks.is_empty() {
        let text = div()
            .id(format!("chat-message-{}-text", message.id))
            .child(message.body.clone());
        return vec![text.into_any_element()];
    }
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let blocks = message.blocks.iter().enumerate();
    blocks
        .map(|(index, block)| {
            block_view(message, index, block, names, cx, theme).into_any_element()
        })
        .collect()
}

/// "edited", and "sending…" while the message is on its way.
fn marks(message: &ChatMessage, theme: &Theme) -> Vec<AnyElement> {
    let mark = |text: &'static str| {
        div()
            .text_size(design::text::CAPTION)
            .text_color(theme.muted)
            .child(text)
    };
    let mut marks = Vec::new();
    if message.edited {
        marks.push(mark("edited").into_any_element());
    }
    if message.pending {
        let id = format!("chat-message-{}-pending", message.id);
        marks.push(mark("sending…").id(id).into_any_element());
    }
    marks
}

/// A root's replies: in the timeline, the button into its thread; atop
/// the thread pane, the count over a rule.
fn replies(
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Option<AnyElement> {
    if message.reply_count == 0 {
        return None;
    }
    if pane == Pane::Timeline {
        let root = message.seq;
        let open = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
            chat.claim(event);
            cx.notify();
            chat.open_thread(root, cx)
        });
        let id = format!("chat-message-{}-replies", message.id);
        let button = replies_button(id, message.reply_count, theme, open);
        return Some(div().flex().pt_1().child(button).into_any_element());
    }
    let separator = div()
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
        .child(div().h(px(1.)).flex_1().bg(theme.border));
    Some(separator.into_any_element())
}

/// A run's first message names its author, what the author is (an agent
/// and its manager, a module) and its block.
fn header(message: &ChatMessage, cx: &mut Context<Chat>, theme: &Theme) -> impl IntoElement {
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
    if let Some(label) = &message.badge {
        let (foreground, background) = match message.agent {
            true => (theme.agent, theme.agent_soft),
            false => (theme.muted, theme.surface_raised),
        };
        header = header.child(badge(
            format!("chat-message-{}-badge", message.id),
            label.clone(),
            foreground,
            background,
        ));
    }
    if message.height > 0 {
        // the link opens Explorer at its block, and the card under it
        // stays unchosen
        let link = design::explorer::link(&design::explorer::block_path(message.height));
        let open = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
            chat.claim(event);
            cx.host().open_link(&link);
        });
        let id = format!("chat-message-{}-height", message.id);
        header = header.child(design::block_link(id, message.height, theme).on_click(open));
    }
    header
}

/// A program's own post: its event code, quiet and mono, and (on the first
/// of a run) a link to where the program itself shows the room.
fn program_post(
    chat: &Chat,
    message: &ChatMessage,
    program: &str,
    code: &str,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let mut line = div()
        .id(format!("chat-message-{}-program", message.id))
        .flex()
        .items_center()
        .gap(design::space::SM)
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(design::mono(code.to_owned()));
    // one link per run of the program's lines: the room is the same
    let link = message
        .show_author
        .then(|| crate::links::program_link(&chat.session.chain_id, &chat.room_id()))
        .flatten();
    if let Some(link) = link {
        let open = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
            chat.claim(event);
            chat.open_link(link.clone(), cx);
        });
        let theme = *theme;
        line = line.child(
            div()
                .id(format!("chat-message-{}-program-open", message.id))
                .text_color(theme.accent)
                .cursor_pointer()
                .hover(move |style| style.text_decoration_1())
                .role(ducktape_view_guest::Role::Link)
                .focusable()
                .on_click(open)
                .child(format!("Open in {program}")),
        );
    }
    line.into_any_element()
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
    names: &Names,
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
