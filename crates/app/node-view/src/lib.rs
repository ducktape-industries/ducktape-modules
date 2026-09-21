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
use ducktape_view_guest::wire::{Length, Node, kit, kit::Tone};
use ducktape_view_guest::{Context, Host, Render, Task, View, Window};
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
    const PREFERRED_WINDOW_SIZE: &'static str = "680x620";

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Node {
        let key = "nodes";
        let head = kit::centered_row(
            format!("{key}/head"),
            [
                kit::fill_width(kit::title(format!("{key}/title"), "Nodes")),
                kit::caption(format!("{key}/count"), self.count()),
            ],
        );
        kit::page(key, [head, self.body(key, cx)])
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
                kit::plural(set.validators.len() as u64, "validator", "validators"),
                kit::plural(set.members.len() as u64, "member", "members"),
            ),
            None => String::new(),
        }
    }

    /// The four states of the set: loading, refused, empty, ready.
    fn body(&self, key: &str, cx: &mut Context<Self>) -> Node {
        match &self.set {
            Loaded::Idle | Loaded::Loading(_) => {
                kit::secondary(format!("{key}/loading"), "Reading the validator set…")
            }
            Loaded::Failed(refusal) => {
                let retry = cx.listener(|view, _: &(), _, cx| view.read(cx));
                kit::notice(
                    format!("{key}/refused"),
                    kit::column(
                        format!("{key}/refused/body"),
                        [
                            kit::wrapping(kit::text(
                                format!("{key}/refused/why"),
                                refusal.sentence.clone(),
                            )),
                            kit::action(format!("{key}/retry"), "Retry", Some(retry)),
                        ],
                    ),
                    Tone::Danger,
                )
            }
            Loaded::Ready(set) if set.members.is_empty() && set.validators.is_empty() => {
                kit::empty_state(
                    format!("{key}/empty"),
                    "No members",
                    "The validator set of this network is empty.",
                )
            }
            Loaded::Ready(set) => kit::scroll(
                format!("{key}/list"),
                kit::column(
                    format!("{key}/sections"),
                    [
                        kit::section_row(&format!("{key}/set-header"), "Validator set", None),
                        validators(key, &set.validators),
                        kit::section_row(&format!("{key}/members-header"), "Memberships", None),
                        members(key, &set.members),
                    ],
                ),
            ),
        }
    }
}

fn validators(key: &str, validators: &[String]) -> Node {
    if validators.is_empty() {
        return kit::secondary(
            format!("{key}/no-validators"),
            "No key validates on this network.",
        );
    }
    kit::column(
        format!("{key}/validators"),
        validators.iter().enumerate().map(|(index, validator)| {
            kit::centered_row(
                format!("{key}/validator/{index}"),
                [
                    kit::width(
                        kit::secondary(
                            format!("{key}/validator/{index}/n"),
                            format!("{}", index + 1),
                        ),
                        Length::Fixed(28.),
                    ),
                    kit::fill_width(kit::mono(
                        format!("{key}/validator/{index}/key"),
                        kit::short_id(validator, 16),
                    )),
                ],
            )
        }),
    )
}

fn members(key: &str, members: &[Member]) -> Node {
    kit::column(
        format!("{key}/members"),
        members.iter().enumerate().map(|(index, member)| {
            kit::centered_row(
                format!("{key}/member/{index}"),
                [
                    kit::fill_width(kit::mono(
                        format!("{key}/member/{index}/key"),
                        kit::short_id(&member.key, 16),
                    )),
                    kit::truncated(format!("{key}/member/{index}/address"), &member.address, 28),
                    kit::badge(
                        format!("{key}/member/{index}/standing"),
                        &member.standing,
                        if member.validator {
                            Tone::Success
                        } else {
                            Tone::Neutral
                        },
                    ),
                ],
            )
        }),
    )
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
