//! A change's conversation: its body, its reviews, and the replies in
//! chat's hidden channel beneath them, with a composer at the end.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::state::verdict_label;
use crate::ui::components::{badge, button, empty_state, id, path_text, quiet, short_hex};
use crate::ui::scroller;
use forge::Verdict;

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((change, _, _, reviews)) = forge.change() else {
        return div().into_any_element();
    };
    let mut column = scroller("forge-conversation");
    if !change.body.trim().is_empty() {
        column = column.child(
            div()
                .id(id("forge-change-body"))
                .p_2()
                .bg(theme.surface)
                .child(crate::ui::markdown::render(
                    "forge-change-body-text",
                    &change.body,
                    theme,
                    &crate::ui::code::links(Vec::new(), cx),
                )),
        );
    }
    for review in &reviews.items {
        let author = forge.key_name(&review.author);
        let outdated = forge.outdated(&review.draft.commit_oid);
        let mut card = div()
            .id(id(format!("forge-review-{}", review.id)))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::bold(author))
                    .child(badge(
                        id(format!("forge-review-verdict-{}", review.id)),
                        verdict_label(review.draft.verdict),
                        match review.draft.verdict {
                            Verdict::Approve => theme.success,
                            Verdict::RequestChanges => theme.danger,
                            Verdict::Comment => theme.muted,
                        },
                        match review.draft.verdict {
                            Verdict::Approve => theme.success_soft,
                            Verdict::RequestChanges => theme.danger_soft,
                            Verdict::Comment => theme.surface_raised,
                        },
                    ))
                    .child(quiet(
                        format!("at {}", short_hex(&review.draft.commit_oid)),
                        theme,
                    ))
                    .when(outdated, |element| {
                        element.child(badge(
                            id(format!("forge-review-outdated-{}", review.id)),
                            "outdated",
                            theme.warning,
                            theme.warning_soft,
                        ))
                    }),
            );
        if !review.draft.body.trim().is_empty() {
            card = card.child(quiet(review.draft.body.clone(), theme));
        }
        for comment in &review.draft.comments {
            card = card.child(quiet(
                format!(
                    "{}:{} — {}",
                    path_text(&comment.path),
                    comment.line,
                    comment.body
                ),
                theme,
            ));
        }
        column = column.child(card);
    }
    column = column.child(messages(forge, theme));
    column.child(composer(forge, cx, theme)).into_any_element()
}

/// The hidden chat channel of this change, in chat's row shape.
fn messages(forge: &Forge, theme: &Theme) -> AnyElement {
    let Some((change, _, _, _)) = forge.change() else {
        return div().into_any_element();
    };
    match forge.messages.get(&change.channel) {
        None
        | Some(ducktape_view_guest::view::Loaded::Idle)
        | Some(ducktape_view_guest::view::Loaded::Loading(_)) => {
            quiet("Reading the conversation…", theme)
        }
        Some(ducktape_view_guest::view::Loaded::Failed(refusal)) => div()
            .id(id("forge-conversation-refused"))
            .p_2()
            .bg(theme.danger_soft)
            .text_size(design::text::SECONDARY)
            .child(refusal.sentence.clone())
            .into_any_element(),
        Some(ducktape_view_guest::view::Loaded::Ready(rows)) if rows.is_empty() => empty_state(
            id("forge-conversation-empty"),
            "No replies yet",
            "This change's channel is quiet.",
            theme,
        )
        .into_any_element(),
        Some(ducktape_view_guest::view::Loaded::Ready(rows)) => {
            let mut column = div()
                .id(id("forge-conversation-messages"))
                .flex()
                .flex_col()
                .gap_2();
            for message in rows {
                let author = forge.handle_name(&message.author);
                column = column.child(
                    div()
                        .id(id(format!("forge-message-{}", message.message_id)))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .p_2()
                        .bg(theme.surface)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(crate::ui::bold(author))
                                .child(quiet(format!("#{}", message.seq), theme)),
                        )
                        .child(
                            div()
                                .text_size(design::text::BODY)
                                .child(message.text.clone()),
                        ),
                );
            }
            column.into_any_element()
        }
    }
}

fn composer(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        forge.reply = text.clone();
        cx.notify();
    });
    let send = cx.listener(|forge, _: &ClickEvent, window, cx| forge.post_reply(window, cx));
    div()
        .id(id("forge-composer"))
        .flex()
        .gap_2()
        .items_center()
        .child(
            Input::new(id("forge-reply"))
                .h(px(30.))
                .flex_1()
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .value(forge.reply.clone())
                .placeholder("Reply in this change")
                .label("Reply")
                .on_input(typed),
        )
        .child(
            button(id("forge-reply-send"), "Send", theme, send)
                .kind(design::Kind::Primary)
                .enabled(forge.session.connected && !forge.reply.trim().is_empty()),
        )
        .into_any_element()
}
