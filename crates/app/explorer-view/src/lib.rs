//! Explorer: the programs this network runs, as the `module-registry`
//! program answers them, and the changes already scheduled against it.
//!
//! The contract is borsh and this view's state is a serde snapshot, so a
//! reply is folded to rows as it lands.
use abi::hex;
use ducktape_view_guest::caps::{Program, QueryBytes};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Live, Loaded};
use ducktape_view_guest::wire::{Node, kit, kit::Tone};
use ducktape_view_guest::{Context, Host, Render, Task, View, Window};
use futures::StreamExt;
use modules::module_registry as registry;
use serde::{Deserialize, Serialize};

/// The registry's query surface, as this view reads it.
struct Registry;
impl Program for Registry {
    const PROGRAM: &'static str = registry::PROGRAM;
    type Request = registry::Query;
    type Reply = registry::Reply;
}

#[derive(Serialize, Deserialize, Default)]
pub struct Explorer {
    network: Loaded<Network>,
    #[serde(skip)]
    live: Option<Task<()>>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Network {
    programs: Vec<Entry>,
    changes: Vec<Change>,
}

/// One running program: its id and the blob its code lives in.
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    program: String,
    code: String,
    params: usize,
}

/// One change the registry will fold in at a later block.
#[derive(Clone, Serialize, Deserialize)]
struct Change {
    height: u64,
    /// `Set` or `Remove`: what the change does to the program
    verb: String,
    program: String,
    /// the code the change sets, where it sets one
    code: Option<String>,
}

impl View for Explorer {
    const PREFERRED_WINDOW_SIZE: &'static str = "760x640";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut stream = cx.host().subscribe::<Live>(registry::PROGRAM.into());
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

impl Render for Explorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Node {
        let key = "explorer";
        let head = kit::centered_row(
            format!("{key}/head"),
            [
                kit::fill_width(kit::title(format!("{key}/title"), "Programs")),
                kit::caption(format!("{key}/count"), self.count()),
            ],
        );
        kit::page(key, [head, self.body(key, cx)])
    }
}

impl Explorer {
    /// One read of the registry — the boot, a retry, a restore, a live bump.
    /// What is already on screen stays there while it runs.
    fn read(&mut self, cx: &mut Context<Self>) {
        match self.network.ready() {
            Some(_) => cx.refresh(network(cx.host()), |view, network, _| {
                view.network = Loaded::Ready(network)
            }),
            None => self.network = cx.load(network(cx.host()), |view| &mut view.network),
        }
        cx.notify();
    }

    fn count(&self) -> String {
        match self.network.ready() {
            Some(network) => kit::plural(network.programs.len() as u64, "program", "programs"),
            None => String::new(),
        }
    }

    /// The four states of the registry: loading, refused, empty, ready.
    fn body(&self, key: &str, cx: &mut Context<Self>) -> Node {
        match &self.network {
            Loaded::Idle | Loaded::Loading(_) => {
                kit::secondary(format!("{key}/loading"), "Reading the registry…")
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
            Loaded::Ready(network) if network.programs.is_empty() && network.changes.is_empty() => {
                kit::empty_state(
                    format!("{key}/empty"),
                    "No programs",
                    "The registry of this network runs nothing yet.",
                )
            }
            Loaded::Ready(network) => kit::scroll(
                format!("{key}/list"),
                kit::column(
                    format!("{key}/sections"),
                    [
                        kit::section_row(&format!("{key}/running-header"), "Running", None),
                        programs(key, &network.programs),
                        kit::section_row(&format!("{key}/scheduled-header"), "Scheduled", None),
                        changes(key, &network.changes),
                    ],
                ),
            ),
        }
    }
}

fn programs(key: &str, programs: &[Entry]) -> Node {
    if programs.is_empty() {
        return kit::secondary(format!("{key}/no-programs"), "No program runs here yet.");
    }
    kit::column(
        format!("{key}/programs"),
        programs.iter().map(|entry| {
            let row = format!("{key}/program/{}", entry.program);
            kit::centered_row(
                row.clone(),
                [
                    kit::fill_width(kit::truncated(format!("{row}/name"), &entry.program, 32)),
                    kit::caption(
                        format!("{row}/params"),
                        kit::plural(entry.params as u64, "param byte", "param bytes"),
                    ),
                    kit::mono(format!("{row}/code"), kit::short_id(&entry.code, 12)),
                ],
            )
        }),
    )
}

fn changes(key: &str, changes: &[Change]) -> Node {
    if changes.is_empty() {
        return kit::secondary(
            format!("{key}/no-changes"),
            "Nothing is scheduled against the registry.",
        );
    }
    kit::column(
        format!("{key}/changes"),
        changes.iter().enumerate().map(|(index, change)| {
            let row = format!("{key}/change/{index}");
            let mut cells = vec![
                kit::badge(
                    format!("{row}/verb"),
                    &change.verb,
                    match change.verb.as_str() {
                        "Remove" => Tone::Danger,
                        _ => Tone::Accent,
                    },
                ),
                kit::fill_width(kit::truncated(format!("{row}/name"), &change.program, 32)),
                kit::caption(format!("{row}/height"), format!("at {}", change.height)),
            ];
            if let Some(code) = &change.code {
                cells.push(kit::mono(format!("{row}/code"), kit::short_id(code, 12)));
            }
            kit::centered_row(row, cells)
        }),
    )
}

/// What the registry runs and what it will run.
///
/// `At(0)` is the folded set: the registry applies the changes due at or
/// before the height asked, and it answers no height of its own, so there is
/// no "as of now" to ask for — the scheduled list below is what is still to
/// come.
async fn network(host: Host) -> Result<Network, Refusal> {
    let programs = match host
        .ask::<QueryBytes<Registry>>(registry::Query::At(0))
        .await?
    {
        registry::Reply::Programs(programs) => programs,
        other => return Err(unexpected(&other)),
    };
    let mut scheduled = Vec::new();
    let mut after = None;
    loop {
        let page = modules::Page { after, limit: None };
        let reply = match host
            .ask::<QueryBytes<Registry>>(registry::Query::Scheduled { page })
            .await?
        {
            registry::Reply::Scheduled(reply) => reply,
            other => return Err(unexpected(&other)),
        };
        scheduled.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    Ok(Network {
        programs: programs
            .iter()
            .map(|entry| Entry {
                program: entry.program.clone(),
                code: hex(entry.code.digest()),
                params: entry.params.len(),
            })
            .collect(),
        changes: scheduled
            .iter()
            .map(|scheduled| Change {
                height: scheduled.height,
                verb: match scheduled.change {
                    registry::Change::Set(_) => "Set",
                    registry::Change::Remove(_) => "Remove",
                }
                .into(),
                program: scheduled.change.program().to_string(),
                code: match &scheduled.change {
                    registry::Change::Set(entry) => Some(hex(entry.code.digest())),
                    registry::Change::Remove(_) => None,
                },
            })
            .collect(),
    })
}

fn unexpected(reply: &impl std::fmt::Debug) -> Refusal {
    malformed(format!("{} answered {reply:?}", registry::PROGRAM))
}

export_view!(
    Explorer,
    "Explorer",
    "The programs this network runs and the changes scheduled against them.",
    ["rpc"]
);

#[cfg(test)]
mod tests;
