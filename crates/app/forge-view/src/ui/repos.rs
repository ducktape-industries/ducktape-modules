//! The repositories: the full list when nothing is open, the rail that
//! switches between them when something is.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, Stateful};

use crate::Forge;
use crate::queries::PAGE;
use crate::ui::components::{button, empty_state, heading, id, quiet, ref_label, row};
use crate::ui::{pending, scroller, staged};
use forge::{Query, Reply, RepoInfo};

fn query() -> Query {
    Query::Repos { page: PAGE }
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

/// The facts column widths on a repository row: owner, head, refs, activity.
const OWNER_W: Pixels = px(140.);
const HEAD_W: Pixels = px(72.);
const REFS_W: Pixels = px(52.);
const ACTIVITY_W: Pixels = px(96.);
/// The overview's filter field.
const SEARCH_W: Pixels = px(320.);

/// One repository: its name over the address it clones from, and what it
/// is (owner, default branch, refs, last activity) on the right.
fn repo_row(
    forge: &Forge,
    info: &RepoInfo,
    owner: String,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let name = info.name.clone();
    let group = format!("forge-repo-{name}-row");
    let open = cx.listener({
        let name = name.clone();
        move |forge, _: &ClickEvent, _, cx| forge.open_repo(name.clone(), cx)
    });
    div()
        .id(id(format!("forge-repo-{name}")))
        .group(group.clone())
        .flex()
        .items_center()
        .gap_4()
        .px_4()
        .py(design::space::SM)
        .border_b_1()
        .border_color(theme.border)
        .hover(|style| style.bg(theme.surface))
        .role(Role::Button)
        .aria_label(format!("Open {name}"))
        .focusable()
        .on_click(open)
        .child(repo_title(forge, &name, &group, cx, theme))
        .child(repo_facts(info, owner, theme))
        .into_any_element()
}

/// A repository's name over its clone address and Copy.
fn repo_title(
    forge: &Forge,
    name: &str,
    group: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Div {
    let url = crate::ui::repo_link(forge, name);
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(design::space::HAIR)
        .child(
            div()
                .text_size(design::text::SECTION)
                .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                .truncate()
                .child(name.to_owned()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.faint)
                        .child(url.clone()),
                )
                .child(copy_button(forge, name, url, group, cx, theme)),
        )
}

/// Copies the clone address; shown on hover, and while it says Copied.
fn copy_button(
    forge: &Forge,
    name: &str,
    url: String,
    group: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    let copy = cx.listener({
        let name = name.to_owned();
        move |forge, _: &ClickEvent, _, cx| {
            cx.host()
                .notify::<ducktape_view_guest::methods::ClipboardWrite>(url.clone());
            forge.copied = Some(name.clone());
            cx.notify();
        }
    });
    let copied = forge.copied.as_deref() == Some(name);
    let button = div()
        .id(id(format!("forge-repo-{name}-copy")))
        .role(Role::Button)
        .aria_label(format!("Copy the address of {name}"))
        .focusable()
        // a click here is not also a click on the row that opens the repo
        .occlude()
        .px_1p5()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .hover(|style| style.text_color(theme.foreground))
        .on_click(copy)
        .child(if copied { "Copied" } else { "Copy" });
    match copied {
        true => button,
        false => button
            .invisible()
            .group_hover(group.to_owned(), |style| style.visible()),
    }
}

/// What a repository is: owner, default branch, refs, last activity.
fn repo_facts(info: &RepoInfo, owner: String, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_5()
        .flex_shrink_0()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(
            div()
                .w(OWNER_W)
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    design::avatar(&owner, design::size::AVATAR_SM, theme)
                        .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD),
                )
                .child(div().min_w(px(0.)).truncate().child(owner)),
        )
        .child(
            div()
                .w(HEAD_W)
                .truncate()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .child(ref_label(&info.repo.settings.head)),
        )
        .child(
            div()
                .w(REFS_W)
                .child(design::plural(info.repo.refs_count, "ref", "refs")),
        )
        .child(
            div()
                .w(ACTIVITY_W)
                .flex()
                .justify_end()
                .child(design::block_link(
                    id(format!("forge-repo-{}-activity", info.name)),
                    info.repo.last_activity,
                    theme,
                )),
        )
}

/// The screen: every repository of this network, newest activity first.
pub(crate) fn overview(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let body = staged(
        forge,
        &query(),
        "forge-repos-list",
        "Reading the repositories…",
        cx,
        theme,
    );
    let count = body.as_ref().ok().map(|reply| listed(forge, reply).len());
    let mut column = div()
        .id(id("forge-repos"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header(forge, count, cx, theme, "forge-repos-search"));
    if let Some(form) = &forge.new_repo {
        column = column.child(dialog(form, cx, theme));
    }
    column = column.child(pending(forge, "repos", theme));
    let body = match body {
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
    // rows run edge to edge, a hairline between them
    let mut list = scroller("forge-repos-list").p_0().gap_0();
    for info in rows {
        let owner = forge.party_name(&info.repo.owner);
        list = list.child(repo_row(forge, info, owner, cx, theme));
    }
    column.child(list).into_any_element()
}

/// The rail: the same repositories, compact, while one of them is open.
pub(crate) fn rail(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let column = div()
        .id(id("forge-rail"))
        .w(px(forge.layout.tree))
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .child(rail_home(cx, theme))
        .child(rail_search(forge, cx, theme));
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

/// The window's title bar already says Forge: the rail's head is the way
/// back to every repository.
fn rail_home(cx: &mut Context<Forge>, theme: &Theme) -> Stateful<Div> {
    let home = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_repos(cx));
    div()
        .id(id("forge-rail-header"))
        .flex()
        .items_center()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.sidebar_border)
        .child(
            div()
                .id(id("forge-rail-home"))
                .flex_1()
                .px_1()
                .py_1()
                .text_size(design::text::SECONDARY)
                .text_color(theme.sidebar_muted)
                .hover(|style| {
                    style
                        .bg(theme.sidebar_raised)
                        .text_color(theme.sidebar_foreground)
                })
                .role(Role::Button)
                .focusable()
                .on_click(home)
                .child("← All repositories"),
        )
}

/// The row pads, not the field: a full-width field with its own margins
/// ran past the rail's edge.
fn rail_search(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> Div {
    div().px_2().py_1().child(
        Input::new(id("forge-rail-search"))
            .h(design::size::CONTROL)
            .px_2()
            .py_1()
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
    )
}

fn header(
    forge: &Forge,
    count: Option<usize>,
    cx: &mut Context<Forge>,
    theme: &Theme,
    search_id: &str,
) -> AnyElement {
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
        .children(count.map(|count| quiet(count.to_string(), theme)))
        .child(
            Input::new(id(search_id.to_owned()))
                .h(design::size::CONTROL)
                .w(SEARCH_W)
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .value(forge.search.clone())
                .placeholder("Filter by name")
                .label("Filter repositories")
                .on_input(typed),
        )
        .child(div().flex_1())
        .child(
            button(id("forge-new-repo"), "+ New", theme, new)
                .kind(design::Kind::Primary)
                .enabled(forge.may_write()),
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
                .h(design::size::CONTROL)
                .w_full()
                .px_2()
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
                .text_size(design::text::SECONDARY)
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
            .child(
                button(id("forge-new-repo-submit"), "Create", theme, create)
                    .kind(design::Kind::Primary),
            ),
    )
    .into_any_element()
}
