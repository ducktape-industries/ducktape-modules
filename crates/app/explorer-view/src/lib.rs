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
use ducktape_view_guest::{
    AnyElement, ClickEvent, Context, ElementId, Host, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Task, Theme, View, Window, div, px,
};
mod components;
use components::{EmptyState, Section};
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
    const PREFERRED_WINDOW_SIZE: &'static str = "760,640";

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        div()
            .id(ElementId::Name("explorer".into()))
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
                    .id(ElementId::Name("explorer-head".into()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("explorer-title")
                            .flex_1()
                            .text_size(px(16.))
                            .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                            .role(ducktape_view_guest::Role::Heading)
                            .aria_level(1)
                            .child("Programs"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(self.count()),
                    ),
            )
            .child(self.body(cx, &theme))
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
            Some(network) => plural(network.programs.len(), "program", "programs"),
            None => String::new(),
        }
    }

    /// The four states of the registry: loading, refused, empty, ready.
    fn body(&self, cx: &mut Context<Self>, theme: &Theme) -> AnyElement {
        match &self.network {
            Loaded::Idle | Loaded::Loading(_) => div()
                .id(ElementId::Name("explorer-loading".into()))
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Reading the registry…")
                .into_any_element(),
            Loaded::Failed(refusal) => {
                let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
                div()
                    .id(ElementId::Name("explorer-refused".into()))
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
                            .id(ElementId::Name("explorer-retry".into()))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme.surface)
                            .hover(|s| s.bg(theme.surface_raised))
                            .role(ducktape_view_guest::Role::Button)
                            .focusable()
                            .on_click(retry)
                            .child("Retry"),
                    )
                    .into_any_element()
            }
            Loaded::Ready(network) if network.programs.is_empty() && network.changes.is_empty() => {
                EmptyState::new(
                    "explorer-empty",
                    "No programs",
                    "The registry of this network runs nothing yet.",
                )
                .into_any_element()
            }
            Loaded::Ready(network) => div()
                .id(ElementId::Name("explorer-list".into()))
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .child(Section::new("explorer-running-header", "Running"))
                .child(programs(&network.programs, theme))
                .child(Section::new("explorer-scheduled-header", "Scheduled"))
                .child(changes(&network.changes, theme))
                .into_any_element(),
        }
    }
}

fn programs(programs: &[Entry], theme: &Theme) -> AnyElement {
    if programs.is_empty() {
        return div()
            .id(ElementId::Name("explorer-no-programs".into()))
            .text_size(px(12.))
            .text_color(theme.muted)
            .child("No program runs here yet.")
            .into_any_element();
    }
    div()
        .id(ElementId::Name("explorer-programs".into()))
        .flex()
        .flex_col()
        .gap_2()
        .children(programs.iter().map(|entry| {
            div()
                .id(ElementId::Name(
                    format!("explorer-program-{}", entry.program).into(),
                ))
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().truncate().child(entry.program.clone()))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted)
                        .child(plural(entry.params, "param byte", "param bytes")),
                )
                .child(
                    div()
                        .font_family("JetBrains Mono")
                        .text_size(px(12.))
                        .child(short_id(&entry.code, 12)),
                )
        }))
        .into_any_element()
}

fn changes(changes: &[Change], theme: &Theme) -> AnyElement {
    if changes.is_empty() {
        return div()
            .id(ElementId::Name("explorer-no-changes".into()))
            .text_size(px(12.))
            .text_color(theme.muted)
            .child("Nothing is scheduled against the registry.")
            .into_any_element();
    }
    div()
        .id(ElementId::Name("explorer-changes".into()))
        .flex()
        .flex_col()
        .gap_2()
        .children(changes.iter().enumerate().map(|(index, change)| {
            let (foreground, background) = if change.verb == "Remove" {
                (theme.danger, theme.danger_soft)
            } else {
                (theme.accent_foreground, theme.accent_soft)
            };
            let mut row = div()
                .id(ElementId::named_usize("explorer-change", index))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px_1()
                        .py_0p5()
                        .rounded_sm()
                        .bg(background)
                        .text_color(foreground)
                        .text_size(px(11.))
                        .child(change.verb.clone()),
                )
                .child(div().flex_1().truncate().child(change.program.clone()))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted)
                        .child(format!("at {}", change.height)),
                );
            if let Some(code) = &change.code {
                row = row.child(
                    div()
                        .font_family("JetBrains Mono")
                        .text_size(px(12.))
                        .child(short_id(code, 12)),
                );
            }
            row
        }))
        .into_any_element()
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
