//! Refs: branches and tags, how far each one is from the default head, and
//! the Change a comparison can become.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::ui::components::{badge, button, empty_state, id, quiet, ref_label, row, short_hex};
use crate::ui::{pending, scroller, staged};
use forge::{Mergeability, Query, RefInfo, Reply, Revision};

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    head: &[u8],
) -> AnyElement {
    let mut column = div()
        .id(id("forge-refs"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(pending(forge, "changes", theme));
    if let Some(form) = &forge.form {
        column = column.child(crate::ui::changes::form(form, forge, cx, theme));
    }
    let reply = match staged(
        forge,
        &forge.refs_query(),
        "forge-refs-list",
        "Reading refs…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let Reply::Refs { page, .. } = reply else {
        return column.into_any_element();
    };
    if page.items.is_empty() {
        return column
            .child(empty_state(
                id("forge-refs-empty"),
                "No refs",
                "This repository is unborn: nothing has been pushed to it yet.",
                theme,
            ))
            .into_any_element();
    }
    let mut list = scroller("forge-refs-list");
    for info in &page.items {
        list = list.child(ref_row(forge, info, head, cx, theme));
    }
    column
        .child(list.children(forbidden(forge, theme)))
        .into_any_element()
}

/// The ref name and target columns.
const NAME_W: Pixels = px(200.);
const TARGET_W: Pixels = px(100.);

/// One ref: its name, kind, target and standing, and Compare for a branch
/// other than the default head.
fn ref_row(
    forge: &Forge,
    info: &RefInfo,
    head: &[u8],
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> AnyElement {
    let name = info.name.clone();
    let label = ref_label(&name);
    let tag = name.starts_with(b"refs/tags/");
    let pick = cx.listener({
        let name = name.clone();
        move |forge, _: &ClickEvent, _, cx| forge.pick_ref(name.clone(), cx)
    });
    let start = cx.listener({
        let name = name.clone();
        move |forge, _: &ClickEvent, _, cx| forge.start_change(name.clone(), cx)
    });
    let mut line = row(id(format!("forge-ref-row-{label}")), theme)
        .on_click(pick)
        .selected(name == forge.head_name())
        .cell(
            div()
                .w(NAME_W)
                .truncate()
                .child(crate::ui::bold(label.clone())),
        )
        .cell(badge(
            id(format!("forge-ref-kind-{label}")),
            if tag { "tag" } else { "branch" },
            theme.muted,
            theme.surface_raised,
        ))
        .cell(
            div()
                .w(TARGET_W)
                .whitespace_nowrap()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(short_hex(&info.target)),
        )
        .cell(standing(forge, &name, head, theme))
        .cell(div().flex_1());
    if name != forge.default_head() && !tag {
        line = line.cell(
            button(
                id(format!("forge-compare-{label}")),
                "Compare →",
                theme,
                start,
            )
            .enabled(forge.may_write()),
        );
    }
    line.into_any_element()
}

/// What this repository's settings forbid, if anything.
fn forbidden(forge: &Forge, theme: &Theme) -> Option<AnyElement> {
    let allow = forge
        .repo()
        .map(|(info, _, _)| {
            (
                info.repo.settings.allow_force,
                info.repo.settings.allow_delete,
            )
        })
        .unwrap_or((false, false));
    let what = match allow {
        (false, false) => "force pushes and ref deletions",
        (false, true) => "force pushes",
        (true, false) => "ref deletions",
        (true, true) => return None,
    };
    Some(quiet(format!("This repository forbids {what}."), theme))
}

/// Ahead/behind the default head, once the comparison for this ref lands.
fn standing(forge: &Forge, name: &[u8], head: &[u8], theme: &Theme) -> AnyElement {
    if name == head {
        return quiet("browsing", theme);
    }
    let query = Query::Compare {
        repo: forge.repo_name(),
        from: Revision::Ref(name.to_vec()),
        into: Revision::Ref(head.to_vec()),
    };
    match forge.ready(&query) {
        Some(Reply::Compare { comparison, .. }) => {
            let word = match comparison.mergeability {
                Mergeability::UpToDate => "merged",
                Mergeability::FastForward => "fast-forward",
                Mergeability::Diverged => "diverged",
                Mergeability::Unrelated => "unrelated",
            };
            quiet(
                format!(
                    "{} ahead · {} behind · {word}",
                    comparison.ahead, comparison.behind
                ),
                theme,
            )
        }
        _ => quiet("comparing…", theme),
    }
}
