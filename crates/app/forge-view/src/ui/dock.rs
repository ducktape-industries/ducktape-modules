//! The three panels a Change docks, one at a time: its body, every line
//! comment in one place, and what stands between it and its target ref.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::ui::changes::revision_name;
use crate::ui::components::{empty_state, heading, id, path_text, quiet, ref_label, short_oid};
use crate::ui::{fact, prose};
use forge::Mergeability;

pub(crate) fn overview(forge: &Forge, theme: &Theme) -> AnyElement {
    let Some((change, source, target, _)) = forge.change() else {
        return quiet("Reading this change…", theme);
    };
    div()
        .id(id("forge-overview"))
        .flex()
        .flex_col()
        .gap_2()
        .child(fact("Number", format!("#{}", change.n), theme))
        .child(fact("From", ref_label(&revision_name(&change.from)), theme))
        .child(fact("Into", ref_label(&change.into), theme))
        .child(fact(
            "Source head",
            source.clone().map_or("gone".into(), |oid| short_oid(&oid)),
            theme,
        ))
        .child(fact(
            "Target head",
            target.clone().map_or("gone".into(), |oid| short_oid(&oid)),
            theme,
        ))
        .child(fact("Reviews", change.review_count.to_string(), theme))
        .child(fact("Comments", change.comment_count.to_string(), theme))
        .child(fact("Channel", change.channel.clone(), theme))
        .child(prose("forge-overview-body", &change.body))
        .into_any_element()
}

/// Every thread of this change in one place, each jumping to its line.
pub(crate) fn comments(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((_, _, _, reviews)) = forge.change() else {
        return quiet("Reading this change…", theme);
    };
    let names = forge.names.ready();
    let mut column = div().id(id("forge-comments")).flex().flex_col().gap_2();
    let mut any = false;
    if let Some(review) = forge.review() {
        for staged in &review.comments {
            any = true;
            column = column.child(
                div()
                    .id(id(format!("forge-comment-draft-{}", staged.anchor())))
                    .p_2()
                    .rounded_md()
                    .bg(theme.accent_soft)
                    .text_size(px(12.))
                    .child(format!("pending · {} — {}", staged.anchor(), staged.body)),
            );
        }
    }
    for review in &reviews.items {
        let author = names.map_or_else(
            || crate::state::short(&abi::hex(&review.author)),
            |names| names.key(&review.author),
        );
        for comment in &review.draft.comments {
            any = true;
            let path = comment.path.clone();
            let jump = cx.listener(move |forge, _: &ClickEvent, _, cx| {
                forge.single_file(Some(path.clone()), cx)
            });
            column = column.child(
                div()
                    .id(id(format!(
                        "forge-comment-{}-{}-{}",
                        review.id,
                        path_text(&comment.path),
                        comment.line
                    )))
                    .p_2()
                    .rounded_md()
                    .bg(theme.surface_raised)
                    .text_size(px(12.))
                    .role(Role::Button)
                    .focusable()
                    .on_click(jump)
                    .child(format!(
                        "{author} · {}:{} — {}",
                        path_text(&comment.path),
                        comment.line,
                        comment.body
                    )),
            );
        }
    }
    if !any {
        return empty_state(
            id("forge-comments-empty"),
            "No line comments",
            "Nobody has written on a line of this change yet.",
            theme,
        )
        .into_any_element();
    }
    column.into_any_element()
}

pub(crate) fn merge_status(forge: &Forge, theme: &Theme) -> AnyElement {
    let Some((change, _, _, _)) = forge.change() else {
        return quiet("Reading this change…", theme);
    };
    let names = forge.names.ready();
    let mut column = div()
        .id(id("forge-merge-status"))
        .flex()
        .flex_col()
        .gap_2()
        .child(fact(
            "Approvals",
            change.verdicts.approve.to_string(),
            theme,
        ))
        .child(fact(
            "Changes requested",
            change.verdicts.request_changes.to_string(),
            theme,
        ))
        .child(fact("Comments", change.verdicts.comment.to_string(), theme));
    column = column.child(fact(
        "Mergeability",
        match forge.compare().map(|c| c.mergeability) {
            Some(Mergeability::UpToDate) => "already contained",
            Some(Mergeability::FastForward) => "fast-forward",
            Some(Mergeability::Diverged) => "diverged",
            Some(Mergeability::Unrelated) => "unrelated histories",
            None => "comparing…",
        },
        theme,
    ));
    if let Some(comparison) = forge.compare() {
        column = column.child(fact(
            "Distance",
            format!("{} ahead · {} behind", comparison.ahead, comparison.behind),
            theme,
        ));
    }
    column = column.child(heading(
        id("forge-merge-status-title"),
        "Still to review",
        3,
        theme,
    ));
    if change.reviewers.is_empty() {
        column = column.child(quiet("Nobody was asked by name.", theme));
    }
    for key in &change.reviewers {
        column = column.child(quiet(
            names.map_or_else(
                || crate::state::short(&abi::hex(key)),
                |names| names.key(key),
            ),
            theme,
        ));
    }
    column
        .child(quiet(
            "Approvals are advisory: the merge is a compare-and-set on both heads.",
            theme,
        ))
        .into_any_element()
}
