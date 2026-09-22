//! Commits: a virtualized log of the picked ref, and one commit's own diff
//! against its first parent.
use std::rc::Rc;

use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::contract::{CommitInfo, Query, Reply};
use crate::queries::PAGE;
use crate::ui::components::{button, chip, empty_state, heading, id, quiet, short_oid};
use crate::ui::{diff, fact, staged};

pub(crate) fn query(forge: &Forge, from: crate::contract::Revision) -> Query {
    Query::Log {
        repo: forge.repo_name(),
        from,
        cursor: None,
        limit: PAGE,
    }
}

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    if let Some(oid) = forge.nav().commit.clone() {
        return detail(forge, &oid, cx, theme);
    }
    log(forge, &query(forge, forge.revision()), "forge-log", cx, theme)
}

/// One page-following log, virtualized.
pub(crate) fn log(
    forge: &Forge,
    query: &Query,
    element_id: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let reply = match staged(forge, query, element_id, "Reading the history…", cx, theme) {
        Ok(reply) => reply,
        Err(state) => return state,
    };
    let Reply::Log { page, .. } = reply else {
        return div().into_any_element();
    };
    if page.items.is_empty() {
        return empty_state(
            id(format!("{element_id}-empty")),
            "No commits",
            "This ref carries no history yet.",
            theme,
        )
        .into_any_element();
    }
    let rows: Vec<(String, String, String, String, usize)> = page
        .items
        .iter()
        .map(|commit| {
            (
                commit.oid.clone(),
                summary(commit),
                String::from_utf8_lossy(&commit.author.name).into_owned(),
                commit.author.time.to_string(),
                commit.parents.len(),
            )
        })
        .collect();
    let count = rows.len();
    let theme = *theme;
    let open: crate::ui::diff::Route<String> =
        Rc::new(cx.listener(|forge, oid: &String, _, cx| forge.open_commit(Some(oid.clone()), cx)));
    let handle = forge.log_scroll.clone();
    uniform_list(id(element_id.to_owned()), count, move |range, _, _| {
        range
            .map(|index| {
                let (oid, summary, author, time, parents) = rows[index].clone();
                let open = open.clone();
                let clicked = oid.clone();
                let mut row = crate::ui::components::row(id(format!("forge-commit-{oid}")), &theme)
                    .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
                        open(&clicked, window, app)
                    })
                    .cell(
                        div()
                            .w(px(72.))
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .child(short_oid(&oid)),
                    )
                    .cell(div().flex_1().truncate().child(summary))
                    .cell(quiet(author, &theme))
                    .cell(quiet(format!("t{time}"), &theme));
                if parents > 1 {
                    row = row.cell(chip(
                        id(format!("forge-commit-merge-{oid}")),
                        format!("{parents} parents"),
                        theme.accent_foreground,
                        theme.accent_soft,
                    ));
                }
                row
            })
            .collect::<Vec<_>>()
    })
    .track_scroll(&handle)
    .flex_1()
    .min_h(px(0.))
    .into_any_element()
}

fn summary(commit: &CommitInfo) -> String {
    String::from_utf8_lossy(&commit.message)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn detail(forge: &Forge, oid: &str, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_commit(None, cx));
    let commit = match forge.ready(&query(forge, forge.revision())) {
        Some(Reply::Log { page, .. }) => page.items.iter().find(|c| c.oid == oid),
        _ => None,
    };
    let mut column = div()
        .id(id("forge-commit-detail"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(
            div()
                .id(id("forge-commit-header"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(heading(
                    id("forge-commit-title"),
                    commit.map_or_else(|| short_oid(oid), summary),
                    2,
                    theme,
                ))
                .child(div().flex_1())
                .child(button(id("forge-commit-close"), "Back", theme, close)),
        );
    if let Some(commit) = commit {
        let message = String::from_utf8_lossy(&commit.message).into_owned();
        column = column.child(
            div()
                .id(id("forge-commit-facts"))
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .py_2()
                .child(fact("Commit", commit.oid.clone(), theme))
                .child(fact("Tree", commit.tree.clone(), theme))
                .child(fact(
                    "Parents",
                    if commit.parents.is_empty() {
                        "root commit".to_owned()
                    } else {
                        commit
                            .parents
                            .iter()
                            .map(|parent| short_oid(parent))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    theme,
                ))
                .child(fact(
                    "Author",
                    format!(
                        "{} <{}> at {}",
                        String::from_utf8_lossy(&commit.author.name),
                        String::from_utf8_lossy(&commit.author.email),
                        commit.author.time
                    ),
                    theme,
                ))
                .child(quiet(message, theme)),
        );
    }
    let diff_query = Query::Diff {
        repo: forge.repo_name(),
        base: forge.commit_parent(oid),
        head: oid.to_owned(),
        path: None,
        cursor: None,
        limit: PAGE,
    };
    column
        .child(diff::render(
            forge,
            &diff_query,
            "forge-commit-diff",
            false,
            cx,
            theme,
        ))
        .into_any_element()
}
