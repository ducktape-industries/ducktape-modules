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
use ducktape_view_guest::view::{Cx, Live, Loaded, View, Watching, ask};
use ducktape_view_guest::wire::{Length, Node, kit, kit::Tone};
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
    live: Option<Watching>,
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
    const PREFERRED_WINDOW_SIZE: &'static str = "720x640";

    fn boot(cx: &mut Cx<Self>) -> Self {
        let mut view = Self::default();
        view.restored(cx);
        view
    }

    fn restored(&mut self, cx: &mut Cx<Self>) {
        self.live = Some(cx.watch::<Live>(identity::PROGRAM.into(), |view, _, cx| view.read(cx)));
        self.read(cx);
    }

    fn render(&mut self, cx: &mut Cx<Self>) -> Node {
        let key = "members";
        let typed = cx.on_value(|view, text: String, _| view.filter = text);
        let head = kit::centered_row(
            format!("{key}/head"),
            [
                kit::fill_width(kit::title(format!("{key}/title"), "Members")),
                kit::caption(format!("{key}/count"), self.count()),
            ],
        );
        let filter = kit::text_field(
            format!("{key}/filter"),
            "Filter by name or number",
            &self.filter,
            typed,
            None,
            false,
        );
        kit::page(key, [head, filter, self.body(key, cx)])
    }
}

impl Members {
    /// One read of both programs — the boot, a retry, a restore, a live
    /// bump. Rows already on screen stay there while it runs, so a bump
    /// never blinks the list back to "Loading"; an empty or refused slot
    /// says it is loading, because it has nothing else to say.
    fn read(&mut self, cx: &mut Cx<Self>) {
        match self.rows.ready() {
            Some(_) => cx.refresh(roster(), |view, rows, _| view.rows = Loaded::Ready(rows)),
            None => self.rows = cx.load(roster(), |view| &mut view.rows),
        }
    }

    fn count(&self) -> String {
        match self.rows.ready() {
            Some(rows) => kit::plural(rows.len() as u64, "account", "accounts"),
            None => String::new(),
        }
    }

    /// The four states of the roster: loading, refused, empty, ready.
    fn body(&self, key: &str, cx: &mut Cx<Self>) -> Node {
        match &self.rows {
            Loaded::Idle | Loaded::Loading(_) => {
                kit::secondary(format!("{key}/loading"), "Reading the roster…")
            }
            Loaded::Failed(refusal) => {
                let retry = cx.on(|view, cx| view.read(cx));
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
            Loaded::Ready(rows) if rows.is_empty() => kit::empty_state(
                format!("{key}/empty"),
                "No accounts",
                "The identity program of this network holds no accounts yet.",
            ),
            Loaded::Ready(rows) => {
                let shown: Vec<&Row> = rows.iter().filter(|row| self.matches(row)).collect();
                if shown.is_empty() {
                    return kit::empty_state(
                        format!("{key}/no-match"),
                        "Nothing matches",
                        format!("No account reads like “{}”.", self.filter.trim()),
                    );
                }
                kit::scroll(
                    format!("{key}/list"),
                    kit::column(
                        format!("{key}/rows"),
                        shown
                            .into_iter()
                            .map(|row| render_row(&format!("{key}/row/{}", row.number), row)),
                    ),
                )
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

fn render_row(key: &str, row: &Row) -> Node {
    let mut cells = vec![
        kit::width(
            kit::secondary(format!("{key}/number"), format!("#{}", row.number)),
            Length::Fixed(56.),
        ),
        kit::fill_width(kit::truncated(format!("{key}/name"), &row.name, 40)),
        kit::badge(format!("{key}/control"), &row.control, Tone::Neutral),
        kit::caption(
            format!("{key}/keys"),
            kit::plural(row.keys as u64, "key", "keys"),
        ),
    ];
    if let Some(standing) = &row.standing {
        cells.push(kit::badge(
            format!("{key}/standing"),
            standing,
            Tone::Success,
        ));
    }
    kit::centered_row(key, cells)
}

/// The roster, with each account's valset standing joined on the keys it
/// holds.
///
/// `identity::Reply::Accounts` carries no cursor back, so a view cannot ask
/// for the next page: `Page::all()` is the only honest ask, and how much
/// comes back is the program's call.
async fn roster() -> Result<Vec<Row>, Refusal> {
    let accounts =
        match ask::<QueryBytes<Identity>>(identity::Query::List { page: Page::all() }).await? {
            identity::Reply::Accounts(accounts) => accounts,
            other => return Err(unexpected(identity::PROGRAM, &other)),
        };
    let members = match ask::<QueryBytes<Valset>>(valset::Query::Memberships).await? {
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
