//! One Change: its header and the three tabs a reviewer lives in.
//! Conversation is chat's hidden channel; Files is the reviewer's home.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::contract::{ChangeState, Mergeability, Query, Reply, Verdict};
use crate::state::{ChangeTab, Dock, verdict_label};
use crate::ui::changes::{revision_name, state_chip};
use crate::ui::components::{
    button, chip, empty_state, heading, id, path_text, quiet, ref_label, row, short_oid,
};
use crate::ui::{commits, diff, fact, markdown, pending, scroller, staged};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some(query) = forge.change_query() else {
        return div().into_any_element();
    };
    let reply = match staged(forge, &query, "forge-change", "Reading this change…", cx, theme) {
        Ok(reply) => reply,
        Err(state) => return state,
    };
    let Reply::Change { change, .. } = reply else {
        return div().into_any_element();
    };
    let scope = crate::state::change_key(&forge.repo_name(), change.n);
    let mut column = div()
        .id(id("forge-change-detail"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header(forge, cx, theme))
        .child(pending(forge, &scope, theme));
    if let Some(form) = &forge.form {
        column = column.child(crate::ui::changes::form(form, forge, cx, theme));
    }
    let body: AnyElement = match forge.nav().change_tab {
        ChangeTab::Conversation => conversation(forge, cx, theme),
        ChangeTab::Commits => commits::log(
            forge,
            &commits::query(forge, change.from.clone()),
            "forge-change-log",
            cx,
            theme,
        ),
        ChangeTab::Files => files(forge, cx, theme),
    };
    column.child(body).into_any_element()
}

fn header(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((change, source, _, _)) = forge.change() else {
        return div().into_any_element();
    };
    let back = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_change(None, cx));
    let edit = cx.listener(|forge, _: &ClickEvent, _, cx| forge.start_edit(cx));
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| forge.close_change(cx));
    let merge = cx.listener(|forge, _: &ClickEvent, _, cx| forge.merge(cx));
    let names = forge.names.ready();
    let author = names.map_or_else(
        || crate::state::short(&abi::hex(&change.author)),
        |names| names.key(&change.author),
    );
    let refusal = forge.merge_refusal();
    let open = change.state == ChangeState::Open;
    let mine = forge.me_key().is_some_and(|key| key == change.author);
    let mut top = div()
        .id(id("forge-change-head"))
        .flex()
        .items_center()
        .gap_2()
        .child(button(id("forge-change-back"), "← Changes", theme, back))
        .child(heading(
            id("forge-change-title"),
            format!("#{} {}", change.n, change.title),
            1,
            theme,
        ))
        .child(state_chip(change.state, change.n, theme))
        .child(quiet(
            format!(
                "{} → {} · {author}",
                ref_label(&revision_name(&change.from)),
                ref_label(&change.into)
            ),
            theme,
        ))
        .child(div().flex_1());
    if let Some(head) = source {
        top = top.child(quiet(format!("head {}", short_oid(head)), theme));
    }
    top = top
        .child(button(id("forge-edit-change"), "Edit", theme, edit).enabled(open && mine))
        .child(button(id("forge-close-change"), "Close", theme, close).enabled(open))
        .child(
            button(id("forge-merge"), "Merge", theme, merge)
                .primary(true)
                .enabled(refusal.is_empty() && forge.session.connected),
        );
    let mut bar = div().id(id("forge-change-tabs")).flex().gap_1();
    for tab in ChangeTab::ALL {
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_change_tab(tab, cx));
        bar = bar.child(
            button(
                id(format!("forge-change-tab-{}", tab.slug())),
                tab.label(),
                theme,
                pick,
            )
            .selected(forge.nav().change_tab == tab),
        );
    }
    bar = bar.child(div().flex_1());
    for dock in Dock::CHANGE {
        let toggle = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.toggle_dock(dock, cx));
        bar = bar.child(
            button(
                id(format!("forge-dock-{}", dock.slug())),
                dock.label(),
                theme,
                toggle,
            )
            .selected(forge.nav().dock == Some(dock)),
        );
    }
    let mut column = div()
        .id(id("forge-change-header"))
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .pt_3()
        .pb_2()
        .border_b_1()
        .border_color(theme.border)
        .child(top);
    if !refusal.is_empty() && open {
        column = column.child(
            div()
                .id(id("forge-merge-refusal"))
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(refusal),
        );
    }
    column.child(bar).into_any_element()
}

// ------------------------------------------------------------ conversation

fn conversation(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((change, _, _, reviews)) = forge.change() else {
        return div().into_any_element();
    };
    let mut column = scroller("forge-conversation");
    if !change.body.trim().is_empty() {
        column = column.child(
            div()
                .id(id("forge-change-body"))
                .p_2()
                .rounded_md()
                .bg(theme.surface)
                .child(markdown(
                    "forge-change-body-text",
                    &change.body,
                    forge.session.dark,
                )),
        );
    }
    let names = forge.names.ready();
    for review in &reviews.items {
        let author = names.map_or_else(
            || crate::state::short(&abi::hex(&review.author)),
            |names| names.key(&review.author),
        );
        let outdated = forge.outdated(&review.draft.commit_oid);
        let mut card = div()
            .id(id(format!("forge-review-{}", review.id)))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::bold(author))
                    .child(chip(
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
                        format!("at {}", short_oid(&review.draft.commit_oid)),
                        theme,
                    ))
                    .when(outdated, |element| {
                        element.child(chip(
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
        for (at, comment) in review.draft.comments.iter().enumerate() {
            card = card.child(quiet(
                format!(
                    "{}:{} — {}",
                    path_text(&comment.path),
                    comment.line,
                    comment.body
                ),
                theme,
            ));
            let _ = at;
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
    let names = forge.names.ready();
    match forge.messages.get(&change.channel) {
        None | Some(ducktape_view_guest::view::Loaded::Idle)
        | Some(ducktape_view_guest::view::Loaded::Loading(_)) => {
            quiet("Reading the conversation…", theme)
        }
        Some(ducktape_view_guest::view::Loaded::Failed(refusal)) => div()
            .id(id("forge-conversation-refused"))
            .p_2()
            .rounded_md()
            .bg(theme.danger_soft)
            .text_size(px(12.))
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
                let author = names.map_or_else(
                    || message.author.clone(),
                    |names| names.handle(&message.author),
                );
                column = column.child(
                    div()
                        .id(id(format!("forge-message-{}", message.message_id)))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .p_2()
                        .rounded_md()
                        .bg(theme.surface)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(crate::ui::bold(author))
                                .child(quiet(format!("#{}", message.seq), theme)),
                        )
                        .child(div().text_size(px(13.)).child(message.text.clone())),
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
                .rounded_md()
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
                .primary(true)
                .enabled(forge.session.connected && !forge.reply.trim().is_empty()),
        )
        .into_any_element()
}

// ------------------------------------------------------------------ files

fn files(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some(query) = forge.diff_query() else {
        return quiet("Comparing the endpoints…", theme);
    };
    let mut columns = div().id(id("forge-files")).flex().flex_1().min_h(px(0.));
    if forge.layout.tree_visible() {
        columns = columns.child(file_tree(forge, &query, cx, theme));
    }
    let pane = div()
        .id(id("forge-files-pane"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(review_bar(forge, cx, theme))
        .child(diff::composer(forge, cx, theme))
        .child(diff::render(
            forge,
            &query,
            "forge-diff",
            true,
            cx,
            theme,
        ));
    columns.child(pane).into_any_element()
}

/// The reviewer's file list: comment and viewed markers, single-file mode.
fn file_tree(
    forge: &Forge,
    query: &Query,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let all = cx.listener(|forge, _: &ClickEvent, _, cx| forge.single_file(None, cx));
    let column = div()
        .id(id("forge-file-tree"))
        .w(px(forge.layout.tree))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .id(id("forge-file-tree-header"))
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .child(heading(id("forge-files-title"), "Files", 2, theme))
                .child(div().flex_1())
                .child(
                    button(id("forge-files-all"), "All files", theme, all)
                        .selected(forge.nav().diff_path.is_none()),
                ),
        );
    let Some(Reply::Diff { page, .. }) = forge.ready(query) else {
        return column
            .child(quiet("Reading the diff…", theme))
            .into_any_element();
    };
    let review = forge.review();
    let published = forge.change().map(|(_, _, _, reviews)| reviews);
    let mut list = scroller("forge-file-tree-list");
    for file in &page.items {
        let Some(path) = diff::path_of(file) else {
            continue;
        };
        let label = path_text(&path);
        let viewed = forge
            .file_key(&path)
            .is_some_and(|key| forge.viewed.contains(&key));
        let drafts = review
            .map(|review| {
                review
                    .comments
                    .iter()
                    .filter(|comment| comment.path == path)
                    .count()
            })
            .unwrap_or(0);
        let landed = published
            .map(|reviews| {
                reviews
                    .items
                    .iter()
                    .flat_map(|review| review.draft.comments.iter())
                    .filter(|comment| comment.path == path)
                    .count()
            })
            .unwrap_or(0);
        let pick = cx.listener({
            let path = path.clone();
            move |forge, _: &ClickEvent, _, cx| forge.single_file(Some(path.clone()), cx)
        });
        let tick = cx.listener({
            let path = path.clone();
            move |forge, _: &ClickEvent, _, cx| forge.toggle_viewed(&path, cx)
        });
        let mut line = row(id(format!("forge-file-{label}")), theme)
            .on_click(pick)
            .selected(forge.nav().diff_path.as_deref() == Some(path.as_slice()))
            .cell(div().flex_1().truncate().child(label.clone()))
            .cell(quiet(format!("+{} −{}", file.additions, file.deletions), theme));
        if drafts + landed > 0 {
            line = line.cell(chip(
                id(format!("forge-file-comments-{label}")),
                format!("💬{}", drafts + landed),
                theme.accent_foreground,
                theme.accent_soft,
            ));
        }
        list = list.child(line).child(
            div()
                .id(id(format!("forge-viewed-{label}")))
                .px_2()
                .text_size(px(11.))
                .text_color(if viewed { theme.success } else { theme.muted })
                .role(Role::Button)
                .aria_label("Mark this file viewed")
                .aria_selected(viewed)
                .focusable()
                .on_click(tick)
                .child(if viewed { "✓ viewed" } else { "mark viewed" }),
        );
    }
    column.child(list).into_any_element()
}

/// Start review → staged comments → finish as exactly one operation.
fn review_bar(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let start = cx.listener(|forge, _: &ClickEvent, _, cx| forge.start_review(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_review(cx));
    let finishing = cx.listener(|forge, _: &ClickEvent, _, cx| forge.finishing(true, cx));
    let Some(review) = forge.review() else {
        return div()
            .id(id("forge-review-bar"))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(quiet(
                "Comment on any line by clicking its gutter number.",
                theme,
            ))
            .child(div().flex_1())
            .child(
                button(id("forge-start-review"), "Start review", theme, start)
                    .primary(true)
                    .enabled(forge.session.connected && forge.me_key().is_some()),
            )
            .into_any_element();
    };
    let mut bar = div()
        .id(id("forge-review-bar"))
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .py_1()
        .bg(theme.surface)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(quiet(
                    format!(
                        "Review pinned at {} · {} pending",
                        short_oid(&review.commit),
                        review.comments.len()
                    ),
                    theme,
                ))
                .child(div().flex_1())
                .child(button(id("forge-cancel-review"), "Discard", theme, cancel))
                .child(
                    button(id("forge-finish-review"), "Finish review", theme, finishing)
                        .primary(true)
                        .enabled(!review.finishing),
                ),
        );
    if !review.error.is_empty() {
        bar = bar.child(
            div()
                .id(id("forge-review-error"))
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(review.error.clone()),
        );
    }
    if !review.finishing {
        return bar.into_any_element();
    }
    let typed = cx.listener(|forge, text: &String, _, cx| forge.typed_review_body(text.clone(), cx));
    let mut verdicts = div().id(id("forge-verdicts")).flex().gap_2().items_center();
    for verdict in [Verdict::Approve, Verdict::RequestChanges, Verdict::Comment] {
        let submit =
            cx.listener(move |forge, _: &ClickEvent, _, cx| forge.finish_review(verdict, cx));
        verdicts = verdicts.child(
            button(
                id(format!(
                    "forge-verdict-{}",
                    match verdict {
                        Verdict::Approve => "approve",
                        Verdict::RequestChanges => "request-changes",
                        Verdict::Comment => "comment",
                    }
                )),
                verdict_label(verdict),
                theme,
                submit,
            )
            .enabled(forge.session.connected),
        );
    }
    bar.child(
        Input::new(id("forge-review-body"))
            .h(px(28.))
            .w_full()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_color(theme.foreground)
            .value(review.body.clone())
            .placeholder("What this review says overall")
            .label("Review body")
            .on_input(typed),
    )
    .child(verdicts)
    .into_any_element()
}

// ------------------------------------------------------------ dock panels

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
        .child(fact(
            "From",
            ref_label(&revision_name(&change.from)),
            theme,
        ))
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
        .child(markdown(
            "forge-overview-body",
            &change.body,
            forge.session.dark,
        ))
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
        .child(fact(
            "Comments",
            change.verdicts.comment.to_string(),
            theme,
        ));
    column = column.child(fact(
        "Mergeability",
        match forge.compare().map(|c| c.mergeability) {
            Some(Mergeability::UpToDate) => "already contained",
            Some(Mergeability::FastForward) => "fast-forward",
            Some(Mergeability::Clean) => "clean three-way",
            Some(Mergeability::Conflicts) => "conflicts",
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
    if let Some(Reply::Compare { conflicts, .. }) =
        forge.compare_query().and_then(|query| forge.ready(&query))
    {
        for conflict in &conflicts.items {
            column = column.child(quiet(
                format!("conflict: {}", path_text(&conflict.path)),
                theme,
            ));
        }
    }
    column = column.child(heading(id("forge-merge-status-title"), "Still to review", 3, theme));
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
