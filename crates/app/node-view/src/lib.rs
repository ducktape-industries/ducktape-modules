//! Nodes: the validator set the `valset` program answers with, and every
//! membership it holds — the key, the address it is reached at, and whether
//! it validates or only resides.
//!
//! The contract is borsh and this view's state is a serde snapshot, so a
//! reply is folded to rows as it lands.
use abi::hex;
use ducktape_view_guest::caps::{Program, QueryBytes};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Live, Loaded};
use ducktape_view_guest::{ClickEvent, InteractiveElement, StatefulInteractiveElement};
use ducktape_view_guest::{
    AnyElement, Context, ElementId, Host, IntoElement, ParentElement, Render,
    Styled, Task, Theme, View, Window, div, px,
};
mod components;
use components::{EmptyState, Section};
use futures::StreamExt;
use modules::valset;
use serde::{Deserialize, Serialize};

/// The validator set's query surface, as this view reads it.
struct Valset;
impl Program for Valset {
    const PROGRAM: &'static str = valset::PROGRAM;
    type Request = valset::Query;
    type Reply = valset::Reply;
}

#[derive(Serialize, Deserialize, Default)]
pub struct Nodes {
    set: Loaded<Set>,
    #[serde(skip)]
    live: Option<Task<()>>,
}

/// What the screen shows: the consensus set as valset answers it, and the
/// memberships behind it.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Set {
    /// the validator keys, hex, in the order the program answers them
    validators: Vec<String>,
    members: Vec<Member>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Member {
    key: String,
    address: String,
    standing: String,
    validator: bool,
}

impl View for Nodes {
    const PREFERRED_WINDOW_SIZE: &'static str = "680,620";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut stream = cx.host().subscribe::<Live>(valset::PROGRAM.into());
        self.live = Some(cx.spawn(async move |this, cx| {
            while stream.next().await.is_some() {
                if this.update(cx, |view, cx| view.read(cx)).is_err() {
                    break;
                }
            }
        }));
        self.read(cx);
    }
}

impl Render for Nodes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let theme = &theme;
        div()
            .id(ElementId::Name("nodes".into()))
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                div()
                    .id(ElementId::Name("nodes-head".into()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_lg().child("Nodes"))
                    .child(div().text_sm().text_color(theme.muted).child(self.count())),
            )
            .child(self.body(cx, theme))
    }
}

impl Nodes {
    /// One read of the set — the boot, a retry, a restore, a live bump.
    /// What is already on screen stays there while it runs.
    fn read(&mut self, cx: &mut Context<Self>) {
        match self.set.ready() {
            Some(_) => cx.refresh(set(cx.host()), |view, set, _| view.set = Loaded::Ready(set)),
            None => self.set = cx.load(set(cx.host()), |view| &mut view.set),
        }
        cx.notify();
    }

    fn count(&self) -> String {
        match self.set.ready() {
            Some(set) => format!(
                "{} · {}",
                plural(set.validators.len(), "validator", "validators"),
                plural(set.members.len(), "member", "members"),
            ),
            None => String::new(),
        }
    }

    /// The four states of the set: loading, refused, empty, ready.
    fn body(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.set {
            Loaded::Idle | Loaded::Loading(_) => div()
                .id(ElementId::Name("nodes-loading".into()))
                .text_sm()
                .text_color(theme.muted)
                .child("Reading the validator set…")
                .into_any_element(),
            Loaded::Failed(refusal) => {
                let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
                div()
                    .id(ElementId::Name("nodes-refused".into()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.danger)
                    .bg(theme.danger_soft)
                    .child(refusal.sentence.clone())
                    .child(
                        div()
                            .id(ElementId::Name("nodes-retry".into()))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme.surface)
                            .hover(|s| s.bg(theme.surface_raised))
                            .role(ducktape_view_guest::Role::Button).focusable().on_click(retry)
                            .child("Retry"),
                    )
                    .into_any_element()
            }
            Loaded::Ready(set) if set.members.is_empty() && set.validators.is_empty() => {
                EmptyState::new("nodes-empty", "No members", "The validator set of this network is empty.")
                .into_any_element()
            }
            Loaded::Ready(set) => div()
                .id(ElementId::Name("nodes-list".into()))
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .child(Section::new("nodes-set-header", "Validator set"))
                .child(validators(&set.validators, theme))
                .child(Section::new("nodes-members-header", "Memberships"))
                .child(members(&set.members, theme))
                .into_any_element(),
        }
    }
}

fn validators(validators: &[String], theme: &Theme) -> AnyElement {
    if validators.is_empty() {
        return div()
            .id(ElementId::Name("nodes-no-validators".into()))
            .text_sm()
            .text_color(theme.muted)
            .child("No key validates on this network.")
            .into_any_element();
    }
    div()
        .id(ElementId::Name("nodes-validators".into()))
        .flex()
        .flex_col()
        .gap_2()
        .children(validators.iter().enumerate().map(|(index, validator)| {
            div()
                .id(ElementId::named_usize("nodes-validator", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(28.))
                        .text_sm()
                        .text_color(theme.muted)
                        .child((index + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .font_family("JetBrains Mono")
                        .text_sm()
                        .child(short_id(validator, 16)),
                )
        }))
        .into_any_element()
}

fn members(members: &[Member], theme: &Theme) -> impl IntoElement {
    div()
        .id(ElementId::Name("nodes-members".into()))
        .flex()
        .flex_col()
        .gap_2()
        .children(members.iter().enumerate().map(|(index, member)| {
            let (foreground, background) = if member.validator {
                (theme.success, theme.success_soft)
            } else {
                (theme.muted, theme.surface_raised)
            };
            div()
                .id(ElementId::named_usize("nodes-member", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .font_family("JetBrains Mono")
                        .text_sm()
                        .child(short_id(&member.key, 16)),
                )
                .child(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .text_sm()
                        .child(member.address.clone()),
                )
                .child(
                    div()
                        .px_1()
                        .py_0p5()
                        .rounded_sm()
                        .bg(background)
                        .text_color(foreground)
                        .text_xs()
                        .child(member.standing.clone()),
                )
        }))
}

fn short_id(id: &str, keep: usize) -> String {
    let mut head: String = id.chars().take(keep).collect();
    if id.chars().count() > keep {
        head.push('…');
    }
    head
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The set, read twice: the consensus keys the program answers, then every
/// membership behind them.
async fn set(host: Host) -> Result<Set, Refusal> {
    let validators = match host
        .ask::<QueryBytes<Valset>>(valset::Query::Validators)
        .await?
    {
        valset::Reply::Validators(keys) => keys,
        other => return Err(unexpected(&other)),
    };
    let memberships = match host
        .ask::<QueryBytes<Valset>>(valset::Query::Memberships)
        .await?
    {
        valset::Reply::Memberships(memberships) => memberships,
        other => return Err(unexpected(&other)),
    };
    Ok(Set {
        validators: validators.iter().map(|key| hex(key)).collect(),
        members: memberships
            .iter()
            .map(|membership| Member {
                key: hex(&membership.key),
                address: membership.address.clone(),
                standing: match membership.standing {
                    valset::Standing::Validator => "Validator",
                    valset::Standing::Resident => "Resident",
                }
                .into(),
                validator: membership.standing == valset::Standing::Validator,
            })
            .collect(),
    })
}

fn unexpected(reply: &impl std::fmt::Debug) -> Refusal {
    malformed(format!("{} answered {reply:?}", valset::PROGRAM))
}

export_view!(
    Nodes,
    "Nodes",
    "The validator set of this network and every membership behind it.",
    ["rpc"]
);

#[cfg(test)]
mod tests;
