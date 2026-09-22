//! Code: a lazy file tree beside one file's bytes. Text is drawn as mono
//! rows with a numbered gutter; a binary or oversize blob is the header the
//! program returned and nothing else.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::queries::PAGE;
use crate::ui::components::{button, empty_state, heading, id, mono, path_text, quiet, row};
use crate::ui::{prose, scroller, staged};
use forge::{Content, EntryKind, Query, Reply, TreeInfo};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut columns = div().id(id("forge-code")).flex().flex_1().min_h(px(0.));
    if forge.layout.tree_visible() || forge.nav().blob.is_none() {
        columns = columns.child(tree(forge, cx, theme));
    }
    columns.child(body(forge, cx, theme)).into_any_element()
}

fn tree_query(forge: &Forge) -> Option<Query> {
    Some(Query::Tree {
        repo: forge.nav().repo.clone()?,
        at: forge.head_oid()?,
        path: forge.nav().path.clone(),
        cursor: None,
        limit: PAGE,
    })
}

fn tree(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        forge.tree_search = text.clone();
        cx.notify();
    });
    let column = div()
        .id(id("forge-tree"))
        .w(px(300.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .id(id("forge-tree-header"))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .child(breadcrumb(forge, cx, theme))
                .child(
                    Input::new(id("forge-tree-search"))
                        .h(px(26.))
                        .w_full()
                        .px_2()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border_strong)
                        .bg(theme.surface)
                        .text_color(theme.foreground)
                        .value(forge.tree_search.clone())
                        .placeholder("Filter this directory")
                        .label("Filter files")
                        .on_input(typed),
                ),
        );
    let Some(query) = tree_query(forge) else {
        return column
            .child(quiet("Resolving the ref…", theme))
            .into_any_element();
    };
    let reply = match staged(
        forge,
        &query,
        "forge-tree-list",
        "Reading the tree…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let Reply::Tree { page, .. } = reply else {
        return column.into_any_element();
    };
    let needle = forge.tree_search.trim().to_lowercase();
    let shown: Vec<&TreeInfo> = page
        .items
        .iter()
        .filter(|entry| {
            needle.is_empty() || path_text(&entry.name).to_lowercase().contains(&needle)
        })
        .collect();
    if shown.is_empty() {
        return column
            .child(empty_state(
                id("forge-tree-empty"),
                "Nothing here",
                "This directory holds no entry the filter keeps.",
                theme,
            ))
            .into_any_element();
    }
    let mut list = scroller("forge-tree-list");
    for entry in shown {
        let name = path_text(&entry.name);
        let full = join(&forge.nav().path, &entry.name);
        let (glyph, kind) = match entry.kind {
            EntryKind::Directory => ("▸", "directory"),
            EntryKind::Gitlink => ("◆", "submodule"),
            EntryKind::Symlink => ("↪", "symlink"),
            EntryKind::Executable => ("▪", "executable"),
            EntryKind::File => ("▪", "file"),
        };
        let oid = entry.oid.clone();
        let is_dir = entry.kind == EntryKind::Directory;
        let open = cx.listener({
            let full = full.clone();
            move |forge, _: &ClickEvent, _, cx| {
                if is_dir {
                    forge.open_dir(full.clone(), cx)
                } else {
                    forge.open_file(full.clone(), oid.clone(), cx)
                }
            }
        });
        list = list.child(
            row(id(format!("forge-tree-{}", path_text(&full))), theme)
                .on_click(open)
                .selected(
                    forge
                        .nav()
                        .blob
                        .as_ref()
                        .is_some_and(|(path, _)| *path == full),
                )
                .cell(div().w(px(16.)).text_color(theme.muted).child(glyph))
                .cell(div().flex_1().truncate().child(name))
                .cell(quiet(kind, theme)),
        );
    }
    column.child(list).into_any_element()
}

fn breadcrumb(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let root = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_dir(Vec::new(), cx));
    let mut bar = div()
        .id(id("forge-tree-breadcrumb"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(button(id("forge-tree-root"), "/", theme, root));
    let mut walked: Vec<u8> = Vec::new();
    for segment in forge.nav().path.split(|byte| *byte == b'/') {
        if segment.is_empty() {
            continue;
        }
        if !walked.is_empty() {
            walked.push(b'/');
        }
        walked.extend_from_slice(segment);
        let here = walked.clone();
        let open =
            cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_dir(here.clone(), cx));
        bar = bar.child(button(
            id(format!("forge-crumb-{}", path_text(&walked))),
            path_text(segment),
            theme,
            open,
        ));
    }
    bar.into_any_element()
}

fn body(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((path, oid)) = forge.nav().blob.clone() else {
        return readme(forge, cx, theme);
    };
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.nav_close_blob(cx);
    });
    let header = div()
        .id(id("forge-blob-header"))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(heading(id("forge-blob-title"), path_text(&path), 2, theme))
        .child(quiet(crate::ui::components::short_oid(&oid), theme))
        .child(div().flex_1())
        .child(button(id("forge-blob-close"), "Close", theme, close));
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid,
        range: None,
    };
    let reply = match staged(forge, &query, "forge-blob", "Reading the file…", cx, theme) {
        Ok(reply) => reply,
        Err(state) => {
            return div()
                .id(id("forge-blob"))
                .flex_1()
                .flex()
                .flex_col()
                .min_h(px(0.))
                .child(header)
                .child(state)
                .into_any_element();
        }
    };
    let Reply::Blob { blob, .. } = reply else {
        return div().into_any_element();
    };
    let content: AnyElement = match blob.content {
        Content::Text => lines(&blob.bytes, theme),
        Content::Binary => empty_state(
            id("forge-blob-binary"),
            "Binary file",
            format!("{} bytes. Nothing here reads as text.", blob.size),
            theme,
        )
        .into_any_element(),
        Content::Oversize => empty_state(
            id("forge-blob-oversize"),
            "Too large to show",
            format!(
                "{} bytes, above this network's inline blob bound. Fetch the repository to read it.",
                blob.size
            ),
            theme,
        )
        .into_any_element(),
        Content::Gitlink => empty_state(
            id("forge-blob-gitlink"),
            "Submodule",
            "This entry points at another repository.",
            theme,
        )
        .into_any_element(),
    };
    div()
        .id(id("forge-blob"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(header)
        .child(content)
        .into_any_element()
}

/// The README of the root tree, when the tree has one.
fn readme(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((name, oid)) = forge.readme() else {
        return div()
            .id(id("forge-code-blank"))
            .flex_1()
            .child(empty_state(
                id("forge-code-empty"),
                "Pick a file",
                "The tree beside this pane holds what this ref carries.",
                theme,
            ))
            .into_any_element();
    };
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid,
        range: None,
    };
    let reply = match staged(
        forge,
        &query,
        "forge-readme",
        "Reading the README…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return state,
    };
    let Reply::Blob { blob, .. } = reply else {
        return div().into_any_element();
    };
    let text = String::from_utf8_lossy(&blob.bytes).into_owned();
    div()
        .id(id("forge-readme"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .p_3()
        .gap_2()
        .child(heading(
            id("forge-readme-title"),
            path_text(&name),
            2,
            theme,
        ))
        .child(if matches!(blob.content, Content::Text) {
            prose("forge-readme-body", &text)
        } else {
            quiet("This README is not text.", theme).into_any_element()
        })
        .into_any_element()
}

/// Source lines, numbered. The gutter is the permalink target a reader
/// points at; it carries no comment in this screen.
fn lines(bytes: &[u8], theme: &Theme) -> AnyElement {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let rows: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let count = rows.len();
    let muted = theme.muted;
    let foreground = theme.foreground;
    crate::ui::components::rows("forge-blob-lines", count, None, None, move |index| {
        div()
            .id(id(format!("forge-blob-line-{}", index + 1)))
            .flex()
            .gap_2()
            .px_2()
            .child(
                div()
                    .w(px(48.))
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(muted)
                    .child((index + 1).to_string()),
            )
            .child(mono(rows[index].clone(), foreground))
            .into_any_element()
    })
}

/// The full path of an entry inside the directory on screen.
pub(crate) fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() {
        return name.to_vec();
    }
    let mut path = dir.to_vec();
    path.push(b'/');
    path.extend_from_slice(name);
    path
}
