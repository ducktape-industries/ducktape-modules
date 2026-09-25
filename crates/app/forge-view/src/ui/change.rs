//! One Change: its header and the three tabs a reviewer lives in.
//! Conversation is chat's hidden channel; Files is the reviewer's home.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::state::{ChangeTab, Dock, verdict_label};
use crate::ui::changes::{revision_name, state_chip};
use crate::ui::components::{
    badge, button, heading, id, path_text, quiet, ref_label, row, short_hex,
};
use crate::ui::{commits, diff, pending, scroller, staged};
use forge::{ChangeState, Query, Reply, Verdict};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some(query) = forge.change_query() else {
        return div().into_any_element();
    };
    let reply = match staged(
        forge,
        &query,
        "forge-change",
        "Reading this change…",
        cx,
        theme,
    ) {
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
        ChangeTab::Conversation => crate::ui::conversation::render(forge, cx, theme),
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
    let author = forge.key_name(&change.author);
    let blocked = forge.merge_block();
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
        top = top.child(quiet(format!("head {}", short_hex(head)), theme));
    }
    // an ended change has nothing left to edit or close
    if open {
        top = top
            .child(button(id("forge-edit-change"), "Edit", theme, edit).enabled(mine))
            .child(button(id("forge-close-change"), "Close", theme, close));
    }
    top = top.child(
        button(id("forge-merge"), "Merge", theme, merge)
            .kind(design::Kind::Primary)
            .enabled(blocked.is_none() && forge.session.connected),
    );
    let mut bar = div().id(id("forge-change-tabs")).flex().gap_1();
    for tab in ChangeTab::ALL {
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_change_tab(tab, cx));
        bar = bar.child(design::tab(
            id(format!("forge-change-tab-{}", tab.slug())),
            tab.label(),
            forge.nav().change_tab == tab,
            theme,
            pick,
        ));
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
    if let Some(blocked) = blocked {
        column = column.child(
            div()
                .id(id("forge-merge-refusal"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(blocked.sentence()),
        );
    }
    column.child(bar).into_any_element()
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
        .child(diff::render(forge, &query, "forge-diff", true, cx, theme));
    columns.child(pane).into_any_element()
}

/// The reviewer's file list: comment and viewed markers, single-file mode.
fn file_tree(forge: &Forge, query: &Query, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
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
            .cell(quiet(
                format!("+{} −{}", file.additions, file.deletions),
                theme,
            ));
        if drafts + landed > 0 {
            line = line.cell(badge(
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
                .text_size(design::text::CAPTION)
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
                    .kind(design::Kind::Primary)
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
                        short_hex(&review.commit),
                        review.comments.len()
                    ),
                    theme,
                ))
                .child(div().flex_1())
                .child(button(id("forge-cancel-review"), "Discard", theme, cancel))
                .child(
                    button(id("forge-finish-review"), "Finish review", theme, finishing)
                        .kind(design::Kind::Primary)
                        .enabled(!review.finishing),
                ),
        );
    if !review.error.is_empty() {
        bar = bar.child(
            div()
                .id(id("forge-review-error"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(review.error.clone()),
        );
    }
    if !review.finishing {
        return bar.into_any_element();
    }
    let typed =
        cx.listener(|forge, text: &String, _, cx| forge.typed_review_body(text.clone(), cx));
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
            .h(design::size::CONTROL)
            .w_full()
            .px_2()
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
