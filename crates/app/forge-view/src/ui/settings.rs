//! Repository settings: the default head, the force/delete flags and who
//! may write. Nothing the contract does not expose appears here.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::{Div, Stateful};

use crate::Forge;
use crate::state::SettingsForm;
use crate::ui::components::{button, empty_state, heading, id, quiet, ref_label, row};
use crate::ui::{pending, scroller, staged};
use forge::Reply;

pub(crate) fn render(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let column = scroller("forge-settings").child(pending(forge, "settings", theme));
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
    column
        .child(heading(id("forge-settings-title"), "Settings", 2, theme))
        .child(heads(forge, &form, cx, theme))
        .child(flags(forge, &form, cx, theme))
        .child(heading(
            id("forge-settings-access-title"),
            "Access",
            3,
            theme,
        ))
        .child(grant_field(forge, &form, cx, theme))
        .children(writer_rows(forge, &writers.items, cx, theme))
        .into_any_element()
}

/// The branches a default head may name, the chosen one selected.
fn heads(
    forge: &Forge,
    form: &SettingsForm,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
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
    heads
}

/// The force and delete toggles, and Save.
fn flags(
    forge: &Forge,
    form: &SettingsForm,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
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
                .kind(design::Kind::Primary)
                .enabled(forge.may_write()),
        )
}

/// Who to grant write access to, and Grant.
fn grant_field(
    forge: &Forge,
    form: &SettingsForm,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    let typed = cx.listener(|forge, text: &String, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.grant = text.clone();
        }
        cx.notify();
    });
    let grant = cx.listener(|forge, _: &ClickEvent, _, cx| forge.grant(cx));
    div()
        .id(id("forge-settings-access"))
        .flex()
        .gap_2()
        .items_center()
        .child(
            Input::new(id("forge-settings-grant-input"))
                .h(design::size::CONTROL)
                .flex_1()
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .value(form.grant.clone())
                .placeholder("account number")
                .label("Grant write access")
                .on_input(typed),
        )
        .child(button(id("forge-settings-grant"), "Grant", theme, grant).enabled(forge.may_write()))
}

/// One row per writer with its Revoke, or the owner-only empty state.
fn writer_rows(
    forge: &Forge,
    writers: &[identity::Principal],
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Vec<AnyElement> {
    if writers.is_empty() {
        return vec![
            empty_state(
                id("forge-settings-no-writers"),
                "Only the owner writes",
                "Nobody else has been granted write access to this repository.",
                theme,
            )
            .into_any_element(),
        ];
    }
    writers
        .iter()
        .map(|key| {
            let label = forge.principal_name(key);
            let revoke = cx.listener({
                let key = key.clone();
                move |forge, _: &ClickEvent, _, cx| forge.revoke(key.clone(), cx)
            });
            row::<fn(&ClickEvent, &mut Window, &mut App)>(
                id(format!("forge-writer-{}", principal_id(key))),
                theme,
            )
            .cell(div().flex_1().truncate().child(label))
            .cell(button(
                id(format!("forge-settings-revoke-{}", principal_id(key))),
                "Revoke",
                theme,
                revoke,
            ))
            .into_any_element()
        })
        .collect()
}

/// A writer's element id: `acct-<n>` for an account.
fn principal_id(principal: &identity::Principal) -> String {
    match principal {
        identity::Principal::Account(number) => format!("acct-{number}"),
        identity::Principal::Module(module) => module.clone(),
        identity::Principal::Root => "system".into(),
    }
}
