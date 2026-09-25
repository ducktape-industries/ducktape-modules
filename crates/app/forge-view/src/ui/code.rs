//! Code: a file tree beside one file's bytes. A directory opens its
//! children inline beneath it; a text file is drawn highlighted with a
//! numbered gutter, a markdown file rendered; a binary or oversize blob is
//! the header the program returned and nothing else.
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

use ducktape_view_guest::design::{self, space, text};
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, KeyDownEvent, Stateful};

use crate::Forge;
use crate::tree::{Key, Row, Slot};
use crate::ui::components::{button, empty_state, heading, id, path_text, quiet};
use crate::ui::{divider, highlight, markdown, staged};
use forge::{BlobView, Content, EntryKind, Query, Reply};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut columns = div().id(id("forge-code")).flex().flex_1().min_h(px(0.));
    if forge.layout.tree_visible() || forge.nav().blob.is_none() {
        columns = columns.child(tree(forge, cx, theme)).child(divider(
            "forge-files-resize",
            theme,
            cx,
            |forge, dx| {
                forge.layout.files += dx;
            },
        ));
    }
    columns.child(body(forge, cx, theme)).into_any_element()
}

/// How far each tree level indents its rows.
const INDENT_STEP: f32 = 14.;
/// The column a row's expand/kind glyph sits in.
const GLYPH: Pixels = px(12.);

fn tree(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let column = tree_column(forge, cx, theme);
    let Some(query) = forge.tree_query(Vec::new()) else {
        // `refs()` lands (even empty) before `head_oid()` ever resolves for
        // a repo with no commits: without this, a freshly created repo sat
        // on "Resolving the ref…" forever instead of saying so.
        let waiting = match forge.refs() {
            Some(_) => no_commits(theme),
            None => quiet("Resolving the ref…", theme).into_any_element(),
        };
        return column.child(waiting).into_any_element();
    };
    if let Err(state) = staged(
        forge,
        &query,
        "forge-tree-list",
        "Reading the tree…",
        cx,
        theme,
    ) {
        return column.child(state).into_any_element();
    }
    let rows = forge.tree_rows();
    if rows.is_empty() {
        return column
            .child(empty_state(
                id("forge-tree-empty"),
                "Nothing here",
                "This tree holds no file the filter keeps.",
                theme,
            ))
            .into_any_element();
    }
    column
        .child(tree_rows(forge, rows, cx, theme))
        .into_any_element()
}

/// The tree's column and its filter field.
fn tree_column(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> Stateful<Div> {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        forge.tree_search = text.clone();
        cx.notify();
    });
    div()
        .id(id("forge-tree"))
        .w(px(forge.layout.files))
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(
            div().id(id("forge-tree-header")).p_2().child(
                Input::new(id("forge-tree-search"))
                    .h(design::size::ROW)
                    .w_full()
                    .px_2()
                    .border_1()
                    .border_color(theme.border_strong)
                    .bg(theme.surface)
                    .text_color(theme.foreground)
                    .value(forge.tree_search.clone())
                    .placeholder("Filter files")
                    .label("Filter files")
                    .on_input(typed),
            ),
        )
}

/// What marks a row: the open file, the keyboard cursor, open folders.
struct Marks {
    open: Option<Vec<u8>>,
    cursor: Option<Vec<u8>>,
    expanded: BTreeSet<Vec<u8>>,
}

/// The rows, drawn whole or as a virtual list, under the tree's keys.
fn tree_rows(forge: &Forge, rows: Vec<Row>, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let pressed = cx.listener(|forge, event: &KeyDownEvent, _, cx| {
        if let Some(key) = Key::parse(&event.keystroke.key) {
            forge.tree_key(key, cx);
        }
    });
    let marks = Marks {
        open: forge.nav().blob.as_ref().map(|(path, _)| path.clone()),
        cursor: forge.nav().cursor.clone(),
        expanded: forge.nav().expanded.clone(),
    };
    let theme = *theme;
    let press = Rc::new(cx.listener(|forge, row: &Row, _, cx| match &row.slot {
        Slot::Entry { .. } if row.is_dir() => forge.toggle_dir(row.path.clone(), cx),
        Slot::Entry { oid, .. } => forge.open_file(row.path.clone(), oid.clone(), cx),
        Slot::Loading | Slot::Failed(_) => {}
    }));
    let count = rows.len();
    let paint = move |index: usize| tree_row(&rows[index], &marks, press.clone(), &theme);
    // A tree that fits is drawn whole; one that may overflow is a virtual
    // list, whose scroll handle keeps the keyboard cursor in view. Half the
    // window is a safe guess at the pane's height: over it costs nothing.
    let list = if count as f32 * design::height::ROW as f32 <= forge.layout.height / 2. {
        let mut list = div()
            .id(id("forge-tree-list"))
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for index in 0..count {
            list = list.child(paint(index));
        }
        list.into_any_element()
    } else {
        uniform_list(
            id("forge-tree-list"),
            count,
            move |range: Range<usize>, _, _| range.map(&paint).collect::<Vec<_>>(),
        )
        .track_scroll(&forge.tree_scroll)
        .flex_1()
        .min_h(px(0.))
        .into_any_element()
    };
    div()
        .id(id("forge-tree-rows"))
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .pb_2()
        .role(Role::Tree)
        .aria_label("Files")
        .focusable()
        .on_key_down(pressed)
        .child(list)
        .into_any_element()
}

/// One tree row: an entry, or a folder still reading or refused.
fn tree_row(
    entry: &Row,
    marks: &Marks,
    press: Rc<impl Fn(&Row, &mut Window, &mut App) + 'static>,
    theme: &Theme,
) -> AnyElement {
    let indent = space::SM + px(entry.depth as f32 * INDENT_STEP);
    let kind = match &entry.slot {
        Slot::Entry { kind, .. } => *kind,
        Slot::Loading | Slot::Failed(_) => {
            let text = match &entry.slot {
                Slot::Failed(sentence) => sentence.clone(),
                _ => "Reading…".to_owned(),
            };
            // a row's height, so a virtual tree measures one and knows all
            return div()
                .min_h(design::size::ROW)
                .flex()
                .items_center()
                .pl(indent + GLYPH + space::XS)
                .child(quiet(text, theme))
                .into_any_element();
        }
    };
    let is_dir = kind == EntryKind::Directory;
    let expanded = is_dir && marks.expanded.contains(&entry.path);
    let glyph = match kind {
        EntryKind::Directory if expanded => "▾",
        EntryKind::Directory => "▸",
        EntryKind::Gitlink => "◆",
        EntryKind::Symlink => "↪",
        EntryKind::Executable | EntryKind::File => "",
    };
    let row = entry.clone();
    // A row is pressed, never focused: the list holds focus, so Enter
    // reaches the tree's key handler alone and not a focused row too.
    let selected = marks.open.as_deref() == Some(&entry.path[..]);
    let cursor = marks.cursor.as_deref() == Some(&entry.path[..]);
    let mut line = div()
        .id(id(format!("forge-tree-{}", path_text(&entry.path))))
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .min_h(design::size::ROW)
        .pl(indent)
        .pr_2()
        .role(Role::TreeItem)
        .aria_level(entry.depth + 1)
        .aria_selected(selected)
        .hover(|style| style.bg(theme.hover))
        .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            press(&row, window, app)
        })
        .child(
            div()
                .w(GLYPH)
                .flex_none()
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .child(glyph),
        )
        .child(div().flex_1().truncate().child(entry.name.clone()));
    if is_dir {
        line = line.aria_expanded(expanded);
    }
    if selected {
        line = line.bg(theme.accent_soft);
    } else if cursor {
        line = line.bg(theme.hover);
    }
    line.into_any_element()
}

pub(crate) fn no_commits(theme: &Theme) -> AnyElement {
    empty_state(
        id("forge-tree-no-commits"),
        "No commits yet",
        "Push code to this ref to browse it here: `git push duck://<network>/forge/<name> main`.",
        theme,
    )
    .into_any_element()
}

fn body(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some((path, oid)) = forge.nav().blob.clone() else {
        return div()
            .id(id("forge-code-blank"))
            .flex_1()
            .child(empty_state(
                id("forge-code-empty"),
                "Pick a file",
                "Open a folder in the tree to see what it holds; press a file to read it here.",
                theme,
            ))
            .into_any_element();
    };
    let pane = div()
        .id(id("forge-blob"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(blob_header(&path, &oid, cx, theme));
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.clone(),
        range: None,
    };
    let reply = match staged(forge, &query, "forge-blob", "Reading the file…", cx, theme) {
        Ok(reply) => reply,
        Err(state) => return pane.child(state).into_any_element(),
    };
    let Reply::Blob { blob, .. } = reply else {
        return pane.into_any_element();
    };
    pane.child(blob_content(forge, &path, &oid, blob, cx, theme))
        .into_any_element()
}

/// The open file's path, id and close button.
fn blob_header(path: &[u8], oid: &str, cx: &mut Context<Forge>, theme: &Theme) -> Stateful<Div> {
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.nav_close_blob(cx);
    });
    div()
        .id(id("forge-blob-header"))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(heading(id("forge-blob-title"), path_text(path), 2, theme))
        .child(quiet(crate::ui::components::short_hex(oid), theme))
        .child(div().flex_1())
        .child(button(id("forge-blob-close"), "Close", theme, close))
}

/// A blob as its content reads: markdown rendered, text highlighted, or
/// the header the program returned for anything else.
fn blob_content(
    forge: &Forge,
    path: &[u8],
    oid: &str,
    blob: &BlobView,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let name = path_text(path);
    let dir = path[..path.iter().rposition(|b| *b == b'/').unwrap_or(0)].to_vec();
    match blob.content {
        Content::Text if is_markdown(&name) => div()
            .id(id("forge-blob-doc"))
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_5()
            .py_4()
            .child(markdown::render_blocks(
                "forge-blob-markdown",
                &forge.blob_cache.doc(oid, &blob.bytes),
                theme,
                &links(dir, cx),
            ))
            .into_any_element(),
        Content::Text => lines(forge.blob_cache.lines(oid, &name, &blob.bytes), theme),
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
    }
}

/// A document's links, a relative one resolved against `dir`, its folder
/// (the root for a change's body).
pub(crate) fn links(dir: Vec<u8>, cx: &mut Context<Forge>) -> markdown::OnLink {
    Rc::new(cx.listener(move |forge, dest: &String, _, cx| forge.follow_link(&dir, dest, cx)))
}

fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// A text blob's rows and their highlight tokens.
pub(crate) struct Lines {
    rows: Vec<String>,
    tokens: Vec<Vec<(Range<usize>, highlight::Token)>>,
}

/// The open blob decoded once: its lines and tokens (by blob id and name,
/// since the name picks the language) and its markdown blocks (by blob id),
/// so a frame redraws a file without decoding and tokenizing it again. One
/// entry each: the pane shows one file at a time.
#[derive(Default)]
pub(crate) struct BlobCache {
    lines: RefCell<Option<(String, String, Rc<Lines>)>>,
    doc: RefCell<Option<(String, Rc<Vec<markdown::Block>>)>>,
}

impl BlobCache {
    fn lines(&self, oid: &str, name: &str, bytes: &[u8]) -> Rc<Lines> {
        let mut slot = self.lines.borrow_mut();
        if let Some((o, n, lines)) = slot.as_ref()
            && o == oid
            && n == name
        {
            return lines.clone();
        }
        let text = String::from_utf8_lossy(bytes);
        let rows: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let tokens = highlight::tokens(name, &rows.iter().map(String::as_str).collect::<Vec<_>>());
        let lines = Rc::new(Lines { rows, tokens });
        *slot = Some((oid.to_owned(), name.to_owned(), lines.clone()));
        lines
    }

    pub(crate) fn doc(&self, oid: &str, bytes: &[u8]) -> Rc<Vec<markdown::Block>> {
        let mut slot = self.doc.borrow_mut();
        if let Some((o, blocks)) = slot.as_ref()
            && o == oid
        {
            return blocks.clone();
        }
        let blocks = Rc::new(markdown::parse(&String::from_utf8_lossy(bytes)));
        *slot = Some((oid.to_owned(), blocks.clone()));
        blocks
    }
}

/// One digit's advance in [`design::fonts::FAMILY_MONO`], in ems: the
/// font's published metric, since no text measurement reaches a view. The
/// gutter is sized from it before any line is laid out.
const MONO_DIGIT_EM: f32 = 0.6;

/// Source lines, highlighted, numbered in a mono gutter.
fn lines(lines: Rc<Lines>, theme: &Theme) -> AnyElement {
    let count = lines.rows.len();
    let digit = design::type_scale::SECONDARY as f32 * MONO_DIGIT_EM;
    let gutter = px(count.to_string().len() as f32 * digit) + space::BLOCK; // plus its padding and edge
    let theme = *theme;
    crate::ui::components::rows("forge-blob-lines", count, None, None, move |index| {
        div()
            .id(id(format!("forge-blob-line-{}", index + 1)))
            .flex()
            .gap_3()
            .child(
                div()
                    .w(gutter)
                    .flex_none()
                    .pr_2()
                    .flex()
                    .justify_end()
                    .border_r_1()
                    .border_color(theme.border)
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.faint)
                    .child((index + 1).to_string()),
            )
            .child(highlight::line(
                id(format!("forge-blob-text-{}", index + 1)),
                &lines.rows[index],
                &lines.tokens[index],
                &theme,
            ))
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

#[cfg(test)]
mod tests {
    use super::BlobCache;
    use std::rc::Rc;

    #[test]
    fn a_blob_is_decoded_once_per_id_and_name() {
        let cache = BlobCache::default();
        let first = cache.lines("a1", "x.rs", b"fn a() {}\n");
        assert!(Rc::ptr_eq(&first, &cache.lines("a1", "x.rs", b"")));
        assert!(!Rc::ptr_eq(&first, &cache.lines("a1", "x.toml", b"")));
        assert!(!Rc::ptr_eq(&first, &cache.lines("b2", "x.toml", b"")));
        let doc = cache.doc("a1", b"# Hi");
        assert!(Rc::ptr_eq(&doc, &cache.doc("a1", b"")));
        assert!(!Rc::ptr_eq(&doc, &cache.doc("b2", b"# Hi")));
    }
}
