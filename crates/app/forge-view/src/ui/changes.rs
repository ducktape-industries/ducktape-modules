//! Changes: the filter rail with "Needs my judgment" on top, the list, and
//! the form that opens or edits one.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::contract::{ChangeState, ChangeSummary, Judgment, Reply, ReviewCounts};
use crate::state::{ChangeForm, Filter};
use crate::ui::components::{button, chip, empty_state, heading, id, quiet, ref_label, row};
use crate::ui::{pending, scroller, staged};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut column = div()
        .id(id("forge-changes"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(filters(forge, cx, theme))
        .child(pending(forge, "changes", theme));
    if let Some(form) = &forge.form {
        column = column.child(self::form(form, forge, cx, theme));
    }
    let query = forge.changes_query(&forge.repo_name());
    let reply = match staged(
        forge,
        &query,
        "forge-changes-list",
        "Reading the changes…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let rows: Vec<(ChangeSummary, Option<&Judgment>)> = match reply {
        Reply::Changes { page, .. } => page.items.iter().map(|s| (s.clone(), None)).collect(),
        Reply::Judgment { page, .. } => page
            .items
            .iter()
            .map(|item| (item.change.clone(), Some(item)))
            .collect(),
        _ => Vec::new(),
    };
    let needle = forge.search.trim().to_lowercase();
    let shown: Vec<&(ChangeSummary, Option<&Judgment>)> = rows
        .iter()
        .filter(|(summary, _)| needle.is_empty() || summary.title.to_lowercase().contains(&needle))
        .collect();
    if shown.is_empty() {
        return column
            .child(empty_state(
                id("forge-changes-empty"),
                if forge.filter == Filter::Judgment {
                    "Nothing waits on you"
                } else {
                    "No changes here"
                },
                if forge.filter == Filter::Judgment {
                    "No review is requested of you and no thread of yours has an answer."
                } else {
                    "Compare a branch from the Refs tab to open one."
                },
                theme,
            ))
            .into_any_element();
    }
    let names = forge.names.ready();
    let mut list = scroller("forge-changes-list");
    for (summary, judgment) in shown {
        let n = summary.n;
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_change(Some(n), cx));
        let author = names.map_or_else(
            || crate::state::short(&abi::hex(&summary.author)),
            |names| names.key(&summary.author),
        );
        let mut line = row(id(format!("forge-change-{n}")), theme)
            .on_click(open)
            .cell(
                div()
                    .w(px(44.))
                    .text_color(theme.muted)
                    .child(format!("#{n}")),
            )
            .cell(
                div()
                    .flex_1()
                    .truncate()
                    .child(crate::ui::bold(summary.title.clone())),
            )
            .cell(state_chip(summary.state, n, theme))
            .cell(quiet(
                format!(
                    "{} → {}",
                    ref_label(&revision_name(&summary.from)),
                    ref_label(&summary.into)
                ),
                theme,
            ))
            .cell(quiet(author, theme))
            .cell(quiet(format!("{} comments", summary.comment_count), theme))
            .cell(verdicts(&summary.verdicts, n, theme));
        if let Some(judgment) = judgment {
            if judgment.requested {
                line = line.cell(chip(
                    id(format!("forge-change-requested-{n}")),
                    "review requested",
                    theme.accent_foreground,
                    theme.accent_soft,
                ));
            }
            if judgment.replies.is_some() {
                line = line.cell(chip(
                    id(format!("forge-change-unread-{n}")),
                    "new reply",
                    theme.warning,
                    theme.warning_soft,
                ));
            }
        }
        list = list.child(line);
    }
    column.child(list).into_any_element()
}

pub(crate) fn revision_name(revision: &crate::contract::Revision) -> Vec<u8> {
    match revision {
        crate::contract::Revision::Ref(name) => name.clone(),
        crate::contract::Revision::Oid(oid) => oid.clone().into_bytes(),
    }
}

pub(crate) fn state_chip(state: ChangeState, n: u64, theme: &Theme) -> AnyElement {
    let (label, foreground, background) = match state {
        ChangeState::Open => ("open", theme.success, theme.success_soft),
        ChangeState::Merged => ("merged", theme.accent_foreground, theme.accent_soft),
        ChangeState::Closed => ("closed", theme.muted, theme.surface_raised),
    };
    chip(
        id(format!("forge-change-state-{n}")),
        label,
        foreground,
        background,
    )
    .into_any_element()
}

fn verdicts(counts: &ReviewCounts, n: u64, theme: &Theme) -> AnyElement {
    if counts.approve + counts.request_changes + counts.comment == 0 {
        return div().into_any_element();
    }
    chip(
        id(format!("forge-change-verdicts-{n}")),
        format!(
            "✓{} ✗{} 💬{}",
            counts.approve, counts.request_changes, counts.comment
        ),
        theme.muted,
        theme.surface_raised,
    )
    .into_any_element()
}

fn filters(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        forge.search = text.clone();
        cx.notify();
    });
    let mut bar = div()
        .id(id("forge-filters"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .px_3()
        .py_2();
    for filter in Filter::ALL {
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.set_filter(filter, cx));
        bar = bar.child(
            button(
                id(format!("forge-filter-{}", filter.slug())),
                filter.label(),
                theme,
                pick,
            )
            .selected(forge.filter == filter)
            .enabled(
                filter == Filter::Open
                    || filter == Filter::Merged
                    || filter == Filter::Closed
                    || forge.me_key().is_some(),
            ),
        );
    }
    bar.child(div().flex_1())
        .child(
            Input::new(id("forge-changes-search"))
                .h(px(26.))
                .w(px(220.))
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .value(forge.search.clone())
                .placeholder("Search titles")
                .label("Search changes")
                .on_input(typed),
        )
        .into_any_element()
}

/// The Change draft: a comparison turned into a title, a body and reviewers.
pub(crate) fn form(
    form: &ChangeForm,
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let title = cx.listener(|forge, text: &String, _, cx| {
        if let Some(form) = &mut forge.form {
            form.title = text.clone();
            form.error.clear();
        }
        cx.notify();
    });
    let body = cx.listener(|forge, text: &String, _, cx| {
        if let Some(form) = &mut forge.form {
            form.body = text.clone();
        }
        cx.notify();
    });
    let submit = cx.listener(|forge, _: &ClickEvent, _, cx| forge.submit_change(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_change(cx));
    let mut card = div()
        .id(id("forge-change-form"))
        .flex()
        .flex_col()
        .gap_2()
        .m_3()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .child(heading(
            id("forge-change-form-title"),
            match form.edit {
                Some(n) => format!("Edit change #{n}"),
                None => "New change".to_owned(),
            },
            2,
            theme,
        ))
        .child(quiet(
            format!("{} → {}", ref_label(&form.from), ref_label(&form.into)),
            theme,
        ))
        .child(
            Input::new(id("forge-change-title"))
                .h(px(28.))
                .w_full()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .value(form.title.clone())
                .placeholder("What this change does")
                .label("Change title")
                .on_input(title),
        )
        .child(
            Input::new(id("forge-change-body"))
                .h(px(28.))
                .w_full()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .value(form.body.clone())
                .placeholder("Why it changes")
                .label("Change body")
                .on_input(body),
        )
        .child(reviewers(form, forge, cx, theme));
    if !form.error.is_empty() {
        card = card.child(
            div()
                .id(id("forge-change-error"))
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(form.error.clone()),
        );
    }
    card.child(
        div()
            .flex()
            .gap_2()
            .child(div().flex_1())
            .child(button(id("forge-change-cancel"), "Cancel", theme, cancel))
            .child(
                button(
                    id("forge-change-submit"),
                    if form.edit.is_some() { "Save" } else { "Open" },
                    theme,
                    submit,
                )
                .primary(true)
                .enabled(forge.session.connected),
            ),
    )
    .into_any_element()
}

/// The identity picker, the same roster chat picks from.
fn reviewers(
    form: &ChangeForm,
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let Some(names) = forge.names.ready() else {
        return quiet("Reading the roster…", theme);
    };
    let mut bar = div()
        .id(id("forge-change-reviewers"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(quiet("Reviewers", theme));
    for account in names.rows().iter().take(24) {
        let Some(key) = names.key_of(account.number) else {
            continue;
        };
        let picked = form.reviewers.contains(&key);
        let toggle = cx.listener({
            let key = key.clone();
            move |forge, _: &ClickEvent, _, cx| {
                if let Some(form) = &mut forge.form {
                    if let Some(at) = form.reviewers.iter().position(|held| *held == key) {
                        form.reviewers.remove(at);
                    } else if form.reviewers.len() < crate::contract::MAX_REVIEWERS {
                        form.reviewers.push(key.clone());
                    }
                }
                cx.notify();
            }
        });
        bar = bar.child(
            button(
                id(format!("forge-reviewer-{}", account.number)),
                account.name.clone(),
                theme,
                toggle,
            )
            .selected(picked),
        );
    }
    bar.into_any_element()
}
