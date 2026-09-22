//! The diff viewer: unified, drawn by this view from the program's typed
//! hunks. No patch text is parsed anywhere — a literal `++ x` is a source
//! line here, because it arrives as one.
//!
//! Every row is virtual, and a line's gutter number is its comment button.
use std::rc::Rc;

use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::ui::components::{chip, empty_state, id, mono, path_text, quiet};
use crate::ui::staged;
use forge::{Content, FileDiff, FileStatus, LineKind, Query, Reply, Side};

/// A route an event of a virtual row takes back into the view.
pub(crate) type Route<E> = Rc<dyn Fn(&E, &mut Window, &mut App)>;

/// The anchor a gutter button carries: path, new side, line.
pub(crate) type Anchor = (Vec<u8>, bool, u64);

/// One painted row of the flattened diff.
#[derive(Clone)]
struct Painted {
    kind: Kind,
    text: String,
    path: Vec<u8>,
    old: Option<u64>,
    new: Option<u64>,
    line: LineKind,
    /// a comment this reader has staged at this anchor
    draft: Option<String>,
    /// published line comments anchored here: author, body, outdated
    published: Vec<(String, String, bool)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    File,
    Hunk,
    Line,
}

/// `reviewable` turns the gutter into comment buttons.
pub(crate) fn render(
    forge: &Forge,
    query: &Query,
    element_id: &str,
    reviewable: bool,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let reply = match staged(forge, query, element_id, "Reading the diff…", cx, theme) {
        Ok(reply) => reply,
        Err(state) => return state,
    };
    let Reply::Diff {
        page, total_files, ..
    } = reply
    else {
        return div().into_any_element();
    };
    let only = forge.nav().diff_path.clone();
    let files: Vec<&FileDiff> = page
        .items
        .iter()
        .filter(|file| only.is_none() || only.as_deref() == path_of(file).as_deref())
        .collect();
    if files.is_empty() {
        return empty_state(
            id(format!("{element_id}-empty")),
            "No changes",
            if *total_files == 0 {
                "These endpoints hold the same tree."
            } else {
                "No file here matches what is selected."
            },
            theme,
        )
        .into_any_element();
    }
    // A screen with no file tree beside it (a commit's own diff) carries the
    // file headers above the virtual list, where they stay in view.
    let strip = (!reviewable).then(|| file_strip(&files, element_id, cx, theme));
    let rows = paint(forge, &files, reviewable);
    let count = rows.len();
    // A diff's width is set by its longest source line, so that row is the
    // one the list measures — and the one a headless render always draws.
    let widest = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == Kind::Line)
        .max_by_key(|(_, row)| row.text.len())
        .map(|(index, _)| index);
    let theme = *theme;
    let comment: Route<Anchor> =
        Rc::new(cx.listener(|forge, at: &Anchor, _, cx| {
            forge.open_comment(at.0.clone(), at.1, at.2, cx)
        }));
    let handle = forge.diff_scroll.clone();
    let list =
        crate::ui::components::rows(element_id, count, widest, Some(&handle), move |index| {
            paint_row(&rows[index], index, reviewable, &comment, &theme)
        });
    let Some(strip) = strip else {
        return list.into_any_element();
    };
    div()
        .id(id(format!("{element_id}-pane")))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(strip)
        .child(list)
        .into_any_element()
}

/// Which files this diff touches, above the rows themselves. Each one
/// narrows the list to itself.
fn file_strip(
    files: &[&FileDiff],
    element_id: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let mut strip = div()
        .id(id(format!("{element_id}-files")))
        .flex()
        .flex_col()
        .gap_0p5()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border);
    for file in files {
        let Some(path) = path_of(file) else { continue };
        let label = path_text(&path);
        let pick = cx.listener({
            let path = path.clone();
            move |forge, _: &ClickEvent, _, cx| forge.single_file(Some(path.clone()), cx)
        });
        strip = strip.child(
            div()
                .id(id(format!("{element_id}-file-{label}")))
                .flex()
                .gap_2()
                .text_size(px(12.))
                .role(Role::Button)
                .focusable()
                .on_click(pick)
                .child(crate::ui::bold(label))
                .child(quiet(status_label(file.status), theme))
                .child(quiet(
                    format!("+{} −{}", file.additions, file.deletions),
                    theme,
                )),
        );
    }
    strip.into_any_element()
}

pub(crate) fn path_of(file: &FileDiff) -> Option<Vec<u8>> {
    file.new_path.clone().or_else(|| file.old_path.clone())
}

fn status_label(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Modified => "modified",
        FileStatus::ModeChanged => "mode changed",
        FileStatus::TypeChanged => "type changed",
    }
}

fn paint(forge: &Forge, files: &[&FileDiff], reviewable: bool) -> Vec<Painted> {
    let review = reviewable.then(|| forge.review()).flatten();
    let published = if reviewable {
        published_comments(forge)
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for file in files {
        let path = path_of(file).unwrap_or_default();
        let header = match (&file.old_path, &file.new_path) {
            (Some(old), Some(new)) if old != new => {
                format!("{} → {}", path_text(old), path_text(new))
            }
            _ => path_text(&path),
        };
        rows.push(Painted {
            kind: Kind::File,
            text: format!(
                "{header} · {} · +{} −{}",
                status_label(file.status),
                file.additions,
                file.deletions
            ),
            path: path.clone(),
            old: None,
            new: None,
            line: LineKind::Context,
            draft: None,
            published: Vec::new(),
        });
        if file.hunks.is_empty() {
            rows.push(Painted {
                kind: Kind::Hunk,
                text: match file.content {
                    Content::Binary => "Binary file — no lines to show".into(),
                    Content::Oversize => "Too large to diff inline".into(),
                    Content::Gitlink => "Submodule pointer".into(),
                    Content::Text => "No line changes".into(),
                },
                path: path.clone(),
                old: None,
                new: None,
                line: LineKind::Context,
                draft: None,
                published: Vec::new(),
            });
            continue;
        }
        for hunk in &file.hunks {
            rows.push(Painted {
                kind: Kind::Hunk,
                text: format!(
                    "@@ -{},{} +{},{} @@",
                    hunk.old.start, hunk.old.count, hunk.new.start, hunk.new.count
                ),
                path: path.clone(),
                old: None,
                new: None,
                line: LineKind::Context,
                draft: None,
                published: Vec::new(),
            });
            for line in &hunk.lines {
                let anchor_side = line.new_line.is_some();
                let anchor_line = line.new_line.or(line.old_line).unwrap_or(0);
                rows.push(Painted {
                    kind: Kind::Line,
                    text: String::from_utf8_lossy(&line.bytes)
                        .trim_end_matches('\n')
                        .to_owned(),
                    path: path.clone(),
                    old: line.old_line,
                    new: line.new_line,
                    line: line.kind,
                    draft: review
                        .and_then(|review| review.staged(&path, anchor_side, anchor_line))
                        .map(|staged| staged.body.clone()),
                    published: published
                        .iter()
                        .filter(|(p, side, at, _, _, _)| {
                            *p == path && *side == anchor_side && *at == anchor_line
                        })
                        .map(|(_, _, _, author, body, outdated)| {
                            (author.clone(), body.clone(), *outdated)
                        })
                        .collect(),
                });
            }
        }
    }
    rows
}

type Anchored = (Vec<u8>, bool, u64, String, String, bool);

/// Every published line comment of the open change, by anchor.
fn published_comments(forge: &Forge) -> Vec<Anchored> {
    let Some((_, _, _, reviews)) = forge.change() else {
        return Vec::new();
    };
    let names = forge.names.ready();
    let mut all = Vec::new();
    for review in &reviews.items {
        let author = names.map_or_else(
            || crate::state::short(&abi::hex(&review.author)),
            |names| names.key(&review.author),
        );
        let outdated = forge.outdated(&review.draft.commit_oid);
        for comment in &review.draft.comments {
            all.push((
                comment.path.clone(),
                comment.side == Side::New,
                comment.line,
                author.clone(),
                comment.body.clone(),
                outdated,
            ));
        }
    }
    all
}

fn paint_row(
    row: &Painted,
    index: usize,
    reviewable: bool,
    comment: &Route<Anchor>,
    theme: &Theme,
) -> AnyElement {
    match row.kind {
        Kind::File => div()
            .id(id(format!("forge-diff-file-{}", path_text(&row.path))))
            .w_full()
            .px_2()
            .py_1()
            .bg(theme.surface_raised)
            .text_size(px(12.))
            .child(crate::ui::bold(row.text.clone()))
            .into_any_element(),
        Kind::Hunk => div()
            .id(id(format!("forge-diff-hunk-{index}")))
            .w_full()
            .px_2()
            .font_family("monospace")
            .text_size(px(11.5))
            .text_color(theme.accent)
            .bg(theme.surface)
            .child(row.text.clone())
            .into_any_element(),
        Kind::Line => line_row(row, index, reviewable, comment, theme),
    }
}

fn line_row(
    row: &Painted,
    index: usize,
    reviewable: bool,
    comment: &Route<Anchor>,
    theme: &Theme,
) -> AnyElement {
    let (background, colour) = match row.line {
        LineKind::Added => (theme.success_soft, theme.foreground),
        LineKind::Deleted => (theme.danger_soft, theme.foreground),
        LineKind::Context => (theme.background, theme.foreground),
    };
    let marker = match row.line {
        LineKind::Added => "+",
        LineKind::Deleted => "−",
        LineKind::Context => " ",
    };
    let mut body = div()
        .id(id(format!("forge-diff-line-{index}")))
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .bg(background)
        .child(gutter(row, false, reviewable, comment, theme))
        .child(gutter(row, true, reviewable, comment, theme))
        .child(
            div()
                .w(px(12.))
                .font_family("monospace")
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(marker),
        )
        .child(mono(row.text.clone(), colour));
    if let Some(draft) = &row.draft {
        body = body.child(chip(
            id(format!("forge-diff-draft-{index}")),
            format!("pending: {draft}"),
            theme.accent_foreground,
            theme.accent_soft,
        ));
    }
    if row.published.is_empty() {
        return body.into_any_element();
    }
    let mut column = div()
        .id(id(format!("forge-diff-thread-{index}")))
        .w_full()
        .flex()
        .flex_col()
        .child(body);
    for (at, (author, text, outdated)) in row.published.iter().enumerate() {
        column = column.child(
            div()
                .id(id(format!("forge-diff-comment-{index}-{at}")))
                .w_full()
                .flex()
                .gap_2()
                .px_6()
                .py_1()
                .bg(theme.surface)
                .text_size(px(12.))
                .child(crate::ui::bold(author.clone()))
                .child(div().flex_1().child(text.clone()))
                .when(*outdated, |element| {
                    element.child(chip(
                        id(format!("forge-diff-outdated-{index}-{at}")),
                        "outdated",
                        theme.warning,
                        theme.warning_soft,
                    ))
                }),
        );
    }
    column.into_any_element()
}

/// A gutter number. Under a review it is the button that anchors a comment
/// at `path:line (side)`; otherwise it is a number.
fn gutter(
    row: &Painted,
    new_side: bool,
    reviewable: bool,
    comment: &Route<Anchor>,
    theme: &Theme,
) -> AnyElement {
    let number = if new_side { row.new } else { row.old };
    let cell = || {
        div()
            .w(px(44.))
            .font_family("monospace")
            .text_size(px(11.5))
            .text_color(theme.muted)
    };
    let Some(number) = number else {
        return cell().child(" ").into_any_element();
    };
    if !reviewable {
        return cell().child(number.to_string()).into_any_element();
    }
    let at = (row.path.clone(), new_side, number);
    let comment = comment.clone();
    cell()
        .id(id(format!(
            "forge-gutter-{}-{}-{number}",
            path_text(&row.path),
            if new_side { "new" } else { "old" }
        )))
        .rounded_sm()
        .hover(|style| style.bg(theme.accent_soft))
        .role(Role::Button)
        .aria_label("Comment on this line")
        .focusable()
        .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            comment(&at, window, app)
        })
        .child(number.to_string())
        .into_any_element()
}

/// The composer for the one anchor whose gutter was clicked.
pub(crate) fn composer(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let Some(review) = forge.review() else {
        return div().into_any_element();
    };
    let Some(open) = review.open.clone() else {
        return div().into_any_element();
    };
    let typed = cx.listener(|forge, text: &String, _, cx| forge.typed_comment(text.clone(), cx));
    let save = cx.listener(|forge, _: &ClickEvent, _, cx| forge.stage_comment(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.discard_comment(cx));
    let mut card = div()
        .id(id("forge-comment"))
        .flex()
        .flex_col()
        .gap_2()
        .m_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .child(quiet(open.anchor(), theme))
        .child(
            Input::new(id("forge-comment-body"))
                .h(px(28.))
                .w_full()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .value(open.body.clone())
                .placeholder("What should change here?")
                .label("Line comment")
                .on_input(typed),
        );
    if !review.error.is_empty() {
        card = card.child(
            div()
                .id(id("forge-comment-error"))
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(review.error.clone()),
        );
    }
    card.child(
        div()
            .flex()
            .gap_2()
            .child(div().flex_1())
            .child(crate::ui::components::button(
                id("forge-comment-cancel"),
                "Cancel",
                theme,
                cancel,
            ))
            .child(
                crate::ui::components::button(id("forge-comment-save"), "Stage", theme, save)
                    .primary(true),
            ),
    )
    .into_any_element()
}
