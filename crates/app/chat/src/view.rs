//! What a view needs of chat: the marker it names this program by in
//! `program.query`/`op.submit`, and the roster folded into what a principal is
//! called. Names are display text, not identity: "the same person" is the
//! account number.
use std::collections::{BTreeMap, BTreeSet};

use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Refusal, pages, wrong_reply};
use ducktape_view_guest::methods::{Program, Query as Ask};

use crate::{AccountRow, Page, Principal, Query, Reply};

pub struct Chat;
impl Program for Chat {
    const NAME: &'static str = crate::PROGRAM;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}

/// How many roster pages one read follows: 64 pages of 256 accounts.
const ROSTER_PAGES: usize = 64;

/// The identity roster, every page of it, folded into [`Names`].
pub async fn roster(host: Host) -> Result<Names, Refusal> {
    let (rows, next) = pages(None, ROSTER_PAGES, |after| {
        let ask = host.ask::<Ask<Chat>>(Query::Accounts {
            page: Page {
                after,
                limit: Some(Page::MAX_LIMIT),
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

/// The roster as a view reads it: each account's name, and which accounts
/// a program controls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    names: BTreeMap<u64, String>,
    programs: BTreeSet<u64>,
    /// the roster read stopped at its page budget: more accounts exist
    more: bool,
}

impl Names {
    pub const fn empty() -> Self {
        Self {
            names: BTreeMap::new(),
            programs: BTreeSet::new(),
            more: false,
        }
    }

    pub fn from_roster(roster: impl IntoIterator<Item = AccountRow>) -> Self {
        let mut names = Self::empty();
        for account in roster {
            if account.program {
                names.programs.insert(account.number);
            }
            names.names.insert(account.number, account.name);
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
        self.names.keys().copied()
    }

    pub fn is_program(&self, account: u64) -> bool {
        self.programs.contains(&account)
    }

    /// The name the roster gives a principal: an account's own.
    pub fn name(&self, principal: &Principal) -> Option<&str> {
        let account = match principal {
            Principal::Account(number) => *number,
            Principal::Module(_) | Principal::System => return None,
        };
        self.names.get(&account).map(String::as_str)
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
            Principal::Module(module) => format!("@{module}"),
            Principal::System => "@system".into(),
        }
    }

    /// A person's account is human; a program account (an agent's)
    /// and every module or system author is software.
    pub fn is_agent(&self, principal: &Principal) -> bool {
        match principal {
            Principal::Account(number) => self.is_program(*number),
            Principal::Module(_) | Principal::System => true,
        }
    }
}

/// A principal no roster names: its account number, its module.
pub fn unnamed(principal: &Principal) -> String {
    match principal {
        Principal::Account(number) => format!("account {number}"),
        Principal::Module(module) => module.clone(),
        Principal::System => "system".into(),
    }
}
