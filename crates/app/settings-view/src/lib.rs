//! Settings reads node system facts; account contracts stay in the view.
mod account;
mod api;
use account::{Account, read_account};
use api::*;
use ducktape_view_guest::design;
use ducktape_view_guest::methods::Changes;
use ducktape_view_guest::methods::{ClipboardWrite, ClockTicks};
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Context, Render, Task, View, Window, export_view};
use ducktape_view_guest::{Div, FontWeight, Stateful};
use futures::StreamExt;
use identity::Standing;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    session: Session,
    status: Loadable<NodeStatus>,
    account: Loadable<Option<Account>>,
    invite: Loadable<Invite>,
    ttl: usize,
    copied: String,
    create_account: Form,
    create_agent: Form,
    agent_key: Form,
    /// each agent's rename, by its number
    #[serde(default)]
    rename_agent: BTreeMap<u64, Form>,
    /// the one suspend, resume or revoke in flight
    #[serde(default)]
    agent_standing: Form,
    #[serde(skip)]
    watches: Vec<Task<()>>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
/// A one-field form: what was typed, whether its submit is in flight, and
/// the refusal it met.
struct Form {
    text: String,
    busy: bool,
    error: String,
}
const TTL: [u64; 3] = [1, 7, 30];
impl View for Settings {
    const PREFERRED_WINDOW_SIZE: &'static str = "820,940";
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            ttl: 1,
            ..Self::default()
        };
        view.restored(window, cx);
        view
    }
    fn restored(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.watches.clear();
        let mut props = cx.host().subscribe::<HostSession>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while let Some(reply) = props.next().await {
                if this
                    .update(cx, |view, cx| {
                        match reply {
                            Ok(session) => {
                                if session.account.is_some() {
                                    view.create_account = Form::default();
                                }
                                view.session = session;
                                view.read_account(cx);
                            }
                            Err(refusal) => view.account = Loadable::Failed(refusal),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut live = cx.host().subscribe::<Changes<Valset>>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while live.next().await.is_some() {
                if this
                    .update(cx, |view, cx| {
                        view.read(cx);
                        view.read_account(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut ticks = cx.host().subscribe::<ClockTicks>(1000);
        self.watches.push(cx.spawn(async move |this, cx| {
            while ticks.next().await.is_some() {
                if this.update(cx, |view, cx| view.read(cx)).is_err() {
                    break;
                }
            }
        }));
        self.read(cx);
    }
}
impl Settings {
    fn read(&mut self, cx: &mut Context<Self>) {
        if self.status.ready().is_some() {
            cx.refresh(cx.host().ask::<ChainStatus>(()), |view, status, _| {
                view.status = Loadable::Ready(status)
            });
        } else if !self.status.is_loading() {
            self.status = cx.load(cx.host().ask::<ChainStatus>(()), |v| &mut v.status);
        }
        cx.notify();
    }
    fn read_account(&mut self, cx: &mut Context<Self>) {
        let (key, number) = (self.session.signer.clone(), self.session.account);
        self.account = cx.load(read_account(cx.host(), key, number), |v| &mut v.account);
        cx.notify();
    }
    fn node(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.status {
            Loadable::Ready(s) => div()
                .id("settings/node/data")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .children([
                    line("network", "Network", &s.chain_id),
                    line(
                        "height",
                        "Height / epoch",
                        &format!("{} / {}", s.height, s.epoch),
                    ),
                    line("block", "Block time", &format!("{} ms", s.block_time_ms)),
                    line("tip", "Tip", &abi::hex(&s.tip)),
                    line(
                        "identity",
                        "Node identity",
                        &design::short_hex(&abi::hex(&s.identity)),
                    ),
                    line("contract", "Contract version", &s.contract.to_string()),
                ])
                .into_any_element(),
            Loadable::Failed(e) => div()
                .id("settings/node/error")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(refusal("node", &e.message, theme))
                .child(
                    button("settings/node/retry", "Retry node", theme)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.read(cx))),
                )
                .into_any_element(),
            _ => {
                secondary("settings/node/loading", "Reading node status…", theme).into_any_element()
            }
        }
    }
    fn account(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.account {
            Loadable::Ready(Some(a)) => {
                let mut body = div()
                    .id("settings/account/data")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .child(line(
                        "who",
                        "Who I am",
                        &match a.number {
                            Some(number) => format!("{} · account {number}", a.name),
                            None => a.name.clone(),
                        },
                    ))
                    .children(a.keys.iter().enumerate().map(|(i, k)| {
                        line(
                            &format!("key/{i}"),
                            &k.label,
                            &if k.validator {
                                format!("{} · Validator", k.key)
                            } else {
                                k.key.clone()
                            },
                        )
                    }));
                // A key resolves to `number: None` only once it has been
                // matched against a non-empty seated key (see
                // `account::read_account`), so a key is seated here.
                if a.number.is_none() {
                    body = body.child(self.create_account_form(cx, theme));
                }
                if a.manages {
                    body = body.child(self.agents(a, cx, theme));
                }
                body.into_any_element()
            }
            Loadable::Ready(None) => div()
                .id("settings/account/empty")
                .flex()
                .flex_col()
                .gap_1()
                .p_6()
                .w_full()
                .max_w(px(420.))
                .child(
                    div()
                        .id("settings/account/empty/title")
                        .text_size(design::text::SECTION)
                        .font_weight(FontWeight::MEDIUM)
                        .child("No account"),
                )
                .child(
                    secondary(
                        "settings/account/empty/detail",
                        "No host key is selected. Sign in with a key to create an account.",
                        theme,
                    )
                    .w_full(),
                )
                .into_any_element(),
            Loadable::Failed(e) => div()
                .id("settings/account/error")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(refusal("account", &e.message, theme))
                .child(
                    button("settings/account/retry", "Retry account", theme)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.read_account(cx))),
                )
                .into_any_element(),
            _ => secondary("settings/account/loading", "Reading your account…", theme)
                .into_any_element(),
        }
    }
    fn create_account_form(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        let busy = self.create_account.busy;
        let typed = cx.listener(|v, event: &String, _, cx| {
            v.create_account.text = event.clone();
            cx.notify();
        });
        let mut name = Input::new("settings/account/create/name")
            .h(px(28.))
            .px_2()
            .py_1()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .value(self.create_account.text.clone())
            .placeholder("Account name")
            .label("Account name")
            .disabled(busy)
            .on_input(typed);
        if !busy {
            name = name.on_submit(cx.listener(|v, _: &(), _, cx| v.submit_create_account(cx)));
        }
        let mut body = div()
            .id("settings/account/create")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .max_w(px(420.))
            .child(secondary(
                "settings/account/create/help",
                "Your key isn't linked to an account yet. An account gives you a name others see.",
                theme,
            ))
            .child(name)
            .child(
                button(
                    "settings/account/create/submit",
                    if busy {
                        "Creating…"
                    } else {
                        "Create account"
                    },
                    theme,
                )
                .aria_disabled(busy)
                .when(busy, |b| b.opacity(0.5).tab_stop(false))
                .when(!busy, |b| {
                    b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.submit_create_account(cx)))
                }),
            );
        if !self.create_account.error.is_empty() {
            body = body.child(refusal("account/create", &self.create_account.error, theme));
        }
        body.into_any_element()
    }
    fn submit_create_account(&mut self, cx: &mut Context<Self>) {
        if self.create_account.busy {
            return;
        }
        let name = self.create_account.text.trim().to_string();
        if name.is_empty() {
            self.create_account.error = "Enter an account name.".into();
            cx.notify();
            return;
        }
        self.create_account.error.clear();
        self.create_account.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = host
                .ask::<Submit<Identity>>(identity::Op::Create {
                    name,
                    scheme: abi::Scheme::Ed25519,
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                // created: the form stays busy until the host's session
                // names the new account, which re-reads it
                if let Err(refusal) = result {
                    view.create_account.busy = false;
                    view.create_account.error =
                        format!("Couldn’t create this account: {}", refusal.message);
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// The agents this person manages, each with what its manager does to
    /// it (rename, suspend or resume, revoke), a form to create one, and
    /// one to add a key to one: the agent's key request (the hex of an
    /// `AddKey` whose consent the new key signed), which this key submits
    /// as the manager.
    fn agents(&self, a: &Account, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        let create_typed = cx.listener(|v, event: &String, _, cx| {
            v.create_agent.text = event.clone();
            cx.notify();
        });
        let key_typed = cx.listener(|v, event: &String, _, cx| {
            v.agent_key.text = event.clone();
            cx.notify();
        });
        let create = cx.listener(|v, _: &ClickEvent, _, cx| v.submit_create_agent(cx));
        let add = cx.listener(|v, _: &ClickEvent, _, cx| v.submit_agent_key(cx));
        let mut body = div()
            .id("settings/agents")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .max_w(px(420.))
            .child(secondary(
                "settings/agents/help",
                "Agents act as accounts you manage. You answer for what they do.",
                theme,
            ))
            .children(a.agents.iter().map(|agent| self.agent(agent, cx, theme)))
            .child(field(
                "settings/agents/create/name",
                "Agent name",
                &self.create_agent,
                theme,
                create_typed,
            ))
            .child(submit(
                "settings/agents/create/submit",
                "Create agent",
                "Creating…",
                self.create_agent.busy,
                theme,
                create,
            ))
            .child(field(
                "settings/agents/key/request",
                "Agent key request",
                &self.agent_key,
                theme,
                key_typed,
            ))
            .child(submit(
                "settings/agents/key/submit",
                "Add key to agent",
                "Adding…",
                self.agent_key.busy,
                theme,
                add,
            ));
        for (key, form) in [
            ("create", &self.create_agent),
            ("key", &self.agent_key),
            ("standing", &self.agent_standing),
        ] {
            if !form.error.is_empty() {
                body = body.child(refusal(&format!("agents/{key}"), &form.error, theme));
            }
        }
        body.into_any_element()
    }
    /// One agent's line and what its manager does to it. Revoked, it only
    /// reads as such.
    fn agent(&self, agent: &account::Agent, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        let number = agent.number;
        let standing = match agent.standing {
            Standing::Active => "active",
            Standing::Suspended => "suspended",
            Standing::Revoked => "revoked",
        };
        let mut body = div()
            .id(format!("settings/agents/{number}/card"))
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .child(line(
                &format!("agents/{number}"),
                &agent.name,
                &format!(
                    "Agent · account {number} · {} · {standing}",
                    design::plural(agent.keys as u64, "key", "keys"),
                ),
            ));
        if agent.standing == Standing::Revoked {
            return body.into_any_element();
        }
        let rename = self.rename_agent.get(&number).cloned().unwrap_or_default();
        let typed = cx.listener(move |v, event: &String, _, cx| {
            v.rename_agent.entry(number).or_default().text = event.clone();
            cx.notify();
        });
        let (toggle_id, toggle_label, toggle_op) = match agent.standing {
            Standing::Suspended => ("resume", "Resume", identity::Op::Resume { account: number }),
            Standing::Active | Standing::Revoked => (
                "suspend",
                "Suspend",
                identity::Op::Suspend { account: number },
            ),
        };
        let busy = self.agent_standing.busy;
        let standing_op = |op: identity::Op| {
            cx.listener(move |v, _: &ClickEvent, _, cx| {
                v.submit_agent_op(op.clone(), |v| &mut v.agent_standing, cx)
            })
        };
        let toggle = standing_op(toggle_op);
        let revoke = standing_op(identity::Op::Revoke { account: number });
        let renamed =
            cx.listener(move |v, _: &ClickEvent, _, cx| v.submit_rename_agent(number, cx));
        body = body
            .child(field(
                &format!("settings/agents/{number}/name"),
                "New name",
                &rename,
                theme,
                typed,
            ))
            .child(
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
                        &format!("settings/agents/{number}/{toggle_id}"),
                        toggle_label,
                        "Working…",
                        busy,
                        theme,
                        toggle,
                    ))
                    .child(submit(
                        &format!("settings/agents/{number}/revoke"),
                        "Revoke",
                        "Working…",
                        busy,
                        theme,
                        revoke,
                    )),
            );
        if !rename.error.is_empty() {
            body = body.child(refusal(
                &format!("agents/{number}/rename"),
                &rename.error,
                theme,
            ));
        }
        body.into_any_element()
    }
    fn submit_rename_agent(&mut self, number: u64, cx: &mut Context<Self>) {
        let form = self.rename_agent.entry(number).or_default();
        let name = form.text.trim().to_string();
        if name.is_empty() {
            form.error = "Enter the agent's new name.".into();
            cx.notify();
            return;
        }
        let op = identity::Op::SetName {
            account: number,
            name,
        };
        self.submit_agent_op(op, move |v| v.rename_agent.entry(number).or_default(), cx);
    }
    fn submit_create_agent(&mut self, cx: &mut Context<Self>) {
        let name = self.create_agent.text.trim().to_string();
        if name.is_empty() {
            self.create_agent.error = "Enter an agent name.".into();
            cx.notify();
            return;
        }
        let op = identity::Op::CreateAgent { name };
        self.submit_agent_op(op, |v| &mut v.create_agent, cx);
    }
    fn submit_agent_key(&mut self, cx: &mut Context<Self>) {
        let mine = |account: u64| {
            self.account
                .ready()
                .and_then(Option::as_ref)
                .is_some_and(|a| a.agents.iter().any(|agent| agent.number == account))
        };
        let op = abi::unhex(self.agent_key.text.trim())
            .and_then(|bytes| abi::decode::<identity::Op>(&bytes).ok())
            .filter(
                |op| matches!(op, identity::Op::AddKey { consent, .. } if mine(consent.account)),
            );
        let Some(op) = op else {
            self.agent_key.error = "That isn’t a key request for one of your agents.".into();
            cx.notify();
            return;
        };
        self.submit_agent_op(op, |v| &mut v.agent_key, cx);
    }
    /// Submits `op` from `form`; done, the form clears and the agents are
    /// read again.
    fn submit_agent_op(
        &mut self,
        op: identity::Op,
        form: impl Fn(&mut Settings) -> &mut Form + 'static,
        cx: &mut Context<Self>,
    ) {
        if form(self).busy {
            return;
        }
        let pending = form(self);
        pending.busy = true;
        pending.error.clear();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.host().ask::<Submit<Identity>>(op).await;
            let _ = this.update(cx, |view, cx| {
                let form = form(view);
                form.busy = false;
                match result {
                    Ok(_) => form.text.clear(),
                    Err(refusal) => form.error = format!("Couldn’t do that: {}", refusal.message),
                }
                view.read_account(cx);
            });
        })
        .detach();
    }
    fn network(&self, cx: &mut Context<Self>, theme: &Theme) -> impl IntoElement {
        let choices = div()
            .id("settings/ttl")
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .children(TTL.into_iter().enumerate().map(|(i, days)| {
                button(
                    format!("settings/ttl/{days}"),
                    format!("{}{} days", if self.ttl == i { "✓ " } else { "" }, days),
                    theme,
                )
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                    v.ttl = i;
                    cx.notify();
                }))
            }));
        let mint = cx.listener(|v: &mut Self, _: &ClickEvent, _, cx| {
            v.copied.clear();
            v.invite = cx.load(
                cx.host().ask::<InviteCreate>(CreateInvite {
                    ttl_days: TTL[v.ttl],
                }),
                |v| &mut v.invite,
            );
            cx.notify();
        });
        let mut body = div()
            .id("settings/network/body")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .child(secondary(
                "settings/invite/help",
                "Invite people · expires after",
                theme,
            ))
            .child(choices)
            .child(
                button("settings/invite/mint", "Mint invite", theme)
                    .aria_disabled(self.invite.is_loading())
                    .when(self.invite.is_loading(), |button| {
                        button.opacity(0.5).tab_stop(false)
                    })
                    .when(!self.invite.is_loading(), |button| button.on_click(mint)),
            );
        match &self.invite {
            Loadable::Ready(invite) => {
                body = body
                    .child(
                        div()
                            .id("settings/invite/blob")
                            .w_full()
                            .font_family(design::fonts::FAMILY_MONO)
                            .text_size(design::text::SECONDARY)
                            .child(invite.invite.clone()),
                    )
                    .children(invite.notes.iter().enumerate().map(|(i, n)| {
                        secondary(format!("settings/invite/note/{i}"), &n.message, theme)
                    }));
                let blob = invite.invite.clone();
                body = body.child(
                    button("settings/invite/copy", "Copy invite", theme).on_click(cx.listener(
                        move |v, _: &ClickEvent, _, cx| {
                            let work = cx.host().ask::<ClipboardWrite>(blob.clone());
                            cx.spawn(async move |this, cx| {
                                let result = work.await;
                                let _ = this.update(cx, |v, cx| {
                                    v.copied = match result {
                                        Ok(()) => "Copied".into(),
                                        Err(e) => e.message,
                                    };
                                    cx.notify();
                                });
                            })
                            .detach();
                            v.copied.clear();
                        },
                    )),
                );
            }
            Loadable::Failed(e) => body = body.child(refusal("invite", &e.message, theme)),
            Loadable::Loading(_) => {
                body = body.child(secondary(
                    "settings/invite/loading",
                    "Minting invite…",
                    theme,
                ))
            }
            Loadable::Idle => {
                body = body.child(secondary(
                    "settings/invite/empty",
                    "No invite minted yet.",
                    theme,
                ))
            }
        }
        body.child(secondary("settings/invite/copied", &self.copied, theme))
    }
}
impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let node = self.node(cx, &theme);
        let account = self.account(cx, &theme);
        let network = self.network(cx, &theme);
        div()
            .id("settings")
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(design::text::BODY)
            .child(
                div()
                    .id("settings/title")
                    .text_size(design::text::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .role(Role::Heading)
                    .aria_level(1)
                    .child("Settings"),
            )
            .child(
                div()
                    .id("settings/scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .id("settings/sections")
                            .flex()
                            .flex_col()
                            .gap_2()
                            .w_full()
                            .child(section("node", "Node", node, &theme))
                            .child(section("account", "Account", account, &theme))
                            .child(section("network", "Network", network, &theme))
                            .child(section(
                                "app",
                                "App",
                                div()
                                    .id("settings/app/body")
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .w_full()
                                    .child(secondary(
                                        "settings/theme",
                                        format!(
                                            "Appearance follows the host theme · {}",
                                            if theme.dark { "Dark" } else { "Light" }
                                        ),
                                        &theme,
                                    ))
                                    .child(line(
                                        "endpoint",
                                        "Endpoint",
                                        if self.session.endpoint.is_empty() {
                                            "Not connected"
                                        } else {
                                            &self.session.endpoint
                                        },
                                    ))
                                    .child(secondary(
                                        "settings/native",
                                        "Endpoint and keystore are managed by the host app.",
                                        &theme,
                                    )),
                                &theme,
                            )),
                    ),
            )
    }
}
fn section(key: &str, title: &str, body: impl IntoElement, theme: &Theme) -> impl IntoElement {
    div()
        .id(format!("settings/{key}/card"))
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .p_3()
        .child(
            div()
                .id(format!("settings/{key}/section"))
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .id(format!("settings/{key}/heading"))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl_2()
                        .pr_1()
                        .w_full()
                        .child(
                            secondary(format!("settings/{key}/heading/label"), title, theme)
                                .font_weight(FontWeight::MEDIUM)
                                .w_full(),
                        ),
                )
                .child(body),
        )
}
fn line(key: &str, label: &str, value: &str) -> Stateful<Div> {
    div()
        .id(format!("settings/{key}"))
        .w_full()
        .child(format!("{label}: {value}"))
}
fn refusal(key: &str, sentence: &str, theme: &Theme) -> impl IntoElement {
    div()
        .id(format!("settings/{key}/refused"))
        .bg(theme.danger_soft)
        .border_1()
        .border_color(theme.danger)
        .px_3()
        .py_2()
        .child(
            div()
                .id(format!("settings/{key}/why"))
                .w_full()
                .child(sentence.to_owned()),
        )
}
fn secondary(id: impl Into<String>, text: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text.into())
}
/// A labelled text field over `form`.
fn field(
    id: &str,
    label: &str,
    form: &Form,
    theme: &Theme,
    typed: impl Fn(&String, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Input::new(id.to_owned())
        .h(px(28.))
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .value(form.text.clone())
        .placeholder(label.to_owned())
        .label(label.to_owned())
        .disabled(form.busy)
        .on_input(typed)
}
/// A form's button: disabled, and saying so, while its submit is in flight.
fn submit(
    id: &str,
    label: &str,
    busy_label: &str,
    busy: bool,
    theme: &Theme,
    pressed: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    button(id.to_owned(), if busy { busy_label } else { label }, theme)
        .aria_disabled(busy)
        .when(busy, |b| b.opacity(0.5).tab_stop(false))
        .when(!busy, |b| b.on_click(pressed))
}
fn button(id: impl Into<String>, label: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .hover(|style| style.bg(theme.surface_raised))
        .role(Role::Button)
        .focusable()
        .child(label.into())
}
export_view!(
    Settings,
    "Settings",
    "Node, account, invites and app preferences.",
    [
        "chain",
        "module",
        "op",
        "invite",
        "host",
        "clock",
        "clipboard"
    ]
);
#[cfg(test)]
mod tests;
