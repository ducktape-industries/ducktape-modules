//! The Programs tab: what the registry runs, lists and will change.
use super::*;
use ducktape_view_guest::design;

pub(super) fn programs(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let network = match &view.network {
        Loaded::Ready(network) => network,
        Loaded::Failed(refusal) => return failed(&refusal.sentence, cx, theme),
        _ => return quiet("explorer-programs-loading", "Reading the registry…", theme),
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
    let running = network.programs.iter().map(|entry| {
        let go = Route::Transactions(Some(entry.program.clone()));
        row(
            SharedString::from(format!("explorer-program-{}", entry.program)).into(),
            entry.program.clone(),
            go,
            cx,
            theme,
        )
        .child(mono(entry.program.clone()).flex_1())
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(plural(entry.params as u64, "param byte", "param bytes")),
        )
        .child(mono(design::short_hex(&entry.code)).text_color(theme.muted))
        .into_any_element()
    });
    let running: Vec<_> = running.collect();
    // A view-only entry sends nothing, so it has no transactions to open.
    let listed = network.views.iter().map(|(name, code)| {
        div()
            .id(SharedString::from(format!("explorer-view-{name}")))
            .flex()
            .items_center()
            .gap_4()
            .h(px(40.))
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(mono(name.clone()).flex_1())
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child("view only"),
            )
            .child(mono(design::short_hex(code)).text_color(theme.muted))
            .into_any_element()
    });
    let listed: Vec<_> = listed.collect();
    let scheduled: Vec<_> = network
        .changes
        .iter()
        .enumerate()
        .map(|(index, scheduled)| {
            let change = &scheduled.change;
            let removal = change.code().is_none();
            div()
                .id(ElementId::named_usize("explorer-change", index))
                .flex()
                .items_center()
                .gap_4()
                .h(px(40.))
                .px_5()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .px_1()
                        .text_size(design::text::CAPTION)
                        .text_color(if removal {
                            theme.danger
                        } else {
                            theme.accent_foreground
                        })
                        .bg(if removal {
                            theme.danger_soft
                        } else {
                            theme.accent_soft
                        })
                        .child(change.verb()),
                )
                .child(mono(change.module().to_string()).flex_1())
                .child(
                    div()
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child(format!("at {}", scheduled.height)),
                )
                .children(change.code().map(|code| {
                    mono(design::short_hex(&abi::hex(code.digest()))).text_color(theme.muted)
                }))
                .into_any_element()
        })
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
        .into_any_element()
}
