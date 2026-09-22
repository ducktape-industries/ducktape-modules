//! The window: a repositories rail, one content column, one docked panel.
//! Narrow windows fold the rail and the dock into toggles rather than
//! squeezing three columns into one.
pub(crate) mod change;
pub(crate) mod changes;
pub(crate) mod code;
pub(crate) mod commits;
pub(crate) mod components;
pub(crate) mod diff;
pub(crate) mod refs;
pub(crate) mod repos;
pub(crate) mod settings;

use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, FontWeight, Stateful, wire};

use crate::state::{Dock, RepoTab};
use crate::{Forge, contract::Reply};
use components::{button, chip, heading, id, quiet};

pub(crate) fn render(forge: &mut Forge, cx: &mut Context<Forge>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let resized = cx.listener(|forge, size: &(Pixels, Pixels), _, cx| {
        forge.measured(f32::from(size.0), f32::from(size.1), cx)
    });
    let shown = cx.listener(|forge, size: &(Pixels, Pixels), _, cx| {
        forge.measured(f32::from(size.0), f32::from(size.1), cx)
    });
    let mut columns = div().id(id("forge-columns")).flex().size_full().min_h(px(0.));
    if forge.layout.tree_visible() {
        columns = columns.child(repos::rail(forge, cx, &theme));
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
        .text_size(px(13.))
        .child(columns);
    ducktape_view_guest::sensor(id("forge-viewport"), root)
        .size_full()
        .on_show(shown)
        .on_resize(resized)
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
                .rounded_md()
                .bg(theme.danger_soft)
                .text_color(theme.foreground)
                .text_size(px(12.))
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
        .pb_2()
        .border_b_1()
        .border_color(theme.border);
    let mut title = div()
        .id(id("forge-repo-title"))
        .flex()
        .items_center()
        .gap_2()
        .child(heading(id("forge-repo-name"), name.clone(), 1, theme));
    if let Some((info, _, _)) = forge.repo() {
        let names = forge.names.ready();
        let owner = names.map_or_else(
            || crate::state::short(&abi::hex(&info.repo.owner)),
            |names| names.key(&info.repo.owner),
        );
        title = title
            .child(chip(
                id("forge-repo-owner"),
                format!("owner {owner}"),
                theme.muted,
                theme.surface_raised,
            ))
            .child(quiet(
                format!(
                    "{} refs · last activity at height {}",
                    info.repo.refs_count, info.repo.last_activity
                ),
                theme,
            ));
    }
    title = title.child(div().flex_1()).child(quiet(
        format!("duck://{}/forge/{name}", chain(forge)),
        theme,
    ));
    header = header.child(title).child(ref_picker(forge, cx, theme));
    let mut tabs = div().id(id("forge-tabs")).flex().gap_1();
    for tab in RepoTab::ALL {
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_tab(tab, cx));
        tabs = tabs.child(
            button(
                id(format!("forge-tab-{}", tab.slug())),
                tab.label(),
                theme,
                open,
            )
            .selected(forge.nav().tab == tab),
        );
    }
    let body: AnyElement = match forge.nav().tab {
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

fn chain(forge: &Forge) -> String {
    if forge.session.chain.is_empty() {
        "network".into()
    } else {
        forge.session.chain.clone()
    }
}

/// Branches and tags, default first; the picked one steers every screen.
fn ref_picker(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let head = forge.head_name();
    let default = forge.default_head();
    let Some(refs) = forge.refs() else {
        return quiet("Reading refs…", theme);
    };
    let mut ordered: Vec<&crate::contract::RefInfo> = refs.iter().collect();
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
            .selected(name == head),
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
        Dock::Overview => change::overview(forge, theme),
        Dock::Comments => change::comments(forge, cx, theme),
        Dock::MergeStatus => change::merge_status(forge, theme),
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
    let names = forge.names.ready();
    let mut column = div()
        .id(id("forge-about"))
        .flex()
        .flex_col()
        .gap_2()
        .child(fact("Default head", components::ref_label(&info.repo.settings.head), theme))
        .child(fact(
            "Hash",
            match info.repo.hash {
                abi::HashKind::Sha1 => "sha1",
                abi::HashKind::Sha256 => "sha256",
            },
            theme,
        ))
        .child(fact(
            "Owner",
            names.map_or_else(
                || crate::state::short(&abi::hex(&info.repo.owner)),
                |names| names.key(&info.repo.owner),
            ),
            theme,
        ))
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
        column = column.child(quiet(
            names.map_or_else(
                || crate::state::short(&abi::hex(key)),
                |names| names.key(key),
            ),
            theme,
        ));
    }
    column.into_any_element()
}

pub(crate) fn fact(label: &str, value: impl Into<String>, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .text_size(px(12.))
        .child(div().w(px(120.)).text_color(theme.muted).child(label.to_owned()))
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
        let failed = !op.error.is_empty();
        column = column.child(
            div()
                .id(id(format!("forge-pending-{}", op.id)))
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .rounded_md()
                .bg(if failed {
                    theme.danger_soft
                } else {
                    theme.surface_raised
                })
                .text_size(px(12.))
                .child(op.label.clone())
                .child(quiet(
                    if failed {
                        format!("Refused: {}", op.error)
                    } else if op.accepted {
                        "Waiting for the next block…".to_owned()
                    } else {
                        "Submitting…".to_owned()
                    },
                    theme,
                )),
        );
    }
    column.into_any_element()
}

/// The `Reply` a read landed, or the loading/refused state in its place.
pub(crate) fn staged<'a>(
    forge: &'a Forge,
    query: &crate::contract::Query,
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
            let retry = cx.listener(move |forge, _: &ClickEvent, _, cx| {
                forge.retry(query.clone(), cx)
            });
            Err(
                components::refused(id(element_id.to_owned()), refusal.sentence.clone(), theme, retry)
                    .into_any_element(),
            )
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

/// A host-painted markdown body — README, change bodies, review bodies.
pub(crate) fn markdown(name: &str, text: &str, dark: bool) -> AnyElement {
    ducktape_view_guest::surface(
        id(name.to_owned()),
        "markdown",
        vec![
            wire::SurfaceValue::Str(text.to_owned()),
            wire::SurfaceValue::Str(String::new()),
            wire::SurfaceValue::Bool(dark),
        ],
    )
    .into_any_element()
}
