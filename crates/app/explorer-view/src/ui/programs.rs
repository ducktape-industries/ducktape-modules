//! The Programs tab: what the registry runs, lists and will change.
use super::*;
use crate::Network;
use module_registry::Scheduled;

pub(super) fn programs(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let network = match &view.network {
        Loadable::Ready(network) => network,
        Loadable::Failed(refusal) => return failed(&refusal.message, cx, theme),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-programs-loading", "Reading the registry…", theme);
        }
    };
    if network.programs.is_empty() && network.views.is_empty() && network.changes.is_empty() {
        return empty_state(
            "explorer-empty",
            "No programs",
            "The registry of this network runs nothing yet.",
            theme,
        )
        .into_any_element();
    }
    let running = running(network, cx, theme);
    let listed = listed(network, theme);
    let scheduled: Vec<_> = network
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| scheduled(index, change, theme))
        .collect();
    let nothing_scheduled = scheduled.is_empty().then(|| {
        quiet(
            "explorer-no-changes",
            "Nothing is scheduled against the registry.",
            theme,
        )
    });
    div()
        .id("explorer-list")
        .child(heading(
            "explorer-running-header",
            "Running",
            Some(caption(
                plural(network.programs.len() as u64, "program", "programs"),
                theme,
            )),
            theme,
        ))
        .children(running)
        .when(!listed.is_empty(), |list| {
            list.child(heading(
                "explorer-views-header",
                "Views",
                Some(caption(plural(listed.len() as u64, "view", "views"), theme)),
                theme,
            ))
            .children(listed)
        })
        .child(heading(
            "explorer-scheduled-header",
            "Scheduled",
            None,
            theme,
        ))
        .children(scheduled)
        .children(nothing_scheduled)
        .children(
            network
                .more
                .then(|| design::more_not_shown("explorer-changes-more", theme).px_5()),
        )
        .into_any_element()
}

/// Each running program, a row that opens its transactions.
fn running(network: &Network, cx: Cx, theme: &Theme) -> Vec<AnyElement> {
    network
        .programs
        .iter()
        .map(|entry| {
            row(
                SharedString::from(format!("explorer-program-{}", entry.program)).into(),
                entry.program.clone(),
                Route::Transactions(Some(entry.program.clone())),
                cx,
                theme,
            )
            .child(mono(entry.program.clone()).flex_1())
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(plural(
                        entry.params.len() as u64,
                        "param byte",
                        "param bytes",
                    )),
            )
            .child(mono(short(entry.code.digest())).text_color(theme.muted))
            .into_any_element()
        })
        .collect()
}

/// The view-only entries. One sends nothing, so it has no transactions to
/// open.
fn listed(network: &Network, theme: &Theme) -> Vec<AnyElement> {
    network
        .views
        .iter()
        .map(|listed| {
            line(
                SharedString::from(format!("explorer-view-{}", listed.name)).into(),
                theme,
            )
            .child(mono(listed.name.clone()).flex_1())
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child("view only"),
            )
            .child(mono(short(listed.view.digest())).text_color(theme.muted))
            .into_any_element()
        })
        .collect()
}

/// One change the registry will fold in: what it does, to what, when.
fn scheduled(index: usize, scheduled: &Scheduled, theme: &Theme) -> AnyElement {
    let change = &scheduled.change;
    let (foreground, background) = match change.code() {
        None => (theme.danger, theme.danger_soft),
        Some(_) => (theme.accent_foreground, theme.accent_soft),
    };
    line(ElementId::named_usize("explorer-change", index), theme)
        .child(design::badge(
            ElementId::named_usize("explorer-change-verb", index),
            change.verb(),
            foreground,
            background,
        ))
        .child(mono(change.program().to_string()).flex_1())
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(format!("at {}", scheduled.height)),
        )
        .children(
            change
                .code()
                .map(|code| mono(short(code.digest())).text_color(theme.muted)),
        )
        .into_any_element()
}

/// A list row that opens nothing.
fn line(id: ElementId, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_4()
        .h(ROW_H)
        .px_5()
        .border_b_1()
        .border_color(theme.border)
}
