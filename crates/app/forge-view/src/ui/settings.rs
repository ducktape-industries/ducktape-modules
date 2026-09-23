//! Repository settings: the default head, the force/delete flags and who
//! may write. Nothing the contract does not expose appears here.
use ducktape_view_guest::prelude::*;

use crate::Forge;
use crate::ui::components::{button, empty_state, heading, id, quiet, ref_label, row};
use crate::ui::{pending, scroller, staged};
use forge::Reply;

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let mut column = scroller("forge-settings").child(pending(forge, "settings", theme));
    let reply = match staged(
        forge,
        &forge.repo_query(),
        "forge-settings-repo",
        "Reading this repository…",
        cx,
        theme,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let Reply::Repo { writers, .. } = reply else {
        return column.into_any_element();
    };
    let Some(form) = forge.repo_settings.clone() else {
        return column.into_any_element();
    };
    let save = cx.listener(|forge, _: &ClickEvent, _, cx| forge.configure(cx));
    let force = cx.listener(|forge, _: &ClickEvent, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.allow_force = !form.allow_force;
        }
        cx.notify();
    });
    let delete = cx.listener(|forge, _: &ClickEvent, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.allow_delete = !form.allow_delete;
        }
        cx.notify();
    });
    let mut heads = div()
        .id(id("forge-settings-heads"))
        .flex()
        .flex_wrap()
        .gap_1()
        .items_center()
        .child(quiet("Default head", theme));
    for name in forge.branches().into_iter().take(24) {
        let pick = cx.listener({
            let name = name.clone();
            move |forge, _: &ClickEvent, _, cx| {
                if let Some(form) = &mut forge.repo_settings {
                    form.head = name.clone();
                }
                cx.notify();
            }
        });
        heads = heads.child(
            button(
                id(format!("forge-settings-head-{}", ref_label(&name))),
                ref_label(&name),
                theme,
                pick,
            )
            .selected(form.head == name),
        );
    }
    column = column
        .child(heading(id("forge-settings-title"), "Settings", 2, theme))
        .child(heads)
        .child(
            div()
                .id(id("forge-settings-flags"))
                .flex()
                .gap_2()
                .child(
                    button(
                        id("forge-settings-force"),
                        "Allow force pushes",
                        theme,
                        force,
                    )
                    .selected(form.allow_force),
                )
                .child(
                    button(
                        id("forge-settings-delete"),
                        "Allow ref deletion",
                        theme,
                        delete,
                    )
                    .selected(form.allow_delete),
                )
                .child(div().flex_1())
                .child(
                    button(id("forge-settings-save"), "Save", theme, save)
                        .primary(true)
                        .enabled(forge.session.connected),
                ),
        )
        .child(heading(
            id("forge-settings-access-title"),
            "Access",
            3,
            theme,
        ));
    let typed = cx.listener(|forge, text: &String, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.grant = text.clone();
        }
        cx.notify();
    });
    let grant = cx.listener(|forge, _: &ClickEvent, _, cx| forge.grant(cx));
    column = column.child(
        div()
            .id(id("forge-settings-access"))
            .flex()
            .gap_2()
            .items_center()
            .child(
                Input::new(id("forge-settings-grant-input"))
                    .h(px(28.))
                    .flex_1()
                    .px_2()
                    .border_1()
                    .border_color(theme.border_strong)
                    .bg(theme.surface)
                    .text_color(theme.foreground)
                    .value(form.grant.clone())
                    .placeholder("account number or key in hex")
                    .label("Grant write access")
                    .on_input(typed),
            )
            .child(
                button(id("forge-settings-grant"), "Grant", theme, grant)
                    .enabled(forge.session.connected),
            ),
    );
    if writers.items.is_empty() {
        return column
            .child(empty_state(
                id("forge-settings-no-writers"),
                "Only the owner writes",
                "Nobody else has been granted write access to this repository.",
                theme,
            ))
            .into_any_element();
    }
    let names = forge.names.ready();
    for key in &writers.items {
        let label = names.map_or_else(
            || crate::state::short(&abi::hex(key)),
            |names| names.key(key),
        );
        let revoke = cx.listener({
            let key = key.clone();
            move |forge, _: &ClickEvent, _, cx| forge.revoke(key.clone(), cx)
        });
        column = column.child(
            row::<fn(&ClickEvent, &mut Window, &mut App)>(
                id(format!("forge-writer-{}", abi::hex(key))),
                theme,
            )
            .cell(div().flex_1().truncate().child(label))
            .cell(button(
                id(format!("forge-settings-revoke-{}", abi::hex(key))),
                "Revoke",
                theme,
                revoke,
            )),
        );
    }
    column.into_any_element()
}
