//! Members: every account the `identity` program holds — its number, its
//! name, what controls it and how many keys it carries — with the standing
//! `valset` gives the keys it holds, where it holds one.
//!
//! The contracts are borsh and this view's state is a serde snapshot, so a
//! reply is folded to [`Row`]s as it lands: nothing the programs speak is
//! kept across a snapshot, only what the screen shows.
use ducktape_view_guest::caps::{Program, QueryBytes};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Live, Loaded};
use ducktape_view_guest::{
    App, ClickEvent, Context, ElementId, Host, Input, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, StatefulInteractiveElement, Styled, Task, Theme, View,
    Window, div, px,
};
use futures::StreamExt;
use modules::{Page, identity, valset};
use serde::{Deserialize, Serialize};

/// The identity program's query surface, as this view reads it.
struct Identity;
impl Program for Identity {
    const PROGRAM: &'static str = identity::PROGRAM;
    type Request = identity::Query;
    type Reply = identity::Reply;
}

/// The validator set, read for the standing beside a member.
struct Valset;
impl Program for Valset {
    const PROGRAM: &'static str = valset::PROGRAM;
    type Request = valset::Query;
    type Reply = valset::Reply;
}

#[derive(Serialize, Deserialize, Default)]
pub struct Members {
    rows: Loaded<Vec<Row>>,
    /// what the reader typed into the filter; the rows are never refetched
    /// for it, since the program has no search
    filter: String,
    #[serde(skip)]
    live: Option<Task<()>>,
}

/// One account as this screen shows it.
#[derive(Clone, Serialize, Deserialize)]
struct Row {
    number: u64,
    name: String,
    /// what holds the account: its own keys, another program, or nothing
    control: String,
    keys: usize,
    /// the valset standing of a key this account holds, where it holds one
    standing: Option<String>,
}

impl View for Members {
    const PREFERRED_WINDOW_SIZE: &'static str = "720,640";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut stream = cx.host().subscribe::<Live>(identity::PROGRAM.into());
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

impl Render for Members {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let typed = cx.listener(|view, text: &String, _, cx| {
            view.filter = text.clone();
            cx.notify();
        });
        let body = self.body(cx, &theme);
        div()
            .id(ElementId::Name("members".into()))
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
                    .id(ElementId::Name("members-head".into()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("members-title")
                            .flex_1()
                            .text_size(px(16.))
                            .font_semibold()
                            .role(Role::Heading)
                            .aria_level(1)
                            .child("Members"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(self.count()),
                    ),
            )
            .child(
                Input::new(ElementId::Name("members-filter".into()))
                    .h(px(28.))
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_strong)
                    .bg(theme.surface)
                    .text_color(theme.foreground)
                    .value(self.filter.clone())
                    .placeholder("Filter by name or number")
                    .label("Filter members")
                    .on_input(typed),
            )
            .child(body)
    }
}

impl Members {
    /// One read of both programs — the boot, a retry, a restore, a live
    /// bump. Rows already on screen stay there while it runs, so a bump
    /// never blinks the list back to "Loading"; an empty or refused slot
    /// says it is loading, because it has nothing else to say.
    fn read(&mut self, cx: &mut Context<Self>) {
        match self.rows.ready() {
            Some(_) => cx.refresh(roster(cx.host()), |view, rows, _| {
                view.rows = Loaded::Ready(rows)
            }),
            None => self.rows = cx.load(roster(cx.host()), |view| &mut view.rows),
        }
        cx.notify();
    }

    fn count(&self) -> String {
        match self.rows.ready() {
            Some(rows) => plural(rows.len(), "account", "accounts"),
            None => String::new(),
        }
    }

    /// The four states of the roster: loading, refused, empty, ready.
    fn body(&self, cx: &mut Context<Self>, theme: &Theme) -> impl IntoElement {
        match &self.rows {
            Loaded::Idle | Loaded::Loading(_) => div()
                .id(ElementId::Name("members-loading".into()))
                .text_size(px(12.))
                .text_color(theme.muted)
                .child("Reading the roster…")
                .into_any_element(),
            Loaded::Failed(refusal) => {
                let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
                div()
                    .id(ElementId::Name("members-refused".into()))
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
                            .id(ElementId::Name("members-retry".into()))
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
            Loaded::Ready(rows) if rows.is_empty() => empty_state(
                "members-empty",
                "No accounts",
                "The identity program of this network holds no accounts yet.",
                theme,
            )
            .into_any_element(),
            Loaded::Ready(rows) => {
                let shown: Vec<&Row> = rows.iter().filter(|row| self.matches(row)).collect();
                if shown.is_empty() {
                    return empty_state(
                        "members-no-match",
                        "Nothing matches",
                        format!("No account reads like “{}”.", self.filter.trim()),
                        theme,
                    )
                    .into_any_element();
                }
                div()
                    .id(ElementId::Name("members-list".into()))
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(shown.into_iter().map(|row| MemberRow::new(row, theme)))
                    .into_any_element()
            }
        }
    }

    fn matches(&self, row: &Row) -> bool {
        let needle = self.filter.trim().to_lowercase();
        needle.is_empty()
            || row.name.to_lowercase().contains(&needle)
            || row.number.to_string().contains(&needle)
    }
}

#[derive(IntoElement)]
struct MemberRow {
    row: Row,
    theme: Theme,
}

impl MemberRow {
    fn new(row: &Row, theme: &Theme) -> Self {
        Self {
            row: row.clone(),
            theme: theme.clone(),
        }
    }
}

impl RenderOnce for MemberRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let row = self.row;
        let theme = self.theme;
        let mut element = div()
            .id(ElementId::named_usize("members-row", row.number as usize))
            .flex()
            .items_center()
            .gap_2()
            .min_h(px(26.))
            .px_2()
            .child(
                div()
                    .w(px(56.))
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child(format!("#{}", row.number)),
            )
            .child(div().flex_1().truncate().child(row.name))
            .child(Badge::new(row.control, theme.muted, theme.surface_raised))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child(plural(row.keys, "key", "keys")),
            );
        if let Some(standing) = row.standing {
            element = element.child(Badge::new(standing, theme.success, theme.success_soft));
        }
        element
    }
}

#[derive(IntoElement)]
struct Badge {
    label: String,
    foreground: ducktape_view_guest::Hsla,
    background: ducktape_view_guest::Hsla,
}

impl Badge {
    fn new(
        label: impl Into<String>,
        foreground: ducktape_view_guest::Hsla,
        background: ducktape_view_guest::Hsla,
    ) -> Self {
        Self {
            label: label.into(),
            foreground,
            background,
        }
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(px(11.))
            .child(self.label)
    }
}

#[derive(IntoElement)]
struct EmptyState {
    id: ElementId,
    title: String,
    detail: String,
    muted: ducktape_view_guest::Hsla,
}

fn empty_state(id: &str, title: &str, detail: impl Into<String>, theme: &Theme) -> EmptyState {
    EmptyState {
        id: ElementId::Name(id.into()),
        title: title.to_owned(),
        detail: detail.into(),
        muted: theme.muted,
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap_1()
            .p_6()
            .max_w(px(420.))
            .child(div().text_size(px(13.)).child(self.title))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(self.muted)
                    .child(self.detail),
            )
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The roster, with each account's valset standing joined on the keys it
/// holds.
///
/// `identity::Reply::Accounts` carries no cursor back, so a view cannot ask
/// for the next page: `Page::all()` is the only honest ask, and how much
/// comes back is the program's call.
async fn roster(host: Host) -> Result<Vec<Row>, Refusal> {
    let accounts = match host
        .ask::<QueryBytes<Identity>>(identity::Query::List { page: Page::all() })
        .await?
    {
        identity::Reply::Accounts(accounts) => accounts,
        other => return Err(unexpected(identity::PROGRAM, &other)),
    };
    let members = match host
        .ask::<QueryBytes<Valset>>(valset::Query::Memberships)
        .await?
    {
        valset::Reply::Memberships(members) => members,
        other => return Err(unexpected(valset::PROGRAM, &other)),
    };
    Ok(accounts
        .iter()
        .map(|account| row(account, &members))
        .collect())
}

fn row(account: &identity::Account, members: &[valset::Membership]) -> Row {
    Row {
        number: account.number,
        name: account.name.clone(),
        control: match &account.control {
            identity::Control::Keys(_) => "Person",
            identity::Control::Program { .. } => "Program",
            identity::Control::Revoked { .. } => "Revoked",
        }
        .into(),
        keys: account.keys().len(),
        standing: members
            .iter()
            .find(|member| account.holds(&member.key))
            .map(|member| {
                match member.standing {
                    valset::Standing::Validator => "Validator",
                    valset::Standing::Resident => "Resident",
                }
                .into()
            }),
    }
}

fn unexpected(program: &str, reply: &impl std::fmt::Debug) -> Refusal {
    malformed(format!("{program} answered {reply:?}"))
}

export_view!(
    Members,
    "Members",
    "Every account of this network, with the standing of the keys it holds.",
    ["rpc"]
);

#[cfg(test)]
mod tests;
