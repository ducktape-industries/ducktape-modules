//! The Account section: who the seated key is, its keys, the form that
//! gives a bare key an account, and a person's agents.
use super::*;
use crate::queries::{Account, Agent};
use identity::Standing;

pub(super) fn account(view: &Settings, cx: &mut Context<Settings>, theme: &Theme) -> AnyElement {
    match &view.account {
        Loadable::Ready(Some(account)) => {
            let who = match account.number {
                Some(number) => format!("{} · account {number}", account.name),
                // a key held by an agent that does not act goes by the agent
                None if account.note.is_some() => account.name.clone(),
                None => "Unregistered key".into(),
            };
            let keys = account.keys.iter().enumerate().map(|(i, key)| {
                // a bare key is the host's; an account's key goes by its label
                let label = match (account.number, &key.label) {
                    (None, _) => "Host key",
                    (Some(_), Some(label)) => label,
                    (Some(_), None) => "Key",
                };
                let hex = design::short_hex(&abi::hex(&key.key));
                let shown = match key.validator {
                    true => format!("{hex} · Validator"),
                    false => hex,
                };
                line(&format!("key/{i}"), label, &shown)
            });
            column("settings/account/data")
                .child(line("who", "Who I am", &who))
                .children(keys)
                // a bare key is seated: `queries::account` answers no
                // account at all while none is
                .when_some(account.note.as_ref(), |body, note| {
                    body.child(secondary("settings/account/note", note, theme))
                })
                .when(account.number.is_none() && account.note.is_none(), |body| {
                    body.child(create_account(view, cx, theme))
                })
                .when(account.manages, |body| {
                    body.child(agents(view, account, cx, theme))
                })
                .into_any_element()
        }
        Loadable::Ready(None) => design::empty_state(
            "settings/account/empty",
            "No account",
            "No host key is selected. Sign in with a key to create an account.",
            theme,
        )
        .into_any_element(),
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.read_account(cx));
            design::refused("settings/account", refusal.message.clone(), theme, retry)
                .into_any_element()
        }
        Loadable::Idle | Loadable::Loading(_) => {
            secondary("settings/account/loading", "Reading your account…", theme).into_any_element()
        }
    }
}

fn create_account(view: &Settings, cx: &mut Context<Settings>, theme: &Theme) -> AnyElement {
    let form = &view.create_account;
    let typed = cx.listener(|v: &mut Settings, text: &String, _, cx| {
        v.create_account.text = text.clone();
        cx.notify();
    });
    let pressed =
        cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_create_account(cx));
    let mut name = field(
        "settings/account/create/name",
        "Account name",
        form,
        theme,
        typed,
    );
    if !form.busy {
        name = name
            .on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.submit_create_account(cx)));
    }
    column("settings/account/create")
        .max_w(FORM_W)
        .child(secondary(
            "settings/account/create/help",
            "Your key isn't linked to an account yet. An account gives you a name others see.",
            theme,
        ))
        .child(name)
        .child(submit(
            "settings/account/create/submit",
            "Create account",
            "Creating…",
            form.busy,
            theme,
            pressed,
        ))
        .children(problem(
            "account/create",
            form,
            "Enter an account name.",
            theme,
        ))
        .into_any_element()
}

/// The agents this person manages, each with what its manager does to it
/// (rename, suspend or resume, revoke), a form to create one, and one to
/// add a key to one.
fn agents(
    view: &Settings,
    account: &Account,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> AnyElement {
    let create_typed = cx.listener(|v: &mut Settings, text: &String, _, cx| {
        v.create_agent.text = text.clone();
        cx.notify();
    });
    let key_typed = cx.listener(|v: &mut Settings, text: &String, _, cx| {
        v.agent_key.text = text.clone();
        cx.notify();
    });
    let create = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| {
        v.revoking = None;
        v.submit_create_agent(cx)
    });
    let add = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| {
        v.revoking = None;
        v.submit_agent_key(cx)
    });
    let lines: Vec<AnyElement> = account
        .agents
        .iter()
        .map(|agent| self::agent(view, account, agent, cx, theme))
        .collect();
    column("settings/agents")
        .max_w(FORM_W)
        .child(secondary(
            "settings/agents/help",
            "Agents act as accounts you manage. You answer for what they do.",
            theme,
        ))
        .children(lines)
        .children(
            account
                .more_agents
                .then(|| design::more_not_shown("settings/agents/more", theme)),
        )
        .child(field(
            "settings/agents/create/name",
            "Agent name",
            &view.create_agent,
            theme,
            create_typed,
        ))
        .child(submit(
            "settings/agents/create/submit",
            "Create agent",
            "Creating…",
            view.create_agent.busy,
            theme,
            create,
        ))
        .child(field(
            "settings/agents/key/request",
            "Agent key request",
            &view.agent_key,
            theme,
            key_typed,
        ))
        .child(submit(
            "settings/agents/key/submit",
            "Add key to agent",
            "Adding…",
            view.agent_key.busy,
            theme,
            add,
        ))
        .children(problem(
            "agents/create",
            &view.create_agent,
            "Enter an agent name.",
            theme,
        ))
        .children(problem("agents/key", &view.agent_key, "", theme))
        .children(problem("agents/standing", &view.agent_standing, "", theme))
        .into_any_element()
}

/// One agent's line, "Scout: Agent · managed by Maya · account 12 · 0 keys
/// · suspended" ([`Kind::badge`](identity::Kind::badge) and
/// [`Kind::note`](identity::Kind::note)), and what its manager does to it.
/// Revoked, it only reads as such.
fn agent(
    view: &Settings,
    manager: &Account,
    agent: &Agent,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> AnyElement {
    let number = agent.number;
    let badge = agent
        .kind
        .badge(|_| manager.name.clone())
        .unwrap_or_default();
    let keys = design::plural(agent.keys as u64, "key", "keys");
    let mut about = format!("{badge} · account {number} · {keys}");
    if let Some(note) = agent.kind.note() {
        about = format!("{about} · {note}");
    }
    let body = div()
        .id(format!("settings/agents/{number}/card"))
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .child(line(&format!("agents/{number}"), &agent.name, &about));
    let standing = agent.standing();
    if standing == Standing::Revoked {
        return body.into_any_element();
    }
    let rename = view.rename_agent.get(&number).cloned().unwrap_or_default();
    let typed = cx.listener(move |v: &mut Settings, text: &String, _, cx| {
        v.rename_agent.entry(number).or_default().text = text.clone();
        cx.notify();
    });
    let confirming = view.revoking == Some(number);
    body.child(field(
        &format!("settings/agents/{number}/name"),
        "New name",
        &rename,
        theme,
        typed,
    ))
    .child(actions(view, number, standing, &rename, cx, theme))
    .when(confirming, |body| {
        body.child(secondary(
            format!("settings/agents/{number}/revoke/warning"),
            "Revoking is final: this agent account never acts again.",
            theme,
        ))
    })
    .children(problem(
        &format!("agents/{number}/rename"),
        &rename,
        "Enter the agent's new name.",
        theme,
    ))
    .into_any_element()
}

/// An agent's presses: rename, suspend or resume, revoke (a second press
/// confirms).
fn actions(
    view: &Settings,
    number: u64,
    standing: Standing,
    rename: &Form,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> impl IntoElement {
    let (toggle, toggle_label, toggle_op) = match standing {
        Standing::Suspended => ("resume", "Resume", identity::Op::Resume { account: number }),
        Standing::Active | Standing::Revoked => (
            "suspend",
            "Suspend",
            identity::Op::Suspend { account: number },
        ),
    };
    let toggled = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.set_standing(toggle_op.clone(), cx)
    });
    let revoke = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.revoke(number, cx));
    let renamed = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.revoking = None;
        v.submit_rename_agent(number, cx)
    });
    let busy = view.agent_standing.busy;
    let confirming = view.revoking == Some(number);
    div()
        .id(format!("settings/agents/{number}/actions"))
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(submit(
            &format!("settings/agents/{number}/rename"),
            "Rename",
            "Renaming…",
            rename.busy,
            theme,
            renamed,
        ))
        .child(submit(
            &format!("settings/agents/{number}/{toggle}"),
            toggle_label,
            "Working…",
            busy,
            theme,
            toggled,
        ))
        .child(submit(
            &format!("settings/agents/{number}/revoke"),
            if confirming {
                "Revoke for good"
            } else {
                "Revoke"
            },
            "Working…",
            busy,
            theme,
            revoke,
        ))
}
