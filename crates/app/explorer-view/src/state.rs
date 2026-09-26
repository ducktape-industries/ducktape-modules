//! The state Explorer keeps: where it is, and what it read. Rows are the
//! programs' own types, worded only when drawn (`ui/`).
use ducktape_view_guest::borsh_bytes;
use ducktape_view_guest::methods::NodeStatus;
use ducktape_view_guest::view::Loadable;
use ducktape_view_guest::{Task, design};
use module_registry as registry;
use serde::{Deserialize, Serialize};

use crate::chain::{BlockRow, Chain, TxRow};

/// Where the explorer is: a list under a tab, or one thing opened from it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    #[default]
    Overview,
    Blocks,
    Block(u64),
    /// the recent transactions, of one program when named
    Transactions(Option<String>),
    Tx([u8; 32]),
    Accounts,
    Account(u64),
    Programs,
}

impl Route {
    /// This page's tail after `duck://<chain>/explorer/`, which
    /// [`Route::from_path`] reads back.
    pub fn path(&self) -> String {
        match self {
            Route::Overview => String::new(),
            Route::Blocks => "blocks".into(),
            Route::Block(height) => design::explorer::block_path(*height),
            Route::Transactions(None) => "txs".into(),
            Route::Transactions(Some(program)) => format!("program/{program}"),
            Route::Tx(hash) => design::explorer::tx_path(hash),
            Route::Accounts => "accounts".into(),
            Route::Account(number) => design::explorer::account_path(*number),
            Route::Programs => "programs".into(),
        }
    }

    /// The page a link's tail names, if it names one.
    pub fn from_path(path: &str) -> Option<Route> {
        let parts: Vec<&str> = path.split('/').collect();
        Some(match parts.as_slice() {
            [""] => Route::Overview,
            ["blocks"] => Route::Blocks,
            ["block", height] => Route::Block(ducklink::number(height)?),
            ["txs"] => Route::Transactions(None),
            ["program", name] if !name.is_empty() => Route::Transactions(Some((*name).into())),
            ["tx", hash] => Route::Tx(hash_of(hash)?),
            ["accounts"] => Route::Accounts,
            ["account", number] => Route::Account(ducklink::number(number)?),
            ["programs"] => Route::Programs,
            _ => return None,
        })
    }

    /// The tab this page sits under.
    pub(crate) fn tab(&self) -> usize {
        match self {
            Route::Overview => 0,
            Route::Blocks | Route::Block(_) => 1,
            Route::Transactions(_) | Route::Tx(_) => 2,
            Route::Accounts | Route::Account(_) => 3,
            Route::Programs => 4,
        }
    }
}

/// A 32-byte hash as 64 hex digits, `0x` allowed.
pub(crate) fn hash_of(text: &str) -> Option<[u8; 32]> {
    let text = text.trim_start_matches("0x");
    if text.len() != 64 {
        return None;
    }
    abi::unhex(text)?.try_into().ok()
}

/// Every account identity lists, as identity answers them.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Accounts {
    #[serde(with = "borsh_bytes")]
    pub list: Vec<identity::Account>,
    /// the read stopped at its page budget: more accounts exist
    pub more: bool,
}

/// What the registry runs, lists and will change, as it answers them.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Network {
    #[serde(with = "borsh_bytes")]
    pub programs: Vec<registry::Entry>,
    /// the view-only entries: a name and its view's code, no program behind it
    #[serde(with = "borsh_bytes")]
    pub views: Vec<registry::View>,
    /// what the registry will fold in at later blocks
    #[serde(with = "borsh_bytes")]
    pub changes: Vec<registry::Scheduled>,
    /// the scheduled read stopped at its page budget: more changes exist
    pub more: bool,
}

/// The line under the bar: what a search, a link or a copy came to. Worded
/// when drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Note {
    /// a search that names nothing here
    NotFound(String),
    /// a hash no block has, looked for across a window of `blocks`
    NoSuchHash {
        blocks: u64,
    },
    /// a link whose path names no page
    Unlinked(String),
    Copied,
    /// a refusal, in the host's words
    Refused(String),
}

#[derive(Serialize, Deserialize, Default)]
pub struct Explorer {
    pub(crate) route: Route,
    /// the search field, as typed
    pub(crate) search: String,
    pub(crate) note: Option<Note>,
    /// the session's chain id (`<label>#<salt>`), which links are minted in
    pub(crate) session_chain: String,
    pub(crate) status: Loadable<NodeStatus>,
    pub(crate) chain: Chain,
    pub(crate) accounts: Loadable<Accounts>,
    pub(crate) validators: Loadable<Vec<Vec<u8>>>,
    pub(crate) network: Loadable<Network>,
    /// a block opened outside the window
    pub(crate) opened: Loadable<Option<(BlockRow, Vec<TxRow>)>>,
    /// a `chain.blocks` page is in flight
    #[serde(skip)]
    pub(crate) pulling: bool,
    /// the head is polled on the clock: `chain.heads` was refused or ended
    #[serde(skip)]
    pub(crate) polling: Option<Task<()>>,
    /// What the view follows (`watch.rs`); dropping them unsubscribes.
    #[serde(skip)]
    pub(crate) followers: Vec<Task<()>>,
}

impl Explorer {
    /// Every account identity listed, or none yet.
    pub(crate) fn account_list(&self) -> &[identity::Account] {
        self.accounts
            .ready()
            .map_or(&[][..], |accounts| accounts.list.as_slice())
    }

    pub(crate) fn account(&self, number: u64) -> Option<&identity::Account> {
        self.account_list()
            .iter()
            .find(|account| account.number == number)
    }

    /// The account holding `key`, and which of its keys it is.
    pub(crate) fn holder(&self, key: &[u8]) -> Option<(&identity::Account, usize)> {
        self.account_list().iter().find_map(|account| {
            let at = account.keys().iter().position(|held| held.key == key)?;
            Some((account, at))
        })
    }

    /// Whether the registry runs `program`.
    pub(crate) fn runs(&self, program: &str) -> Option<&registry::Entry> {
        self.network
            .ready()?
            .programs
            .iter()
            .find(|entry| entry.program == program)
    }
}
