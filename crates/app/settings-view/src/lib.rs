//! Settings reads node system facts; account contracts stay in the view.
mod account;
mod api;
use account::{Account, read_account};
use api::*;
use ducktape_view_guest::caps::{ClipboardWrite, Ticks};
use ducktape_view_guest::view::{Live, Loaded};
use ducktape_view_guest::wire::{Node, kit, kit::Tone};
use ducktape_view_guest::{Context, Render, Task, View, Window, export_view};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    session: Session,
    status: Loaded<Status>,
    account: Loaded<Option<Account>>,
    invite: Loaded<Invite>,
    ttl: usize,
    copied: String,
    #[serde(skip)]
    watches: Vec<Task<()>>,
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
        let mut props = cx.host().subscribe::<Props>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while let Some(reply) = props.next().await {
                if this
                    .update(cx, |view, cx| {
                        match reply {
                            Ok(session) => {
                                view.session = session;
                                view.read_account(cx);
                            }
                            Err(refusal) => view.account = Loaded::Failed(refusal),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut live = cx.host().subscribe::<Live>(modules::valset::PROGRAM.into());
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
        let mut ticks = cx.host().subscribe::<Ticks>(1000);
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
            cx.refresh(cx.host().ask::<NodeStatus>(()), |view, status, _| {
                view.status = Loaded::Ready(status)
            });
        } else if !self.status.is_loading() {
            self.status = cx.load(cx.host().ask::<NodeStatus>(()), |v| &mut v.status);
        }
        cx.notify();
    }
    fn read_account(&mut self, cx: &mut Context<Self>) {
        self.account = cx.load(read_account(cx.host(), self.session.account.clone()), |v| {
            &mut v.account
        });
        cx.notify();
    }
    fn node(&self, cx: &mut Context<Self>) -> Node {
        match &self.status {
            Loaded::Ready(s) => kit::column(
                "settings/node/data",
                [
                    line("network", "Network", &s.network),
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
                        &kit::short_id(&abi::hex(&s.identity), 20),
                    ),
                    line("contract", "Contract version", &s.contract.to_string()),
                ],
            ),
            Loaded::Failed(e) => kit::column(
                "settings/node/error",
                [
                    refusal("node", &e.sentence),
                    kit::action(
                        "settings/node/retry",
                        "Retry node",
                        Some(cx.listener(|v, _: &(), _, cx| v.read(cx))),
                    ),
                ],
            ),
            _ => kit::secondary("settings/node/loading", "Reading node status…"),
        }
    }
    fn account(&self, cx: &mut Context<Self>) -> Node {
        match &self.account {
            Loaded::Ready(Some(a)) => kit::column(
                "settings/account/data",
                std::iter::once(line(
                    "who",
                    "Who I am",
                    &match a.number {
                        Some(number) => format!("{} · account {number}", a.name),
                        None => a.name.clone(),
                    },
                ))
                .chain(a.keys.iter().enumerate().map(|(i, k)| {
                    line(
                        &format!("key/{i}"),
                        &k.label,
                        &format!("{} · {}", k.key, k.standing),
                    )
                })),
            ),
            Loaded::Ready(None) => kit::empty_state(
                "settings/account/empty",
                "No account",
                "No host key is selected.",
            ),
            Loaded::Failed(e) => kit::column(
                "settings/account/error",
                [
                    refusal("account", &e.sentence),
                    kit::action(
                        "settings/account/retry",
                        "Retry account",
                        Some(cx.listener(|v, _: &(), _, cx| v.read_account(cx))),
                    ),
                ],
            ),
            _ => kit::secondary("settings/account/loading", "Reading your account…"),
        }
    }
    fn network(&self, cx: &mut Context<Self>) -> Node {
        let choices = kit::centered_row(
            "settings/ttl",
            TTL.into_iter().enumerate().map(|(i, days)| {
                let listener = cx.listener(move |v, _: &(), _, cx| {
                    v.ttl = i;
                    cx.notify();
                });
                kit::action(
                    format!("settings/ttl/{days}"),
                    &format!("{}{} days", if self.ttl == i { "✓ " } else { "" }, days),
                    Some(listener),
                )
            }),
        );
        let mint = cx.listener(|v: &mut Self, _: &(), _, cx| {
            v.copied.clear();
            v.invite = cx.load(
                cx.host().ask::<MintInvite>(Mint {
                    ttl_days: TTL[v.ttl],
                }),
                |v| &mut v.invite,
            );
            cx.notify();
        });
        let mut body = vec![
            kit::secondary("settings/invite/help", "Invite people · expires after"),
            choices,
            kit::action(
                "settings/invite/mint",
                "Mint invite",
                (!self.invite.is_loading()).then_some(mint),
            ),
        ];
        match &self.invite {
            Loaded::Ready(invite) => {
                body.push(kit::wrapping(kit::mono(
                    "settings/invite/blob",
                    &invite.invite,
                )));
                body.extend(invite.notes.iter().enumerate().map(|(i, n)| {
                    kit::secondary(format!("settings/invite/note/{i}"), &n.sentence)
                }));
                let blob = invite.invite.clone();
                body.push(kit::action(
                    "settings/invite/copy",
                    "Copy invite",
                    Some(cx.listener(move |v, _: &(), _, cx| {
                        let work = cx.host().ask::<ClipboardWrite>(blob.clone());
                        cx.spawn(async move |this, cx| {
                            let result = work.await;
                            let _ = this.update(cx, |v, cx| {
                                v.copied = match result {
                                    Ok(()) => "Copied".into(),
                                    Err(e) => e.sentence,
                                };
                                cx.notify();
                            });
                        })
                        .detach();
                        v.copied.clear();
                    })),
                ));
            }
            Loaded::Failed(e) => body.push(refusal("invite", &e.sentence)),
            Loaded::Loading(_) => {
                body.push(kit::secondary("settings/invite/loading", "Minting invite…"))
            }
            Loaded::Idle => body.push(kit::secondary(
                "settings/invite/empty",
                "No invite minted yet.",
            )),
        }
        body.push(kit::secondary("settings/invite/copied", &self.copied));
        kit::column("settings/network/body", body)
    }
}
impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Node {
        kit::set_dark(self.session.dark);
        let node = self.node(cx);
        let account = self.account(cx);
        let network = self.network(cx);
        kit::page(
            "settings",
            [
                kit::title("settings/title", "Settings"),
                kit::scroll(
                    "settings/scroll",
                    kit::column(
                        "settings/sections",
                        [
                            section("node", "Node", node),
                            section("account", "Account", account),
                            section("network", "Network", network),
                            section(
                                "app",
                                "App",
                                kit::column(
                                    "settings/app/body",
                                    [
                                        kit::secondary(
                                            "settings/theme",
                                            format!(
                                                "Appearance follows the host theme · {}",
                                                if self.session.dark { "Dark" } else { "Light" }
                                            ),
                                        ),
                                        line(
                                            "endpoint",
                                            "Endpoint",
                                            if self.session.endpoint.is_empty() {
                                                "Not connected"
                                            } else {
                                                &self.session.endpoint
                                            },
                                        ),
                                        kit::secondary(
                                            "settings/native",
                                            "Endpoint and keystore are managed by the host app.",
                                        ),
                                    ],
                                ),
                            ),
                        ],
                    ),
                ),
            ],
        )
    }
}
fn section(key: &str, title: &str, body: Node) -> Node {
    kit::card(
        format!("settings/{key}/card"),
        kit::column(
            format!("settings/{key}/section"),
            [
                kit::section_row(&format!("settings/{key}/heading"), title, None),
                body,
            ],
        ),
    )
}
fn line(key: &str, label: &str, value: &str) -> Node {
    kit::wrapping(kit::text(
        format!("settings/{key}"),
        format!("{label}: {value}"),
    ))
}
fn refusal(key: &str, sentence: &str) -> Node {
    kit::notice(
        format!("settings/{key}/refused"),
        kit::wrapping(kit::text(format!("settings/{key}/why"), sentence)),
        Tone::Danger,
    )
}
export_view!(
    Settings,
    "Settings",
    "Node, account, invites and app preferences.",
    ["rpc", "host", "clock", "clipboard"]
);
#[cfg(test)]
mod tests;
