//! The repositories: the full list when nothing is open, the rail that
//! switches between them when something is.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::queries::PAGE;
use crate::ui::components::{
    button, chip, empty_state, heading, heading_in, id, quiet, ref_label, row,
};
use crate::ui::{pending, scroller, staged};
use forge::{Query, Reply, RepoInfo};

fn query() -> Query {
    Query::Repos {
        cursor: None,
        limit: PAGE,
    }
}

fn listed<'a>(forge: &'a Forge, reply: &'a Reply) -> Vec<&'a RepoInfo> {
    let Reply::Repos { page, .. } = reply else {
        return Vec::new();
    };
    let needle = forge.search.trim().to_lowercase();
    page.items
        .iter()
        .filter(|info| needle.is_empty() || info.name.to_lowercase().contains(&needle))
        .collect()
}

/// The screen: every repository of this network, newest activity first.
pub(crate) fn overview(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut column = div()
        .id(id("forge-repos"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header(forge, cx, theme, "forge-repos-search"));
    if let Some(form) = &forge.new_repo {
        column = column.child(dialog(form, cx, theme));
    }
    column = column.child(pending(forge, "repos", theme));
    let body = match staged(
        forge,
        &query(),
        "forge-repos-list",
        "Reading the repositories…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let rows = listed(forge, body);
    if rows.is_empty() {
        return column
            .child(empty_state(
                id("forge-repos-empty"),
                if forge.search.trim().is_empty() {
                    "No repositories yet"
                } else {
                    "Nothing matches"
                },
                if forge.search.trim().is_empty() {
                    "Push one into existence: `git push duck://<network>/forge/<name> main`, or create it here."
                } else {
                    "No repository here reads like that."
                },
                theme,
            ))
            .into_any_element();
    }
    let names = forge.names.ready();
    let mut list = scroller("forge-repos-list");
    for info in rows {
        let name = info.name.clone();
        let open = cx.listener({
            let name = name.clone();
            move |forge, _: &ClickEvent, _, cx| forge.open_repo(name.clone(), cx)
        });
        let owner = names.map_or_else(
            || crate::state::short(&abi::hex(&info.repo.owner)),
            |names| names.key(&info.repo.owner),
        );
        list = list.child(
            row(id(format!("forge-repo-{name}")), theme)
                .on_click(open)
                .cell(div().w(px(220.)).truncate().child(crate::ui::bold(name)))
                .cell(chip(
                    id(format!("forge-repo-head-{}", info.name)),
                    ref_label(&info.repo.settings.head),
                    theme.muted,
                    theme.surface_raised,
                ))
                .cell(div().w(px(140.)).truncate().child(quiet(owner, theme)))
                .cell(quiet(format!("{} refs", info.repo.refs_count), theme))
                .cell(div().flex_1())
                .cell(quiet(format!("height {}", info.repo.last_activity), theme)),
        );
    }
    column.child(list).into_any_element()
}

/// The rail: the same repositories, compact, while one of them is open.
pub(crate) fn rail(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let home = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_repos(cx));
    let column = div()
        .id(id("forge-rail"))
        .w(px(forge.layout.tree))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_r_1()
        .border_color(theme.sidebar_border)
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .child(
            div()
                .id(id("forge-rail-header"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .bg(theme.sidebar_raised)
                .border_b_1()
                .border_color(theme.sidebar_border)
                .text_color(theme.sidebar_foreground)
                .child(heading_in(
                    id("forge-rail-title"),
                    "Forge",
                    1,
                    theme.sidebar_foreground,
                ))
                .child(div().flex_1())
                .child(
                    // The rail is ink: its controls wear the sidebar tones,
                    // never the surface ones the content column uses.
                    div()
                        .id(id("forge-rail-home"))
                        .px_1()
                        .py_0p5()
                        .rounded_sm()
                        .text_size(px(12.))
                        .text_color(theme.sidebar_muted)
                        .hover(|style| style.bg(theme.sidebar))
                        .role(Role::Button)
                        .focusable()
                        .on_click(home)
                        .child("All"),
                ),
        )
        .child(
            Input::new(id("forge-rail-search"))
                .h(px(28.))
                .mx_2()
                .my_1()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.sidebar_border)
                .bg(theme.sidebar_raised)
                .text_color(theme.sidebar_foreground)
                .value(forge.search.clone())
                .placeholder("Search repositories…")
                .label("Search repositories")
                .on_input(cx.listener(|forge, text: &String, _, cx| {
                    forge.search = text.clone();
                    cx.notify();
                })),
        );
    let reply = match staged(
        forge,
        &query(),
        "forge-rail-list",
        "Reading the repositories…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let mut list = scroller("forge-rail-list");
    for info in listed(forge, reply) {
        let name = info.name.clone();
        let open = cx.listener({
            let name = name.clone();
            move |forge, _: &ClickEvent, _, cx| forge.open_repo(name.clone(), cx)
        });
        list = list.child(
            row(id(format!("forge-rail-repo-{name}")), theme)
                .on_click(open)
                .sidebar(true)
                .selected(forge.nav().repo.as_deref() == Some(name.as_str()))
                .cell(div().flex_1().truncate().child(name)),
        );
    }
    column.child(list).into_any_element()
}

fn header(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, search_id: &str) -> AnyElement {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        forge.search = text.clone();
        cx.notify();
    });
    let new = cx.listener(|forge, _: &ClickEvent, _, cx| forge.start_repo(cx));
    div()
        .id(id("forge-repos-header"))
        .flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .child(heading(id("forge-repos-title"), "Repositories", 1, theme))
        .child(
            Input::new(id(search_id.to_owned()))
                .h(px(28.))
                .flex_1()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .value(forge.search.clone())
                .placeholder("Filter by name")
                .label("Filter repositories")
                .on_input(typed),
        )
        .child(
            button(id("forge-new-repo"), "+ New", theme, new)
                .primary(true)
                .enabled(forge.session.connected),
        )
        .into_any_element()
}

fn dialog(form: &crate::state::NewRepo, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        if let Some(form) = &mut forge.new_repo {
            form.name = text.clone();
            form.error.clear();
        }
        cx.notify();
    });
    let toggle = cx.listener(|forge, _: &ClickEvent, _, cx| {
        if let Some(form) = &mut forge.new_repo {
            form.sha256 = !form.sha256;
        }
        cx.notify();
    });
    let create = cx.listener(|forge, _: &ClickEvent, _, cx| forge.create_repo(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_repo(cx));
    let mut card = div()
        .id(id("forge-new-repo-card"))
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
            id("forge-new-repo-title"),
            "New repository",
            2,
            theme,
        ))
        .child(
            Input::new(id("forge-new-repo-name"))
                .h(px(28.))
                .w_full()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .value(form.name.clone())
                .placeholder("letters, digits, dot, dash, underscore")
                .label("Repository name")
                .on_input(typed),
        );
    if !form.error.is_empty() {
        card = card.child(
            div()
                .id(id("forge-new-repo-error"))
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(form.error.clone()),
        );
    }
    card.child(
        div()
            .flex()
            .gap_2()
            .child(
                button(id("forge-new-repo-sha256"), "sha256 objects", theme, toggle)
                    .selected(form.sha256),
            )
            .child(div().flex_1())
            .child(button(id("forge-new-repo-cancel"), "Cancel", theme, cancel))
            .child(button(id("forge-new-repo-submit"), "Create", theme, create).primary(true)),
    )
    .into_any_element()
}
