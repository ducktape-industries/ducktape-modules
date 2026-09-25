//! What a view needs of chat: the marker it names this program by in
//! `program.query`/`op.submit`, and the roster folded into what a party is
//! called. Names are display text, not identity: "the same person" is the
//! account number.
use std::collections::{BTreeMap, BTreeSet};

use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Refusal, pages, wrong_reply};
use ducktape_view_guest::methods::{Program, Query as Ask};

use crate::{AccountRow, Page, Party, Query, Reply};

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
    let (rows, _) = pages(None, ROSTER_PAGES, |after| {
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
    Ok(Names::from_roster(rows))
}

/// The roster as a view reads it: each account's name, and which accounts
/// a program controls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    names: BTreeMap<u64, String>,
    programs: BTreeSet<u64>,
}

impl Names {
    pub const fn empty() -> Self {
        Self {
            names: BTreeMap::new(),
            programs: BTreeSet::new(),
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

    /// Every named account, ascending.
    pub fn numbers(&self) -> impl Iterator<Item = u64> + '_ {
        self.names.keys().copied()
    }

    pub fn is_program(&self, account: u64) -> bool {
        self.programs.contains(&account)
    }

    /// The name the roster gives a party: an account's own.
    pub fn name(&self, party: &Party) -> Option<&str> {
        let account = match party {
            Party::Account(number) => *number,
            Party::Module(_) | Party::System => return None,
        };
        self.names.get(&account).map(String::as_str)
    }

    /// A message's author line: the name, else what the party is.
    pub fn author(&self, party: &Party) -> String {
        self.member(party)
    }

    /// A member row, a huddle seat or a dm peer: the name, else what the
    /// party is.
    pub fn member(&self, party: &Party) -> String {
        match self.name(party) {
            Some(name) => name.to_owned(),
            None => unnamed(party),
        }
    }

    /// How a mention reads: `@name`.
    pub fn mention(&self, party: &Party) -> String {
        match party {
            Party::Account(account) => self
                .name(party)
                .filter(|name| !name.is_empty())
                .map_or_else(|| format!("@account-{account}"), |name| format!("@{name}")),
            Party::Module(module) => format!("@{module}"),
            Party::System => "@system".into(),
        }
    }

    /// A person's account is human; a program account (an agent's)
    /// and every module or system author is software.
    pub fn is_agent(&self, party: &Party) -> bool {
        match party {
            Party::Account(number) => self.is_program(*number),
            Party::Module(_) | Party::System => true,
        }
    }
}

/// A party no roster names: its account number, its module.
pub fn unnamed(party: &Party) -> String {
    match party {
        Party::Account(number) => format!("account {number}"),
        Party::Module(module) => module.clone(),
        Party::System => "system".into(),
    }
}
