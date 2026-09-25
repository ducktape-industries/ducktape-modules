//! The window: a repositories rail, one content column, one docked panel.
//! Narrow windows fold the rail and the dock into toggles rather than
//! squeezing three columns into one.
pub(crate) mod change;
pub(crate) mod changes;
pub(crate) mod code;
pub(crate) mod commits;
pub(crate) mod components;
pub(crate) mod conversation;
pub(crate) mod diff;
pub(crate) mod dock;
pub(crate) mod highlight;
pub(crate) mod markdown;
pub(crate) mod readme;
pub(crate) mod refs;
pub(crate) mod repos;
pub(crate) mod settings;

use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, FontWeight, Stateful};

use crate::Forge;
use crate::state::{Dock, Progress, RepoTab};
use components::{badge, button, heading, id, quiet};
use forge::Reply;

pub(crate) fn render(forge: &mut Forge, cx: &mut Context<Forge>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let measured = |cx: &mut Context<Forge>| {
        cx.listener(|forge, size: &(Pixels, Pixels), _, cx| {
            forge.measured(f32::from(size.0), f32::from(size.1), cx)
        })
    };
    let mut columns = div()
        .id(id("forge-columns"))
        .flex()
        .size_full()
        .min_h(px(0.));
    // The rail switches between repositories; with none open, the list
    // itself is the screen, and a rail beside it would say it twice.
    if forge.layout.tree_visible() && forge.nav().repo.is_some() {
        columns = columns.child(repos::rail(forge, cx, &theme)).child(divider(
            "forge-rail-resize",
            &theme,
            cx,
            |forge, dx| {
                forge.layout.tree += dx;
            },
        ));
    }
    columns = columns.child(main(forge, cx, &theme));
    if let Some(dock) = forge.nav().dock.filter(|_| forge.layout.dock_visible()) {
        columns = columns.child(panel(forge, dock, cx, &theme));
    }
    let root = div()
        .id(id("forge"))
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(columns);
    ducktape_view_guest::sensor(id("forge-viewport"), root)
        .size_full()
        .on_show(measured(cx))
        .on_resize(measured(cx))
}

fn main(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut column = div()
        .id(id("forge-main"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.));
    if forge.layout.narrow() {
        column = column.child(narrow_bar(forge, cx, theme));
    }
    if !forge.notice.is_empty() {
        column = column.child(
            div()
                .id(id("forge-notice"))
                .m_2()
                .p_2()
                .bg(theme.danger_soft)
                .text_color(theme.foreground)
                .text_size(design::text::SECONDARY)
                .child(forge.notice.clone()),
        );
    }
    let body: AnyElement = match (forge.nav().repo.clone(), forge.nav().change) {
        (None, _) => repos::overview(forge, cx, theme),
        (Some(_), Some(_)) => change::render(forge, cx, theme),
        (Some(_), None) => repo(forge, cx, theme),
    };
    column.child(body).into_any_element()
}

/// The repository header: name, the ref picker, its clone address and tabs.
fn repo(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let name = forge.repo_name();
    let head = forge.head_name();
    let mut header = div()
        .id(id("forge-repo-header"))
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .pt_3()
        .border_b_1()
        .border_color(theme.border);
    let mut title = div()
        .id(id("forge-repo-title"))
        .flex()
        .items_center()
        .gap_2()
        .child(heading(id("forge-repo-name"), name.clone(), 1, theme));
    if let Some((info, _, _)) = forge.repo() {
        let owner = forge.key_name(&info.repo.owner);
        title = title
            .child(badge(
                id("forge-repo-owner"),
                format!("owner {owner}"),
                theme.muted,
                theme.surface_raised,
            ))
            .child(quiet(
                format!(
                    "{} · active at block {}",
                    design::plural(info.repo.refs_count, "ref", "refs"),
                    info.repo.last_activity
                ),
                theme,
            ));
    }
    title = title
        .child(div().flex_1())
        .child(quiet(repo_link(forge, &name), theme));
    header = header.child(title).child(ref_picker(forge, cx, theme));
    let mut tabs = div().id(id("forge-tabs")).flex().gap_1();
    for tab in RepoTab::ALL {
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_tab(tab, cx));
        tabs = tabs.child(design::tab(
            id(format!("forge-tab-{}", tab.slug())),
            tab.label(),
            forge.nav().tab == tab,
            theme,
            open,
        ));
    }
    let about = cx.listener(|forge, _: &ClickEvent, _, cx| forge.toggle_dock(Dock::About, cx));
    tabs = tabs.child(div().flex_1()).child(
        button(id("forge-dock-about"), "About", theme, about)
            .selected(forge.nav().dock == Some(Dock::About))
            .kind(design::Kind::Quiet),
    );
    let body: AnyElement = match forge.nav().tab {
        RepoTab::Readme => readme::render(forge, cx, theme),
        RepoTab::Code => code::render(forge, cx, theme),
        RepoTab::Commits => commits::render(forge, cx, theme),
        RepoTab::Changes => changes::render(forge, cx, theme),
        RepoTab::Refs => refs::render(forge, cx, theme, &head),
        RepoTab::Settings => settings::render(forge, cx, theme),
    };
    div()
        .id(id("forge-repo"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header.child(tabs))
        .child(body)
        .into_any_element()
}

/// `duck://<chain>/forge/<name>`: the repository's clone address and link,
/// minted by ducklink from the session's chain id (`<label>#<salt>`); a
/// placeholder `duck://<network>/forge/<name>` while no chain is known.
pub(crate) fn repo_link(forge: &Forge, name: &str) -> String {
    ducklink::mint(&forge.session.chain, forge::PROGRAM, &[name])
        .unwrap_or_else(|| format!("duck://<network>/forge/{name}"))
}

/// Branches and tags, default first; the picked one steers every screen.
fn ref_picker(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let head = forge.head_name();
    let default = forge.default_head();
    let Some(refs) = forge.refs() else {
        return quiet("Reading refs…", theme);
    };
    let mut ordered: Vec<&forge::RefInfo> = refs.iter().collect();
    ordered.sort_by_key(|info| (info.name != default, info.name.clone()));
    let mut picker = div()
        .id(id("forge-ref-picker"))
        .flex()
        .flex_wrap()
        .gap_1()
        .items_center()
        .child(quiet("Ref", theme));
    for info in ordered.into_iter().take(24) {
        let name = info.name.clone();
        let pick = cx.listener({
            let name = name.clone();
            move |forge, _: &ClickEvent, _, cx| forge.pick_ref(name.clone(), cx)
        });
        picker = picker.child(
            button(
                id(format!("forge-ref-{}", components::path_text(&name))),
                components::ref_label(&name),
                theme,
                pick,
            )
            .selected(name == head)
            .kind(design::Kind::Quiet),
        );
    }
    picker.into_any_element()
}

/// On a narrow window the rail and the dock become toggles.
fn narrow_bar(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let tree = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.layout.tree_open = !forge.layout.tree_open;
        cx.notify();
    });
    let dock = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.layout.dock_open = !forge.layout.dock_open;
        cx.notify();
    });
    div()
        .id(id("forge-narrow-bar"))
        .flex()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
        .child(
            button(id("forge-toggle-rail"), "Repositories", theme, tree)
                .selected(forge.layout.tree_open),
        )
        .child(
            button(id("forge-toggle-dock"), "Panel", theme, dock).selected(forge.layout.dock_open),
        )
        .into_any_element()
}

/// The one docked panel a screen shows at a time.
fn panel(forge: &Forge, dock: Dock, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let close = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.toggle_dock(dock, cx));
    let body: AnyElement = match dock {
        Dock::About => about(forge, theme),
        Dock::Overview => dock::overview(forge, cx, theme),
        Dock::Comments => dock::comments(forge, cx, theme),
        Dock::MergeStatus => dock::merge_status(forge, theme),
    };
    div()
        .id(id("forge-dock"))
        .w(px(300.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .id(id("forge-dock-header"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(heading(id("forge-dock-title"), dock.label(), 2, theme))
                .child(div().flex_1())
                .child(button(id("forge-dock-close"), "Close", theme, close)),
        )
        .child(
            div()
                .id(id("forge-dock-body"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px_3()
                .pb_3()
                .child(body),
        )
        .into_any_element()
}

fn about(forge: &Forge, theme: &Theme) -> AnyElement {
    let Some((info, bounds, writers)) = forge.repo() else {
        return quiet("Reading this repository…", theme);
    };
    let mut column = div()
        .id(id("forge-about"))
        .flex()
        .flex_col()
        .gap_2()
        .child(fact(
            "Default head",
            components::ref_label(&info.repo.settings.head),
            theme,
        ))
        .child(fact(
            "Hash",
            match info.repo.hash {
                abi::HashKind::Sha1 => "sha1",
                abi::HashKind::Sha256 => "sha256",
            },
            theme,
        ))
        .child(fact("Owner", forge.key_name(&info.repo.owner), theme))
        .child(fact("Page size", bounds.page_size.to_string(), theme))
        .child(fact(
            "Inline blob bound",
            format!("{} bytes", bounds.blob_bytes),
            theme,
        ))
        .child(heading(id("forge-about-access"), "Access", 3, theme));
    if writers.items.is_empty() {
        column = column.child(quiet("Only the owner writes here.", theme));
    }
    for key in &writers.items {
        column = column.child(quiet(forge.key_name(key), theme));
    }
    column.into_any_element()
}

pub(crate) fn fact(label: &str, value: impl Into<String>, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .text_size(design::text::SECONDARY)
        .child(
            div()
                .w(px(120.))
                .text_color(theme.muted)
                .child(label.to_owned()),
        )
        .child(div().flex_1().child(value.into()))
        .into_any_element()
}

/// The optimistic rows of a scope: what the reader issued, still in flight.
pub(crate) fn pending(forge: &Forge, scope: &str, theme: &Theme) -> AnyElement {
    let ops = forge.pending_in(scope);
    if ops.is_empty() {
        return div().into_any_element();
    }
    let mut column = div()
        .id(id(format!("forge-pending-{scope}")))
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .py_1();
    for op in ops {
        let (tone, status) = match &op.progress {
            Progress::Submitting => (theme.surface_raised, "Submitting…".to_owned()),
            Progress::Accepted => (
                theme.surface_raised,
                "Waiting for the next block…".to_owned(),
            ),
            Progress::Refused(sentence) => (theme.danger_soft, format!("Refused: {sentence}")),
        };
        column = column.child(
            div()
                .id(id(format!("forge-pending-{}", op.id)))
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .bg(tone)
                .text_size(design::text::SECONDARY)
                .child(op.label.clone())
                .child(quiet(status, theme)),
        );
    }
    column.into_any_element()
}

/// The `Reply` a read landed, or the loading/refused state in its place.
pub(crate) fn staged<'a>(
    forge: &'a Forge,
    query: &forge::Query,
    element_id: &str,
    loading_text: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Result<&'a Reply, AnyElement> {
    match forge.stage(query) {
        crate::Stage::Ready(reply) => Ok(reply),
        crate::Stage::Loading => Err(components::loading(
            id(format!("{element_id}-loading")),
            loading_text,
            theme,
        )),
        crate::Stage::Failed(refusal) => {
            let query = query.clone();
            let retry =
                cx.listener(move |forge, _: &ClickEvent, _, cx| forge.retry(query.clone(), cx));
            Err(ducktape_view_guest::design::refused(
                element_id,
                refusal.sentence.clone(),
                theme,
                retry,
            )
            .m_2()
            .into_any_element())
        }
    }
}

/// A scrolling content column, the shape every screen body uses.
pub(crate) fn scroller(name: &str) -> Stateful<Div> {
    div()
        .id(id(name.to_owned()))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
}

pub(crate) fn bold(text: impl Into<String>) -> AnyElement {
    div()
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
        .into_any_element()
}

/// The line between two panes, dragged to move it. The shared
/// `resize_handle` is the grab zone; `drag` applies the horizontal delta.
pub(crate) fn divider(
    name: &'static str,
    theme: &Theme,
    cx: &mut Context<Forge>,
    drag: impl Fn(&mut Forge, f32) + 'static,
) -> impl IntoElement {
    let dragged = cx.listener(move |forge, delta: &(Pixels, Pixels), _, cx| {
        drag(forge, delta.0.into());
        forge.layout.clamp();
        cx.notify();
    });
    resize_handle(id(name), div().w(px(1.)).h_full().bg(theme.border)).on_drag(dragged)
}
