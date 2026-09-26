//! What a view needs of chat: the marker it names this module by in
//! `module.query`/`op.submit`, and the roster folded into what a principal is
//! called. Names are display text, not identity: "the same person" is the
//! account number.
use std::collections::BTreeMap;

use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Error, pages, wrong_reply};
use ducktape_view_guest::methods::{Module, Query as Ask};

use crate::{Kind, MsgRow, PageRequest, Principal, Profile, Query, Reply, Standing};

pub struct Chat;
impl Module for Chat {
    const NAME: &'static str = crate::MODULE;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}

/// The identity role as a view follows it. A view's targets are fixed in
/// its manifest, so it names the program networks bind to the role; the
/// module itself asks the binding (`Env.roles`). A view sends it nothing.
pub struct Identity;
impl Module for Identity {
    const NAME: &'static str = "identity";
    type Op = ();
    type Query = abi::role::identity::Query;
    type Reply = abi::role::identity::Reply;
}

/// How many roster pages one read follows: 64 pages of 256 accounts.
const ROSTER_PAGES: usize = 64;

/// Every account's profile, every page of it, folded into [`Names`].
pub async fn roster(host: Host) -> Result<Names, Error> {
    let (rows, next) = pages(None, ROSTER_PAGES, |after| {
        let ask = host.ask::<Ask<Chat>>(Query::Accounts {
            page: PageRequest {
                after,
                limit: Some(PageRequest::MAX_LIMIT),
            },
        });
        async move {
            match ask.await? {
                Reply::Accounts(page) => Ok((page.items, page.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    let mut names = Names::from_roster(rows);
    names.more = next.is_some();
    Ok(names)
}

/// A channel's roots as `viewer` sees them, oldest first: up to `max_pages`
/// pages of `per_page` below the cursor `below` (or the newest), and whether
/// older ones remain.
pub async fn roots(
    host: Host,
    channel_id: String,
    viewer: Vec<Principal>,
    below: Option<Vec<u8>>,
    max_pages: usize,
    per_page: u64,
) -> Result<(Vec<MsgRow>, bool), Error> {
    let (mut rows, next) = pages(below, max_pages, |after| {
        let ask = host.ask::<Ask<Chat>>(Query::Roots {
            channel_id: channel_id.clone(),
            viewer: viewer.clone(),
            page: PageRequest {
                after,
                limit: Some(per_page),
            },
        });
        async move {
            match ask.await? {
                Reply::Roots(page) => Ok((page.items, page.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    rows.sort_by_key(|row| row.seq);
    Ok((rows, next.is_some()))
}

/// The roster as a view reads it: each account's profile, by number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    profiles: BTreeMap<u64, Profile>,
    /// the roster read stopped at its page budget: more accounts exist
    more: bool,
}

impl Names {
    pub const fn empty() -> Self {
        Self {
            profiles: BTreeMap::new(),
            more: false,
        }
    }

    pub fn from_roster(roster: impl IntoIterator<Item = Profile>) -> Self {
        let mut names = Self::empty();
        for profile in roster {
            names.profiles.insert(profile.number, profile);
        }
        names
    }

    /// Whether the roster goes on past what was read: an account beyond it
    /// reads as its number.
    pub fn more(&self) -> bool {
        self.more
    }

    /// The accounts a person picks, as a reviewer or a mention: people and
    /// agents that act, ascending. No module's account, none suspended or
    /// revoked.
    pub fn people(&self) -> impl Iterator<Item = u64> + '_ {
        self.profiles
            .values()
            .filter(|profile| picks(&profile.kind))
            .map(|profile| profile.number)
    }

    /// Whether a person picks `principal` ([`Names::people`]). An account
    /// past the roster's read is picked by its number.
    pub fn pickable(&self, principal: &Principal) -> bool {
        match self.kind(principal) {
            Some(kind) => picks(kind),
            None => principal.account().is_some(),
        }
    }

    fn profile(&self, principal: &Principal) -> Option<&Profile> {
        self.profiles.get(&principal.account()?)
    }

    /// What the roster says a principal is; `None` past the roster.
    pub fn kind(&self, principal: &Principal) -> Option<&Kind> {
        self.profile(principal).map(|profile| &profile.kind)
    }

    /// The name the roster gives a principal: an account's own.
    pub fn name(&self, principal: &Principal) -> Option<&str> {
        self.profile(principal).map(|profile| profile.name.as_str())
    }

    /// A message's author line: the name, else what the principal is.
    pub fn author(&self, principal: &Principal) -> String {
        self.member(principal)
    }

    /// A member row or a dm peer: the name, else what the
    /// principal is; noted while the account does not act ("(suspended)").
    pub fn member(&self, principal: &Principal) -> String {
        let name = match self.name(principal) {
            Some(name) => name.to_owned(),
            None => unnamed(principal),
        };
        match self.kind(principal).and_then(Kind::note) {
            Some(note) => format!("{name} ({note})"),
            None => name,
        }
    }

    /// How a mention reads: `@name`.
    pub fn mention(&self, principal: &Principal) -> String {
        match principal {
            Principal::Account(account) => self
                .name(principal)
                .filter(|name| !name.is_empty())
                .map_or_else(|| format!("@account-{account}"), |name| format!("@{name}")),
            Principal::Root => "@system".into(),
        }
    }

    /// The module an account is the account of.
    pub fn module(&self, principal: &Principal) -> Option<&str> {
        match self.kind(principal)? {
            Kind::Module(module) => Some(module),
            Kind::Person | Kind::Managed { .. } => None,
        }
    }

    /// What an account is, beside its name ([`Kind::badge`]): an agent and
    /// who manages it ("Agent · managed by Dev"), or the module it is
    /// ("Module · forge"). None for a person.
    pub fn badge(&self, principal: &Principal) -> Option<String> {
        self.kind(principal)?
            .badge(|manager| self.member(&Principal::Account(manager)))
    }
}

/// A person picks a person or an agent that acts.
fn picks(kind: &Kind) -> bool {
    match kind {
        Kind::Person => true,
        Kind::Managed { standing, .. } => *standing == Standing::Active,
        Kind::Module(_) => false,
    }
}

/// A principal no roster names: its account number, or the system.
pub fn unnamed(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "system".into(),
    }
}
