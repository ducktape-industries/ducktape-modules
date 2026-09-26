//! Commits: a virtualized log of the picked ref, and one commit's own diff
//! against its first parent.
use ducktape_view_guest::design;
use std::rc::Rc;

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, Stateful};

use crate::Forge;
use crate::queries::PAGE;
use crate::ui::components::{badge, button, empty_state, heading, id, quiet, short_hex};
use crate::ui::{diff, fact, staged};
use forge::{CommitInfo, Query, Reply};

pub(crate) fn query(forge: &Forge, from: forge::Revision) -> Query {
    Query::Log {
        repo: forge.repo_name(),
        from,
        page: PAGE,
    }
}

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    if let Some(oid) = forge.nav().commit.clone() {
        return detail(forge, &oid, cx, theme);
    }
    log(
        forge,
        &query(forge, forge.revision()),
        "forge-log",
        cx,
        theme,
    )
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
    let rows: Vec<(String, String, String, i64, usize)> = page
        .items
        .iter()
        .map(|commit| {
            (
                commit.oid.clone(),
                summary(commit),
                String::from_utf8_lossy(&commit.author.name).into_owned(),
                commit.author.time,
                commit.parents.len(),
            )
        })
        .collect();
    let count = rows.len();
    let theme = *theme;
    let open: crate::ui::diff::Route<String> =
        Rc::new(cx.listener(|forge, oid: &String, _, cx| forge.open_commit(Some(oid.clone()), cx)));
    let handle = forge.log_scroll.clone();
    crate::ui::components::rows(element_id, count, None, Some(&handle), move |index| {
        let (oid, summary, author, time, parents) = rows[index].clone();
        let open = open.clone();
        let clicked = oid.clone();
        let mut row = crate::ui::components::row(id(format!("forge-commit-{oid}")), &theme)
            .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
                open(&clicked, window, app)
            })
            .cell(
                div()
                    .w(OID_W)
                    .whitespace_nowrap()
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(short_hex(&oid)),
            )
            .cell(div().flex_1().truncate().child(summary))
            .cell(quiet(author, &theme))
            .cell(quiet(
                design::date(u64::try_from(time).unwrap_or(0).saturating_mul(1000)),
                &theme,
            ));
        if parents > 1 {
            row = row.cell(badge(
                id(format!("forge-commit-merge-{oid}")),
                format!("{parents} parents"),
                theme.accent_foreground,
                theme.accent_soft,
            ));
        }
        row.into_any_element()
    })
}

/// A commit's ids, parents, author and message.
fn facts(commit: &CommitInfo, theme: &Theme) -> Stateful<Div> {
    let parents = if commit.parents.is_empty() {
        "root commit".to_owned()
    } else {
        commit
            .parents
            .iter()
            .map(|parent| short_hex(parent))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let author = format!(
        "{} <{}> at {}",
        String::from_utf8_lossy(&commit.author.name),
        String::from_utf8_lossy(&commit.author.email),
        commit.author.time
    );
    div()
        .id(id("forge-commit-facts"))
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .child(fact("Commit", commit.oid.clone(), theme))
        .child(fact("Tree", commit.tree.clone(), theme))
        .child(fact("Parents", parents, theme))
        .child(fact("Author", author, theme))
        .child(quiet(
            String::from_utf8_lossy(&commit.message).into_owned(),
            theme,
        ))
}

/// The short commit id column.
const OID_W: Pixels = px(100.);

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
                    commit.map_or_else(|| short_hex(oid), summary),
                    2,
                    theme,
                ))
                .child(div().flex_1())
                .child(button(id("forge-commit-close"), "Back", theme, close)),
        );
    if let Some(commit) = commit {
        column = column.child(facts(commit, theme));
    }
    let diff_query = Query::Diff {
        repo: forge.repo_name(),
        base: forge.commit_parent(oid),
        head: oid.to_owned(),
        path: None,
        page: PAGE,
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
