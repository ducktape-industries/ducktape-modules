//! The README tab: the root README rendered whole, the repository's front
//! page. The Code tab is where the tree lives.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, Stateful};

use crate::Forge;
use crate::ui::code::{links, no_commits};
use crate::ui::components::{empty_state, id, path_text, quiet};
use crate::ui::{markdown, staged};
use forge::{BlobView, Content, Query, Reply};

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let page = div()
        .id(id("forge-readme"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    let Some(root) = forge.tree_query(Vec::new()) else {
        return page
            .child(match forge.refs() {
                Some(_) => no_commits(theme),
                None => quiet("Resolving the ref…", theme),
            })
            .into_any_element();
    };
    if let Err(state) = staged(
        forge,
        &root,
        "forge-readme-tree",
        "Reading the tree…",
        cx,
        theme,
    ) {
        return page.child(state).into_any_element();
    }
    let Some((name, oid)) = forge.readme() else {
        return page
            .child(empty_state(
                id("forge-readme-none"),
                "No README",
                "This ref carries no README at its root. The Code tab has its files.",
                theme,
            ))
            .into_any_element();
    };
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.clone(),
        range: None,
    };
    let reply = match staged(
        forge,
        &query,
        "forge-readme-blob",
        "Reading the README…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return page.child(state).into_any_element(),
    };
    let Reply::Blob { blob, .. } = reply else {
        return page.into_any_element();
    };
    page.child(document(forge, &name, &oid, blob, cx, theme))
        .into_any_element()
}

/// The widest a README runs, a comfortable reading measure.
const MEASURE: Pixels = px(880.);

/// The README's path over its rendered body.
fn document(
    forge: &Forge,
    name: &[u8],
    oid: &str,
    blob: &BlobView,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    let body = if matches!(blob.content, Content::Text) {
        markdown::render_blocks(
            "forge-readme-body",
            &forge.blob_cache.doc(oid, &blob.bytes),
            theme,
            &links(Vec::new(), cx),
        )
    } else {
        quiet("This README is not text.", theme)
    };
    div()
        .id(id("forge-readme-page"))
        .w_full()
        .max_w(MEASURE)
        .px_6()
        .py_5()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .id(id("forge-readme-title"))
                .pb_2()
                .border_b_1()
                .border_color(theme.border)
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(path_text(name)),
        )
        .child(body)
}
