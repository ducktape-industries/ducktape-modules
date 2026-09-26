//! The Settings screen: Node, Account, Network and App sections, each in
//! its own card. `render` reads the state and changes nothing; presses land
//! in `actions.rs`.
use ducktape_view_guest::design;
use ducktape_view_guest::prelude::*;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Div, FontWeight, Stateful};

use crate::Settings;
use crate::state::{Form, Problem, TTL};

mod account;

/// The widest a form or a note runs.
const FORM_W: Pixels = px(420.);

pub(crate) fn render(view: &Settings, cx: &mut Context<Settings>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let node = node(view, cx, &theme);
    let account = account::account(view, cx, &theme);
    let network = network(view, cx, &theme);
    let sections = div()
        .id("settings/sections")
        .flex()
        .flex_col()
        .gap_2()
        .w_full()
        .child(section("node", "Node", node, &theme))
        .child(section("account", "Account", account, &theme))
        .child(section("network", "Network", network, &theme))
        .child(section("app", "App", app(view, &theme), &theme));
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
        .child(design::heading("settings/title", "Settings", 1, &theme))
        .child(
            div()
                .id("settings/scroll")
                .flex_1()
                .overflow_y_scroll()
                .child(sections),
        )
}

fn node(view: &Settings, cx: &mut Context<Settings>, theme: &Theme) -> AnyElement {
    match &view.status {
        Loadable::Ready(s) => column("settings/node/data")
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
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.read_status(cx));
            design::refused("settings/node", refusal.message.clone(), theme, retry)
                .into_any_element()
        }
        Loadable::Idle | Loadable::Loading(_) => {
            secondary("settings/node/loading", "Reading node status…", theme).into_any_element()
        }
    }
}

fn network(view: &Settings, cx: &mut Context<Settings>, theme: &Theme) -> impl IntoElement {
    let choices = div()
        .id("settings/ttl")
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .children(TTL.into_iter().enumerate().map(|(i, days)| {
            let mark = if view.ttl == i { "✓ " } else { "" };
            button(
                format!("settings/ttl/{days}"),
                format!("{mark}{days} days"),
                theme,
            )
            .on_click(cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
                v.ttl = i;
                cx.notify();
            }))
        }));
    let mint = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.mint_invite(cx));
    let minting = view.invite.is_loading();
    let body = column("settings/network/body")
        .child(secondary(
            "settings/invite/help",
            "Invite people · expires after",
            theme,
        ))
        .child(choices)
        .child(
            button("settings/invite/mint", "Mint invite", theme)
                .aria_disabled(minting)
                .when(minting, |button| button.opacity(0.5).tab_stop(false))
                .when(!minting, |button| button.on_click(mint)),
        );
    let body = match &view.invite {
        Loadable::Ready(invite) => {
            let copy = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.copy_invite(cx));
            body.child(
                div()
                    .id("settings/invite/blob")
                    .w_full()
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::SECONDARY)
                    .child(invite.invite.clone()),
            )
            .children(invite.notes.iter().enumerate().map(|(i, note)| {
                secondary(format!("settings/invite/note/{i}"), &note.message, theme)
            }))
            .child(button("settings/invite/copy", "Copy invite", theme).on_click(copy))
        }
        Loadable::Failed(refusal) => body.child(refusal_line("invite", &refusal.message, theme)),
        Loadable::Loading(_) => body.child(secondary(
            "settings/invite/loading",
            "Minting invite…",
            theme,
        )),
        Loadable::Idle => body.child(secondary(
            "settings/invite/empty",
            "No invite minted yet.",
            theme,
        )),
    };
    let copied = match &view.copied {
        Loadable::Ready(()) => "Copied".to_owned(),
        Loadable::Failed(refusal) => refusal.message.clone(),
        Loadable::Idle | Loadable::Loading(_) => String::new(),
    };
    body.child(secondary("settings/invite/copied", copied, theme))
}

fn app(view: &Settings, theme: &Theme) -> impl IntoElement {
    let endpoint = match view.session.endpoint.as_str() {
        "" => "Not connected",
        endpoint => endpoint,
    };
    column("settings/app/body")
        .child(secondary(
            "settings/theme",
            format!(
                "Appearance follows the host theme · {}",
                if theme.dark { "Dark" } else { "Light" }
            ),
            theme,
        ))
        .child(line("endpoint", "Endpoint", endpoint))
        .child(secondary(
            "settings/native",
            "Endpoint and keystore are managed by the host app.",
            theme,
        ))
}

fn section(key: &str, title: &str, body: impl IntoElement, theme: &Theme) -> impl IntoElement {
    let heading = div()
        .id(format!("settings/{key}/heading"))
        .h(design::size::CONTROL)
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
        );
    div()
        .id(format!("settings/{key}/card"))
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .p_3()
        .child(
            column(format!("settings/{key}/section"))
                .child(heading)
                .child(body),
        )
}

/// A full-width column of rows.
fn column(id: impl Into<String>) -> Stateful<Div> {
    div().id(id.into()).flex().flex_col().gap_2().w_full()
}

fn line(key: &str, label: &str, value: &str) -> Stateful<Div> {
    div()
        .id(format!("settings/{key}"))
        .w_full()
        .child(format!("{label}: {value}"))
}

/// What stopped a form, as the form words it; `empty` says what to type.
fn problem_text(problem: &Problem, empty: &str) -> String {
    match problem {
        Problem::Empty => empty.to_owned(),
        Problem::NotAKeyRequest => "That isn’t a key request for one of your agents.".into(),
        Problem::Refused(message) => format!("That didn’t go through: {message}"),
    }
}

/// A form's problem, if it has one, in the refusal box under it.
fn problem(key: &str, form: &Form, empty: &str, theme: &Theme) -> Option<Stateful<Div>> {
    let problem = form.problem.as_ref()?;
    Some(refusal_line(key, &problem_text(problem, empty), theme))
}

/// A refused write or mint, under the control that sent it.
fn refusal_line(key: &str, sentence: &str, theme: &Theme) -> Stateful<Div> {
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
) -> Input {
    Input::new(id.to_owned())
        .h(design::size::CONTROL)
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
