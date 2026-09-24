//! Explorer: the chain as this node keeps it. Finalized blocks and the
//! transactions they carry come from the node's block archive
//! (`rpc.blocks`, `rpc.block`); who signed them from `identity`; the
//! validators from `valset`; the programs from `module-registry`.
//!
//! What the node does not keep is not shown: there are no receipts, so a
//! transaction is "in block N", never applied or rejected; there is no
//! per-block state root or write set; and nothing indexes an account's
//! history, so an account's activity is what a scan of the recent window
//! finds. The window is the last [`WINDOW`] blocks, read a page at a time
//! and then followed at the head as `rpc.status` moves.
use ducktape_view_guest::doors::{
    Block, BlockGet, BlockPage, BlockRef, Blocks, NodeStatus, Program, Query, Ticks,
};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{Context, Host, IntoElement, Render, Task, View, Window};
use futures::StreamExt;
use module_registry as registry;
use serde::{Deserialize, Serialize};

mod components;
mod decode;
pub(crate) mod ui;

pub use decode::Op;

/// The recent window the explorer reads: activity, search by transaction
/// hash and the transaction list reach this far back and no further.
pub const WINDOW: usize = 1_000;
/// Blocks per `rpc.blocks` page (the node caps a page at 100). Small, so
/// one reply's decoding stays well inside a tick's fuel.
const PAGE: u32 = 20;
/// How often the head is re-read, in milliseconds.
const TICK: i64 = 2_000;

struct Registry;
impl Program for Registry {
    const NAME: &'static str = registry::PROGRAM;
    type Op = ();
    type Query = registry::Query;
    type Reply = registry::Reply;
}

struct Identity;
impl Program for Identity {
    const NAME: &'static str = identity::PROGRAM;
    type Op = ();
    type Query = identity::Query;
    type Reply = identity::Reply;
}

struct Valset;
impl Program for Valset {
    const NAME: &'static str = valset::PROGRAM;
    type Op = ();
    type Query = valset::Query;
    type Reply = valset::Reply;
}

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
    fn tab(&self) -> usize {
        match self {
            Route::Overview => 0,
            Route::Blocks | Route::Block(_) => 1,
            Route::Transactions(_) | Route::Tx(_) => 2,
            Route::Accounts | Route::Account(_) => 3,
            Route::Programs => 4,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRow {
    pub height: u64,
    pub id: [u8; 32],
    pub parent: [u8; 32],
    pub time: u64,
    pub epoch: u64,
    pub proposer: Option<Vec<u8>>,
    pub txs: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxRow {
    pub hash: [u8; 32],
    pub height: u64,
    pub time: u64,
    pub signer: Vec<u8>,
    pub seq: u64,
    pub target: String,
    /// the payload decoded once, as it landed
    pub op: Op,
}

/// A block read into rows, its transactions decoded.
fn rows(block: Block) -> (BlockRow, Vec<TxRow>) {
    let row = BlockRow {
        height: block.height,
        id: block.id,
        parent: block.parent,
        time: block.time,
        epoch: block.epoch,
        proposer: block.proposer,
        txs: block.txs.len(),
    };
    let txs = block
        .txs
        .into_iter()
        .rev()
        .map(|tx| TxRow {
            op: decode::decode(&tx.target, &tx.payload),
            hash: tx.hash,
            height: block.height,
            time: block.time,
            signer: tx.signer,
            seq: tx.seq,
            target: tx.target,
        })
        .collect();
    (row, txs)
}

/// The recent window, newest first.
#[derive(Default, Serialize, Deserialize)]
pub struct Chain {
    pub blocks: Vec<BlockRow>,
    pub txs: Vec<TxRow>,
    /// the archive ends inside the window: nothing older to read
    pub complete: bool,
    pub failed: Option<String>,
}

impl Chain {
    fn top(&self) -> Option<u64> {
        self.blocks.first().map(|block| block.height)
    }

    /// Now, as the chain tells it: the newest block's time.
    pub fn now(&self) -> u64 {
        self.blocks.first().map_or(0, |block| block.time)
    }

    pub fn block(&self, height: u64) -> Option<&BlockRow> {
        let top = self.top()?;
        let index = top.checked_sub(height)? as usize;
        self.blocks
            .get(index)
            .filter(|block| block.height == height)
    }

    /// Folds in one page. `before` is what the page was asked with: `None`
    /// reads down from the head, `Some` below the oldest block held.
    fn land(&mut self, before: Option<u64>, page: Vec<Block>) {
        self.failed = None;
        if page.is_empty() {
            self.complete |= before.is_some();
            return;
        }
        let full = page.len() == PAGE as usize;
        let reached_genesis = page.last().is_some_and(|block| block.height == 0);
        let (blocks, txs): (Vec<_>, Vec<_>) = page.into_iter().map(rows).unzip();
        let txs = txs.into_iter().flatten();
        match (before, self.top()) {
            (Some(_), _) => {
                self.blocks.extend(blocks);
                self.txs.extend(txs);
                self.complete = !full || reached_genesis;
            }
            (None, Some(top)) if blocks.last().is_some_and(|b| b.height <= top + 1) => {
                let fresh = blocks.iter().take_while(|b| b.height > top).count();
                let mut merged: Vec<_> = blocks.into_iter().take(fresh).collect();
                merged.append(&mut self.blocks);
                self.blocks = merged;
                let mut merged: Vec<_> = txs.filter(|tx| tx.height > top).collect();
                merged.append(&mut self.txs);
                self.txs = merged;
            }
            // the first page, or a head so far past the window that it no
            // longer joins: start the window again from here
            (None, _) => {
                self.blocks = blocks;
                self.txs = txs.collect();
                self.complete = !full || reached_genesis;
            }
        }
        self.blocks.truncate(WINDOW);
        let oldest = self.blocks.last().map_or(0, |block| block.height);
        self.txs.retain(|tx| tx.height >= oldest);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub key: Vec<u8>,
    pub scheme: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub number: u64,
    pub name: String,
    pub devices: Vec<Device>,
}

/// One running program: its id and the blob its code lives in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub program: String,
    pub code: String,
    pub params: usize,
}

/// One change the registry will fold in at a later block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub height: u64,
    pub verb: String,
    pub program: String,
    pub code: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Network {
    pub programs: Vec<Entry>,
    pub changes: Vec<Change>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Explorer {
    route: Route,
    search: String,
    /// what the last search found nothing for, in words
    note: Option<String>,
    status: Loaded<NodeStatus>,
    chain: Chain,
    accounts: Loaded<Vec<Account>>,
    validators: Loaded<Vec<Vec<u8>>>,
    network: Loaded<Network>,
    /// a block opened outside the window
    opened: Loaded<Option<(BlockRow, Vec<TxRow>)>>,
    #[serde(skip)]
    pulling: bool,
    #[serde(skip)]
    watches: Vec<Task<()>>,
}

impl View for Explorer {
    const PREFERRED_WINDOW_SIZE: &'static str = "1100,720";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut ticks = cx.host().subscribe::<Ticks>(TICK);
        self.watches.push(cx.spawn(async move |this, cx| {
            while ticks.next().await.is_some() {
                if this.update(cx, |view, cx| view.read_head(cx)).is_err() {
                    break;
                }
            }
        }));
        self.read_all(cx);
    }
}

impl Render for Explorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

impl Explorer {
    /// Everything, again: the boot, a restore, a retry.
    fn read_all(&mut self, cx: &mut Context<Self>) {
        self.read_head(cx);
        self.read_accounts(cx);
        self.read_validators(cx);
        self.read_network(cx);
        self.pull(cx);
    }

    fn read_head(&mut self, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ducktape_view_guest::doors::Status>(());
        if self.status.ready().is_some() {
            cx.refresh(ask, |view, status, cx| {
                view.status = Loaded::Ready(status);
                view.pull(cx);
            });
        } else if !self.status.is_loading() {
            self.status = cx.load(ask, |view| &mut view.status);
        }
        // a failed window is read again with the head, not in a loop
        if self.chain.failed.is_some() {
            self.pull(cx);
        }
        cx.notify();
    }

    fn read_accounts(&mut self, cx: &mut Context<Self>) {
        let work = accounts(cx.host());
        match self.accounts.ready() {
            Some(_) => cx.refresh(work, |view, accounts, _| {
                view.accounts = Loaded::Ready(accounts)
            }),
            None => self.accounts = cx.load(work, |view| &mut view.accounts),
        }
    }

    fn read_validators(&mut self, cx: &mut Context<Self>) {
        let work = validators(cx.host());
        match self.validators.ready() {
            Some(_) => cx.refresh(work, |view, keys, _| view.validators = Loaded::Ready(keys)),
            None => self.validators = cx.load(work, |view| &mut view.validators),
        }
    }

    fn read_network(&mut self, cx: &mut Context<Self>) {
        let work = network(cx.host());
        match self.network.ready() {
            Some(_) => cx.refresh(work, |view, network, _| {
                view.network = Loaded::Ready(network)
            }),
            None => self.network = cx.load(work, |view| &mut view.network),
        }
    }

    /// Reads the next page the window wants, if any: the head when the
    /// status is past it, else older blocks until the window is full.
    fn pull(&mut self, cx: &mut Context<Self>) {
        if self.pulling {
            return;
        }
        let head = self.status.ready().map(|status| status.height);
        let before = match (self.chain.top(), head) {
            (None, _) => None,
            (Some(top), Some(head)) if head > top => None,
            _ if !self.chain.complete && self.chain.blocks.len() < WINDOW => {
                self.chain.blocks.last().map(|block| block.height)
            }
            _ => return,
        };
        self.pulling = true;
        let ask = cx.host().ask::<Blocks>(BlockPage {
            before,
            limit: PAGE,
        });
        cx.spawn(async move |this, cx| {
            let page = ask.await;
            let _ = this.update(cx, |view, cx| {
                view.pulling = false;
                match page {
                    Ok(page) => {
                        let was = (view.chain.top(), view.chain.blocks.len());
                        view.chain.land(before, page);
                        view.follow(was.0, cx);
                        // a page that moved nothing is not asked for again
                        // until the head moves
                        if (view.chain.top(), view.chain.blocks.len()) != was {
                            view.pull(cx);
                        }
                    }
                    Err(refusal) => view.chain.failed = Some(refusal.sentence),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-reads what new transactions at the head may have moved.
    fn follow(&mut self, top: Option<u64>, cx: &mut Context<Self>) {
        let Some(top) = top else {
            return;
        };
        let fresh: Vec<&str> = self
            .chain
            .txs
            .iter()
            .take_while(|tx| tx.height > top)
            .map(|tx| tx.target.as_str())
            .collect();
        let (identity, valset, registry) = (
            fresh.contains(&identity::PROGRAM),
            fresh.contains(&valset::PROGRAM),
            fresh.contains(&registry::PROGRAM),
        );
        if identity {
            self.read_accounts(cx);
        }
        if valset {
            self.read_validators(cx);
        }
        if registry {
            self.read_network(cx);
        }
    }

    pub fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        if let Route::Block(height) = route {
            let held = self.chain.block(height).is_some();
            let opened =
                matches!(self.opened.ready(), Some(Some((row, _))) if row.height == height);
            if !held && !opened {
                let ask = cx.host().ask::<BlockGet>(BlockRef::Height(height));
                self.opened = cx.load(
                    async move { ask.await.map(|block| block.map(rows)) },
                    |view| &mut view.opened,
                );
            }
        }
        self.route = route;
        self.note = None;
        // the field holds only what is being typed: a search that lands, a
        // tab, prev/next and a row all leave it empty
        self.search.clear();
        cx.notify();
    }

    /// A height, a block or transaction hash, an account (`#3` or a name)
    /// or a program name.
    pub fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.trim().to_string();
        self.note = None;
        if query.is_empty() {
            return;
        }
        let digits: String = query.chars().filter(|c| *c != ',').collect();
        if let Ok(height) = digits.parse::<u64>() {
            return self.go(Route::Block(height), cx);
        }
        if let Some(hash) = hash_of(&query) {
            if self.chain.txs.iter().any(|tx| tx.hash == hash) || self.opened_tx(&hash).is_some() {
                return self.go(Route::Tx(hash), cx);
            }
            if let Some(block) = self.chain.blocks.iter().find(|block| block.id == hash) {
                return self.go(Route::Block(block.height), cx);
            }
            let ask = cx.host().ask::<BlockGet>(BlockRef::Id(hash));
            let blocks = self.chain.blocks.len();
            cx.spawn(async move |this, cx| {
                let found = ask.await;
                let _ = this.update(cx, |view, cx| {
                    match found {
                        Ok(Some(block)) => {
                            let (row, txs) = rows(block);
                            let height = row.height;
                            view.opened = Loaded::Ready(Some((row, txs)));
                            view.go(Route::Block(height), cx);
                        }
                        Ok(None) => {
                            view.note = Some(format!(
                                "No block has this hash, and no transaction in the last {} does.",
                                decode::plural(blocks as u64, "block", "blocks")
                            ))
                        }
                        Err(refusal) => view.note = Some(refusal.sentence),
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        let accounts = self.accounts.ready().map(Vec::as_slice).unwrap_or_default();
        let number = query
            .strip_prefix('#')
            .and_then(|n| n.trim().parse::<u64>().ok());
        let lower = query.to_lowercase();
        let account = accounts
            .iter()
            .find(|account| Some(account.number) == number || account.name.to_lowercase() == lower)
            .or_else(|| {
                accounts
                    .iter()
                    .find(|account| account.name.to_lowercase().contains(&lower))
            });
        if let Some(account) = account {
            return self.go(Route::Account(account.number), cx);
        }
        let programs = self.network.ready().map(|n| n.programs.as_slice());
        if let Some(entry) = programs
            .unwrap_or_default()
            .iter()
            .find(|entry| entry.program == lower)
        {
            return self.go(Route::Transactions(Some(entry.program.clone())), cx);
        }
        self.note = Some(format!("Nothing here is called “{query}”."));
        cx.notify();
    }

    fn opened_tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        match self.opened.ready() {
            Some(Some((_, txs))) => txs.iter().find(|tx| &tx.hash == hash),
            _ => None,
        }
    }

    /// A transaction by hash, in the window or the block opened outside it.
    pub fn tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        self.chain
            .txs
            .iter()
            .find(|tx| &tx.hash == hash)
            .or_else(|| self.opened_tx(hash))
    }

    /// A block and its transactions, in the window or opened outside it.
    pub fn block(&self, height: u64) -> Option<(BlockRow, Vec<&TxRow>)> {
        if let Some(row) = self.chain.block(height) {
            let txs = self.chain.txs.iter().filter(|tx| tx.height == height);
            return Some((row.clone(), txs.collect()));
        }
        match self.opened.ready() {
            Some(Some((row, txs))) if row.height == height => {
                Some((row.clone(), txs.iter().collect()))
            }
            _ => None,
        }
    }

    /// The account holding `key`, and which of its devices it is.
    pub fn holder(&self, key: &[u8]) -> Option<(&Account, usize)> {
        self.accounts.ready()?.iter().find_map(|account| {
            let device = account
                .devices
                .iter()
                .position(|device| device.key == key)?;
            Some((account, device))
        })
    }
}

fn hash_of(query: &str) -> Option<[u8; 32]> {
    let query = query.trim_start_matches("0x");
    if query.len() != 64 || !query.is_ascii() {
        return None;
    }
    let mut hash = [0; 32];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&query[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(hash)
}

async fn accounts(host: Host) -> Result<Vec<Account>, Refusal> {
    let mut accounts = Vec::new();
    let mut after = None;
    loop {
        let page = registry::Page { after, limit: None };
        let reply = match host
            .ask::<Query<Identity>>(identity::Query::List { page })
            .await?
        {
            identity::Reply::Accounts(reply) => reply,
            other => return Err(malformed(format!("identity answered List with {other:?}"))),
        };
        accounts.extend(reply.items.into_iter().map(|account| {
            Account {
                number: account.number,
                devices: account
                    .keys()
                    .iter()
                    .map(|key| Device {
                        key: key.key.clone(),
                        scheme: decode::scheme(key.scheme),
                        label: key.label.clone(),
                    })
                    .collect(),
                name: account.name,
            }
        }));
        match reply.next {
            Some(next) => after = Some(next),
            None => return Ok(accounts),
        }
    }
}

async fn validators(host: Host) -> Result<Vec<Vec<u8>>, Refusal> {
    match host.ask::<Query<Valset>>(valset::Query::Validators).await? {
        valset::Reply::Validators(keys) => Ok(keys),
        other => Err(malformed(format!(
            "valset answered Validators with {other:?}"
        ))),
    }
}

/// What the registry runs and what it will run.
///
/// `At(0)` is the folded set: the registry applies the changes due at or
/// before the height asked, and it answers no height of its own, so there is
/// no "as of now" to ask for — the scheduled list is what is still to come.
async fn network(host: Host) -> Result<Network, Refusal> {
    let unexpected = |reply: &dyn std::fmt::Debug| {
        malformed(format!("{} answered {reply:?}", registry::PROGRAM))
    };
    let programs = match host.ask::<Query<Registry>>(registry::Query::At(0)).await? {
        registry::Reply::Programs(programs) => programs,
        other => return Err(unexpected(&other)),
    };
    let mut scheduled = Vec::new();
    let mut after = None;
    loop {
        let page = registry::Page { after, limit: None };
        let reply = match host
            .ask::<Query<Registry>>(registry::Query::Scheduled { page })
            .await?
        {
            registry::Reply::Scheduled(reply) => reply,
            other => return Err(unexpected(&other)),
        };
        scheduled.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    Ok(Network {
        programs: programs
            .iter()
            .map(|entry| Entry {
                program: entry.program.clone(),
                code: abi::hex(entry.code.digest()),
                params: entry.params.len(),
            })
            .collect(),
        changes: scheduled
            .iter()
            .map(|scheduled| Change {
                height: scheduled.height,
                verb: match scheduled.change {
                    registry::Change::Set(_) => "Set",
                    registry::Change::Remove(_) => "Remove",
                    registry::Change::SetView(_) => "Set view",
                    registry::Change::RemoveView(_) => "Remove view",
                }
                .into(),
                program: scheduled.change.program().to_string(),
                code: match &scheduled.change {
                    registry::Change::Set(entry) => Some(abi::hex(entry.code.digest())),
                    registry::Change::SetView(view) => Some(abi::hex(view.view.digest())),
                    registry::Change::Remove(_) | registry::Change::RemoveView(_) => None,
                },
            })
            .collect(),
    })
}

export_view!(
    Explorer,
    "Explorer",
    "The chain as this node keeps it: blocks, transactions, accounts and programs.",
    ["rpc", "host", "clock"]
);

#[cfg(test)]
mod tests;
