//! Members: every account the `identity` program holds — its number, its
//! name, what it is (a person, an agent and who manages it, a module) and
//! how many keys it carries — with the standing `valset` gives the keys it
//! holds, where it holds one.
//!
//! The contracts are borsh and this view's state is a serde snapshot, so a
//! reply is folded to [`Row`]s as it lands: nothing the programs speak is
//! kept across a snapshot, only what the screen shows.
use ducktape_view_guest::design;
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Error, malformed};
use ducktape_view_guest::methods::Changes;
use ducktape_view_guest::methods::Query;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{
    App, ClickEvent, Context, ElementId, Host, Input, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, StatefulInteractiveElement, Styled, Task, Theme, View,
    Window, div, px,
};
use futures::StreamExt;
use module_registry::PageRequest;
use serde::{Deserialize, Serialize};

use identity::view::Identity;

use valset::view::Valset;

#[derive(Serialize, Deserialize, Default)]
pub struct Members {
    rows: Loadable<Vec<Row>>,
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
    /// what the account is; labelled as it is drawn ([`identity::view::kind`])
    #[serde(with = "identity::view::borsh_bytes")]
    kind: identity::Kind,
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
        let mut stream = cx.host().subscribe::<Changes<Identity>>(());
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
            .id("members")
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
                    .id("members-head")
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("members-title")
                            .flex_1()
                            .text_size(design::text::TITLE)
                            .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                            .role(ducktape_view_guest::Role::Heading)
                            .aria_level(1)
                            .child("Members"),
                    )
                    .child(
                        div()
                            .text_size(design::text::CAPTION)
                            .text_color(theme.muted)
                            .child(self.count()),
                    ),
            )
            .child(
                Input::new("members-filter")
                    .h(px(28.))
                    .w_full()
                    .px_2()
                    .py_1()
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
                view.rows = Loadable::Ready(rows)
            }),
            None => self.rows = cx.load(roster(cx.host()), |view| &mut view.rows),
        }
        cx.notify();
    }

    fn count(&self) -> String {
        match self.rows.ready() {
            Some(rows) => design::plural(rows.len() as u64, "account", "accounts"),
            None => String::new(),
        }
    }

    /// The four states of the roster: loading, refused, empty, ready.
    fn body(&self, cx: &mut Context<Self>, theme: &Theme) -> impl IntoElement {
        match &self.rows {
            Loadable::Idle | Loadable::Loading(_) => div()
                .id("members-loading")
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("Reading the roster…")
                .into_any_element(),
            Loadable::Failed(refusal) => {
                let retry = cx.listener(|view, _: &ClickEvent, _, cx| view.read(cx));
                design::refused("members", refusal.message.clone(), theme, retry).into_any_element()
            }
            Loadable::Ready(rows) if rows.is_empty() => design::empty_state(
                "members-empty",
                "No accounts",
                "The identity program of this network holds no accounts yet.",
                theme,
            )
            .into_any_element(),
            Loadable::Ready(rows) => {
                let shown: Vec<&Row> = rows.iter().filter(|row| self.matches(row)).collect();
                if shown.is_empty() {
                    return design::empty_state(
                        "members-no-match",
                        "Nothing matches",
                        format!("No account reads like “{}”.", self.filter.trim()),
                        theme,
                    )
                    .into_any_element();
                }
                div()
                    .id("members-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(
                        shown
                            .into_iter()
                            .map(|row| MemberRow::new(row, rows, theme)),
                    )
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
    /// what the account is, its manager named from `rows`
    kind: String,
    theme: Theme,
}

impl MemberRow {
    fn new(row: &Row, rows: &[Row], theme: &Theme) -> Self {
        let name_of = |manager| {
            rows.iter()
                .find(|other| other.number == manager)
                .map(|other| other.name.clone())
        };
        Self {
            row: row.clone(),
            kind: identity::view::kind(&row.kind, name_of),
            theme: *theme,
        }
    }
}

impl RenderOnce for MemberRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (row, kind, theme) = (self.row, self.kind, self.theme);
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
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(format!("#{}", row.number)),
            )
            .child(div().flex_1().truncate().child(row.name))
            .child(Badge::new(kind, theme.muted, theme.surface_raised))
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(design::plural(row.keys as u64, "key", "keys")),
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
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(design::text::CAPTION)
            .child(self.label)
    }
}

/// The roster, with each account's valset standing joined on the keys it
/// holds.
///
/// Both programs answer in pages; the roster follows every `next` cursor to
/// the end, since the screen shows the whole network.
async fn roster(host: Host) -> Result<Vec<Row>, Error> {
    let mut accounts = Vec::new();
    let mut after = None;
    loop {
        let page = PageRequest { after, limit: None };
        let reply = match host
            .ask::<Query<Identity>>(identity::Query::List { page })
            .await?
        {
            identity::Reply::Accounts(reply) => reply,
            other => return Err(unexpected(identity::MODULE, &other)),
        };
        accounts.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    let mut members = Vec::new();
    let mut after = None;
    loop {
        let page = PageRequest { after, limit: None };
        let reply = match host
            .ask::<Query<Valset>>(valset::Query::Memberships { page })
            .await?
        {
            valset::Reply::Memberships(reply) => reply,
            other => return Err(unexpected(valset::MODULE, &other)),
        };
        members.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    Ok(accounts
        .iter()
        .map(|account| row(account, &members))
        .collect())
}

fn row(account: &identity::Account, members: &[valset::Membership]) -> Row {
    Row {
        number: account.number,
        name: account.card.name.clone(),
        kind: account.kind(),
        keys: account.keys().len(),
        standing: members
            .iter()
            .find(|member| account.holds(&member.key))
            .map(|member| {
                match member.role {
                    valset::Role::Validator => "Validator",
                    valset::Role::Resident => "Resident",
                }
                .into()
            }),
    }
}

fn unexpected(program: &str, reply: &impl std::fmt::Debug) -> Error {
    malformed(format!("{program} answered {reply:?}"))
}

export_view!(
    Members,
    "Members",
    "Every account of this network, with the standing of the keys it holds.",
    ["module", "host"]
);

#[cfg(test)]
mod tests;
