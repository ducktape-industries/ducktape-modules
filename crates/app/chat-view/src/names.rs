//! What the network calls a party: the roster chat relays from identity,
//! folded into the labels this view draws. Names are display text, not
//! identity: "the same person" is the account number.
use std::collections::{BTreeMap, BTreeSet};

use chat::{AccountRow, Party, hex};
use ducktape_view_guest::design::short_hex;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameDirectory {
    /// the account behind each key, by key hex
    accounts: BTreeMap<String, u64>,
    names: BTreeMap<u64, String>,
    programs: BTreeSet<u64>,
}

impl NameDirectory {
    pub const fn empty() -> Self {
        Self {
            accounts: BTreeMap::new(),
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
            for key in account.keys {
                names.accounts.insert(key, account.number);
            }
            names.names.insert(account.number, account.name);
        }
        names
    }

    pub fn account_of(&self, key_hex: &str) -> Option<u64> {
        self.accounts.get(key_hex).copied()
    }

    pub fn is_program(&self, account: u64) -> bool {
        self.programs.contains(&account)
    }

    /// The name the roster gives a party: an account's own, or the name of
    /// the account holding a key.
    fn name(&self, party: &Party) -> Option<&str> {
        let account = match party {
            Party::Account(number) => *number,
            Party::Key(key) => self.account_of(&hex(key))?,
            Party::Module(_) | Party::System => return None,
        };
        self.names.get(&account).map(String::as_str)
    }

    /// A message's author line: the name, else what the party is.
    pub fn author(&self, party: &Party) -> String {
        match (self.name(party), party) {
            (Some(name), _) => name.to_owned(),
            (None, Party::Key(key)) => format!("user {}", short_hex(&hex(key))),
            (None, _) => unnamed(party),
        }
    }

    /// A member row, a huddle seat or a dm peer: the name, else the key.
    pub fn member(&self, party: &Party) -> String {
        match (self.name(party), party) {
            (Some(name), _) => name.to_owned(),
            (None, Party::Key(key)) => short_hex(&hex(key)),
            (None, _) => unnamed(party),
        }
    }

    /// How a mention reads: `@name`.
    pub fn mention(&self, party: &Party) -> String {
        match party {
            Party::Account(account) => self
                .name(party)
                .filter(|name| !name.is_empty())
                .map_or_else(|| format!("@account-{account}"), |name| format!("@{name}")),
            Party::Key(_) => format!("@{}", self.member(party)),
            Party::Module(module) => format!("@{module}"),
            Party::System => "@system".into(),
        }
    }

    /// A person's key or account is human; a program account (an agent's)
    /// and every module or system author is software.
    pub fn is_agent(&self, party: &Party) -> bool {
        match party {
            Party::Key(_) => false,
            Party::Account(number) => self.is_program(*number),
            Party::Module(_) | Party::System => true,
        }
    }

    /// Autocomplete candidates: every named account, then the room's
    /// members (a key only while it holds no account), labelled without
    /// the `@`.
    pub fn mention_choices(&self, members: &[Party]) -> Vec<MentionChoice> {
        let accounts = self.names.keys().map(|number| Party::Account(*number));
        let members = members.iter().filter(|party| match party {
            Party::Account(_) => true,
            Party::Key(key) => self.account_of(&hex(key)).is_none(),
            Party::Module(_) | Party::System => false,
        });
        let mut choices: Vec<MentionChoice> = Vec::new();
        for party in accounts.chain(members.cloned()) {
            if !choices.iter().any(|choice| choice.party == party) {
                choices.push(MentionChoice {
                    label: self.mention(&party)[1..].to_string(),
                    party,
                });
            }
        }
        choices.sort_by_key(|choice| choice.label.to_lowercase());
        choices
    }
}

/// A party no roster names.
fn unnamed(party: &Party) -> String {
    match party {
        Party::Account(number) => format!("account {number}"),
        Party::Key(key) => short_hex(&hex(key)),
        Party::Module(module) => module.clone(),
        Party::System => "system".into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionChoice {
    pub label: String,
    pub party: Party,
}

/// The canonical token the composer inserts: `<@account>` or `<@key:hex>`.
pub fn mention_token(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("<@{account}>"),
        Party::Key(key) => format!("<@key:{}>", hex(key)),
        Party::Module(_) | Party::System => String::new(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = chat::dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}
