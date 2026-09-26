//! Members: every account the `identity` program holds, as a list beside
//! one account read in full. The list groups people, then agents, then
//! modules; the account chosen from it shows its bio, its devices, the
//! agents it manages and what its keys signed lately.
//!
//! The screen is read-only: suspending, revoking, renaming and keys live in
//! Settings. Nothing here asks a program anything the list does not: one
//! `identity` list gives every account whole, `valset` the standing of the
//! keys, and the recent activity is a scan of the chain's recent window
//! ([`activity`]).
//!
//! The contracts are borsh and this view's state is a serde snapshot, so a
//! reply is folded to [`Row`]s as it lands: nothing the programs speak is
//! kept across a snapshot, only what the screen shows.
mod activity;
mod ui;

use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Error, malformed};
use ducktape_view_guest::methods::{Changes, HostSession, Query};
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Context, Host, IntoElement, Render, Task, View, Window};
use module_registry::PageRequest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use identity::view::Identity;
use valset::view::Valset;

pub use activity::Recent;

#[derive(Serialize, Deserialize, Default)]
pub struct Members {
    rows: Loadable<Vec<Row>>,
    /// what the reader typed into the filter; the rows are never refetched
    /// for it, since the program has no search
    filter: String,
    /// the kind chip that is on; `None` is All. It and the filter narrow the
    /// list together and never touch `selected`.
    only: Option<Group>,
    /// the account the detail shows, kept while the filter hides its row
    selected: Option<u64>,
    /// the reader's own account, from the session
    #[serde(skip)]
    me: Option<u64>,
    /// the chain links are minted on, from the session
    #[serde(skip)]
    chain: String,
    /// what the selected account signed lately; read again on restore
    #[serde(skip)]
    activity: Loadable<Recent>,
    /// each account's last finished scan, so choosing it again reads nothing;
    /// an entry goes when a bump changes that account's keys
    #[serde(skip)]
    scans: BTreeMap<u64, Recent>,
    #[serde(skip)]
    watches: Vec<Task<()>>,
}

/// The list's three groups, in the order they are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Group {
    People,
    Agents,
    Modules,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::People, Group::Agents, Group::Modules];

    fn of(kind: &identity::Kind) -> Group {
        match kind {
            identity::Kind::Person => Group::People,
            identity::Kind::Managed { .. } => Group::Agents,
            identity::Kind::Module(_) => Group::Modules,
        }
    }
}

/// One account as this screen shows it.
#[derive(Clone, Serialize, Deserialize)]
struct Row {
    number: u64,
    name: String,
    bio: Option<String>,
    /// what the account is; labelled as it is drawn ([`identity::view::kind`])
    #[serde(with = "ducktape_view_guest::borsh_bytes")]
    kind: identity::Kind,
    devices: Vec<Device>,
    /// the valset standing of a key this account holds, where it holds one
    standing: Option<String>,
}

impl Row {
    fn group(&self) -> Group {
        Group::of(&self.kind)
    }

    fn manager(&self) -> Option<u64> {
        match self.kind {
            identity::Kind::Managed { manager, .. } => Some(manager),
            _ => None,
        }
    }
}

/// One key an account acts with.
#[derive(Clone, Serialize, Deserialize)]
struct Device {
    label: Option<String>,
    key: Vec<u8>,
    /// when it joined, in milliseconds
    added_at: u64,
}

impl View for Members {
    const PREFERRED_WINDOW_SIZE: &'static str = "960,640";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watches.clear();
        let session = cx.host().subscribe::<HostSession>(());
        self.watches
            .push(cx.for_each(session, |view, session, _, cx| match session {
                Ok(session) => {
                    view.me = session.account;
                    view.chain = session.chain_id;
                }
                Err(refusal) => log(cx, "the session", &refusal),
            }));
        let changes = cx.host().subscribe::<Changes<Identity>>(());
        self.watches
            .push(cx.for_each(changes, |view, bump, _, cx| match bump {
                Ok(_) => view.read(cx),
                Err(refusal) => log(cx, "identity's live heads", &refusal),
            }));
        self.read(cx);
        self.read_activity(cx);
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
fn log(cx: &mut Context<Members>, what: &str, refusal: &Error) {
    cx.host().log(format!("members: {what} refused: {refusal}"));
}

impl Render for Members {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

impl Members {
    /// One read of both programs — the boot, a retry, a restore, a live
    /// bump. Rows already on screen stay there while it runs, so a bump
    /// never blinks the list back to "Loading"; a refused bump is logged
    /// and leaves them.
    fn read(&mut self, cx: &mut Context<Self>) {
        let work = roster(cx.host());
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                let mut rescan = view.activity.is_idle();
                match (result, view.rows.ready()) {
                    (Ok(rows), old) => {
                        let old = old.map_or(&[][..], Vec::as_slice);
                        view.scans
                            .retain(|&number, _| keys(old, number) == keys(&rows, number));
                        rescan |= view
                            .selected
                            .is_some_and(|number| keys(old, number) != keys(&rows, number));
                        view.rows = Loadable::Ready(rows);
                    }
                    (Err(refusal), Some(_)) => log(cx, "a refresh", &refusal),
                    (Err(refusal), None) => view.rows = Loadable::Failed(refusal),
                }
                if rescan {
                    view.read_activity(cx);
                }
                cx.notify();
            });
        });
        match self.rows.ready() {
            Some(_) => task.detach(),
            None => self.rows = Loadable::Loading(task),
        }
        cx.notify();
    }

    /// Shows `number` in the detail and reads what it signed lately.
    pub fn select(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.selected == Some(number) {
            return;
        }
        self.selected = Some(number);
        self.activity = Loadable::Idle;
        self.read_activity(cx);
        cx.notify();
    }

    /// Reads the selected account's recent activity, once its keys are
    /// known, unless an earlier scan of the same keys is kept; an account
    /// with no keys signs nothing and asks nothing.
    fn read_activity(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let number = row.number;
        if let Some(recent) = self.scans.get(&number) {
            self.activity = Loadable::Ready(recent.clone());
            return;
        }
        if row.devices.is_empty() {
            self.activity = Loadable::Idle;
            return;
        }
        let keys = row
            .devices
            .iter()
            .map(|device| device.key.clone())
            .collect();
        let work = activity::recent(cx.host(), keys);
        // held in `activity`, so choosing someone else drops it unfinished
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                if let Ok(recent) = &result {
                    view.scans.insert(number, recent.clone());
                }
                view.activity = Loadable::from(result);
                cx.notify();
            });
        });
        self.activity = Loadable::Loading(task);
    }

    fn selected_row(&self) -> Option<&Row> {
        let number = self.selected?;
        self.rows.ready()?.iter().find(|row| row.number == number)
    }

    /// The rows the filter and the chip let through, grouped and in order.
    fn shown(&self) -> Vec<&Row> {
        let Some(rows) = self.rows.ready() else {
            return Vec::new();
        };
        let needle = self.filter.trim().to_lowercase();
        let mut shown: Vec<&Row> = rows
            .iter()
            .filter(|row| self.only.is_none_or(|only| row.group() == only))
            .filter(|row| {
                needle.is_empty()
                    || row.name.to_lowercase().contains(&needle)
                    || row.number.to_string().contains(&needle)
            })
            .collect();
        shown.sort_by_key(|row| (row.group(), row.number));
        shown
    }

    /// ↑ and ↓ move the selection through the rows shown.
    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let shown: Vec<u64> = self.shown().iter().map(|row| row.number).collect();
        let at = self
            .selected
            .and_then(|number| shown.iter().position(|shown| *shown == number));
        let next = match (at, down) {
            (None, true) => shown.first(),
            (None, false) => shown.last(),
            (Some(at), true) => shown.get(at + 1),
            (Some(at), false) => at.checked_sub(1).and_then(|at| shown.get(at)),
        };
        if let Some(&next) = next {
            self.select(next, cx);
        }
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

/// The keys account `number` holds in `rows`, if it is there.
fn keys(rows: &[Row], number: u64) -> Option<Vec<&[u8]>> {
    let row = rows.iter().find(|row| row.number == number)?;
    Some(row.devices.iter().map(|device| &device.key[..]).collect())
}

fn row(account: &identity::Account, members: &[valset::Membership]) -> Row {
    Row {
        number: account.number,
        name: account.card.name.clone(),
        bio: account.card.bio.clone(),
        kind: account.kind(),
        devices: account
            .keys()
            .iter()
            .map(|key| Device {
                label: key.label.clone(),
                key: key.key.clone(),
                added_at: key.added_at,
            })
            .collect(),
        standing: members
            .iter()
            .find(|member| account.holds(&member.key))
            .map(|member| {
                match member.role {
                    valset::Role::Validator => "validator",
                    valset::Role::Resident => "resident",
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
    "Every account of this network: who it is, what it runs and what it signed lately.",
    ["chain", "module", "host", "link"]
);

#[cfg(test)]
mod tests;
