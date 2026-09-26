//! What a view needs of chat: the marker it names this module by in
//! `module.query`/`op.submit`, and the roster folded into what a principal is
//! called. Names are display text, not identity: "the same person" is the
//! account number.
use std::collections::BTreeMap;

use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Error, pages, wrong_reply};
use ducktape_view_guest::methods::{Module, Query as Ask};

use crate::{Category, PageRequest, Principal, Profile, Query, Reply};

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

    /// Every named account, ascending.
    pub fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.profiles.keys().copied()
    }

    fn profile(&self, principal: &Principal) -> Option<&Profile> {
        self.profiles.get(&principal.account()?)
    }

    /// The name the roster gives a principal: an account's own.
    pub fn name(&self, principal: &Principal) -> Option<&str> {
        self.profile(principal).map(|profile| profile.name.as_str())
    }

    /// A message's author line: the name, else what the principal is.
    pub fn author(&self, principal: &Principal) -> String {
        self.member(principal)
    }

    /// A member row, a huddle seat or a dm peer: the name, else what the
    /// principal is.
    pub fn member(&self, principal: &Principal) -> String {
        match self.name(principal) {
            Some(name) => name.to_owned(),
            None => unnamed(principal),
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

    /// An account its manager declares an agent.
    pub fn is_agent(&self, principal: &Principal) -> bool {
        self.profile(principal)
            .is_some_and(|profile| profile.category == Some(Category::Agent))
    }

    /// The module an account is the account of.
    pub fn module(&self, principal: &Principal) -> Option<&str> {
        self.profile(principal)?.module.as_deref()
    }

    /// What an account is, beside its name: an agent and who manages it
    /// ("Agent · managed by Dev"), or the module it is ("Module · forge").
    /// None for a person.
    pub fn badge(&self, principal: &Principal) -> Option<String> {
        let profile = self.profile(principal)?;
        if let Some(module) = &profile.module {
            return Some(format!("Module · {module}"));
        }
        let manager = Principal::Account(profile.manager?);
        (profile.category == Some(Category::Agent))
            .then(|| format!("Agent · managed by {}", self.member(&manager)))
    }
}

/// A principal no roster names: its account number, or the system.
pub fn unnamed(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Root => "system".into(),
    }
}
