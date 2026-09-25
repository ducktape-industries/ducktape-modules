//! Nodes: the validator set the `valset` program answers with, and every
//! membership it holds — the key, the address it is reached at, and whether
//! it validates or only resides.
//!
//! The contract is borsh and this view's state is a serde snapshot, so a
//! reply is folded to rows as it lands.
use abi::hex;
use ducktape_view_guest::design;
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::methods::Changes;
use ducktape_view_guest::methods::Query;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, Host, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Task, Theme, View, Window, div, px,
};
mod components;
use components::Section;
use futures::StreamExt;

use serde::{Deserialize, Serialize};

use valset::view::Valset;

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
        let mut stream = cx.host().subscribe::<Changes<Valset>>(());
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
        div()
            .id("nodes")
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
                    .id("nodes-head")
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("nodes-title")
                            .flex_1()
                            .text_size(design::text::TITLE)
                            .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                            .role(ducktape_view_guest::Role::Heading)
                            .aria_level(1)
                            .child("Nodes"),
                    )
                    .child(
                        div()
                            .text_size(design::text::CAPTION)
                            .text_color(theme.muted)
                            .child(self.count()),
                    ),
            )
            .child(self.body(cx, &theme))
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
                design::plural(set.validators.len() as u64, "validator", "validators"),
                design::plural(set.members.len() as u64, "member", "members"),
            ),
            None => String::new(),
        }
    }

    /// The four states of the set: loading, refused, empty, ready.
    fn body(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.set {
            Loaded::Idle | Loaded::Loading(_) => div()
                .id("nodes-loading")
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("Reading the validator set…")
                .into_any_element(),
            Loaded::Failed(refusal) => {
                let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
                design::refused("nodes", refusal.sentence.clone(), theme, retry).into_any_element()
            }
            Loaded::Ready(set) if set.members.is_empty() && set.validators.is_empty() => {
                design::empty_state(
                    "nodes-empty",
                    "No members",
                    "The validator set of this network is empty.",
                    theme,
                )
                .into_any_element()
            }
            Loaded::Ready(set) => div()
                .id("nodes-list")
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
            .id("nodes-no-validators")
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child("No key validates on this network.")
            .into_any_element();
    }
    div()
        .id("nodes-validators")
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
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child((index + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::SECONDARY)
                        .child(design::short_hex(validator)),
                )
        }))
        .into_any_element()
}

fn members(members: &[Member], theme: &Theme) -> impl IntoElement {
    div()
        .id("nodes-members")
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
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::SECONDARY)
                        .child(design::short_hex(&member.key)),
                )
                .child(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .text_size(design::text::SECONDARY)
                        .child(member.address.clone()),
                )
                .child(
                    div()
                        .px_1()
                        .py_0p5()
                        .bg(background)
                        .text_color(foreground)
                        .text_size(design::text::CAPTION)
                        .child(member.standing.clone()),
                )
        }))
}

/// The set, read twice: the consensus keys the program answers, then every
/// membership behind them.
async fn set(host: Host) -> Result<Set, Refusal> {
    let validators = match host.ask::<Query<Valset>>(valset::Query::Validators).await? {
        valset::Reply::Validators(keys) => keys,
        other => return Err(unexpected(&other)),
    };
    let mut memberships = Vec::new();
    let mut after = None;
    loop {
        let page = store::PageRequest { after, limit: None };
        let reply = match host
            .ask::<Query<Valset>>(valset::Query::Memberships { page })
            .await?
        {
            valset::Reply::Memberships(reply) => reply,
            other => return Err(unexpected(&other)),
        };
        memberships.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    Ok(Set {
        validators: validators.iter().map(|key| hex(key)).collect(),
        members: memberships
            .iter()
            .map(|membership| Member {
                key: hex(&membership.key),
                address: membership.address.clone(),
                standing: match membership.role {
                    valset::Role::Validator => "Validator",
                    valset::Role::Resident => "Resident",
                }
                .into(),
                validator: membership.role == valset::Role::Validator,
            })
            .collect(),
    })
}

fn unexpected(reply: &impl std::fmt::Debug) -> Refusal {
    malformed(format!("{} answered {reply:?}", valset::MODULE))
}

export_view!(
    Nodes,
    "Nodes",
    "The validator set of this network and every membership behind it.",
    ["program", "host"]
);

#[cfg(test)]
mod tests;
