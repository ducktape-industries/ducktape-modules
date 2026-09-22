//! Settings reads node system facts; account contracts stay in the view.
mod account;
mod api;
use account::{Account, read_account};
use api::*;
use ducktape_view_guest::caps::{ClipboardWrite, Ticks};
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::{Live, Loaded};
use ducktape_view_guest::{Context, Render, Task, View, Window, export_view};
use ducktape_view_guest::{Div, FontWeight, Stateful};
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
    fn node(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.status {
            Loaded::Ready(s) => div()
                .id("settings/node/data")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .children([
                    line("network", "Network", &s.network),
                    line(
                        "height",
                        "Height / epoch",
                        &format!("{} / {}", s.height, s.epoch),
                    ),
                    line("block", "Block time", &format!("{} ms", s.block_time_ms)),
                    line("tip", "Tip", &abi::hex(&s.tip)),
                    line("identity", "Node identity", &{
                        let id = abi::hex(&s.identity);
                        if id.len() > 20 {
                            format!("{}…", &id[..20])
                        } else {
                            id
                        }
                    }),
                    line("contract", "Contract version", &s.contract.to_string()),
                ])
                .into_any_element(),
            Loaded::Failed(e) => div()
                .id("settings/node/error")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(refusal("node", &e.sentence, theme))
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
            Loaded::Ready(Some(a)) => div()
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
                        &format!("{} · {}", k.key, k.standing),
                    )
                }))
                .into_any_element(),
            Loaded::Ready(None) => div()
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
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child("No account"),
                )
                .child(
                    secondary(
                        "settings/account/empty/detail",
                        "No host key is selected.",
                        theme,
                    )
                    .w_full(),
                )
                .into_any_element(),
            Loaded::Failed(e) => div()
                .id("settings/account/error")
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(refusal("account", &e.sentence, theme))
                .child(
                    button("settings/account/retry", "Retry account", theme)
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.read_account(cx))),
                )
                .into_any_element(),
            _ => secondary("settings/account/loading", "Reading your account…", theme)
                .into_any_element(),
        }
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
                cx.host().ask::<MintInvite>(Mint {
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
            Loaded::Ready(invite) => {
                body = body
                    .child(
                        div()
                            .id("settings/invite/blob")
                            .w_full()
                            .font_family("JetBrains Mono")
                            .text_size(px(12.))
                            .child(invite.invite.clone()),
                    )
                    .children(invite.notes.iter().enumerate().map(|(i, n)| {
                        secondary(format!("settings/invite/note/{i}"), &n.sentence, theme)
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
                                        Err(e) => e.sentence,
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
            Loaded::Failed(e) => body = body.child(refusal("invite", &e.sentence, theme)),
            Loaded::Loading(_) => {
                body = body.child(secondary(
                    "settings/invite/loading",
                    "Minting invite…",
                    theme,
                ))
            }
            Loaded::Idle => {
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
            .text_size(px(13.))
            .child(
                div()
                    .id("settings/title")
                    .text_size(px(16.))
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
        .id(ElementId::Name(format!("settings/{key}/card").into()))
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .rounded_md()
        .p_3()
        .child(
            div()
                .id(ElementId::Name(format!("settings/{key}/section").into()))
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .id(ElementId::Name(format!("settings/{key}/heading").into()))
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
        .id(ElementId::Name(format!("settings/{key}").into()))
        .w_full()
        .child(format!("{label}: {value}"))
}
fn refusal(key: &str, sentence: &str, theme: &Theme) -> impl IntoElement {
    div()
        .id(ElementId::Name(format!("settings/{key}/refused").into()))
        .bg(theme.danger_soft)
        .border_1()
        .border_color(theme.danger)
        .rounded_md()
        .px_3()
        .py_2()
        .child(
            div()
                .id(ElementId::Name(format!("settings/{key}/why").into()))
                .w_full()
                .child(sentence.to_owned()),
        )
}
fn secondary(id: impl Into<String>, text: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into().into()))
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(text.into())
}
fn button(id: impl Into<String>, label: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into().into()))
        .px_2()
        .py_1()
        .rounded_md()
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
    ["rpc", "host", "clock", "clipboard"]
);
#[cfg(test)]
mod tests;
