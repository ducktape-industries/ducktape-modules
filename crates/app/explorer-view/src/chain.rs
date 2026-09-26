//! The recent window: the last [`WINDOW`] finalized blocks and the
//! transactions they carry, folded in a page at a time.
use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::{Block, Description, Outcome, Receipt};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::{Cell, OnceCell};

use crate::{PAGE, WINDOW, decode};

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TxRow {
    pub hash: [u8; 32],
    pub height: u64,
    pub time: u64,
    pub signer: Vec<u8>,
    pub seq: u64,
    pub target: String,
    /// the payload as it landed, described only once a row is on screen
    pub payload: Vec<u8>,
    /// what the host answered for it (`Explorer::describe`), or its bytes
    pub(crate) op: OnceCell<Description>,
    pub(crate) asked: Cell<bool>,
    /// how its run ended, where the node kept its receipt
    pub run: Option<Run>,
}

/// A receipt as the explorer shows it: whose run, its refusal where it was
/// rejected, and the runs its messages caused. Outputs and events are left
/// behind: nothing here shows them, and a snapshot need not carry them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub program: String,
    pub refusal: Option<Error>,
    pub nested: Vec<Run>,
}

impl From<Receipt> for Run {
    fn from(receipt: Receipt) -> Run {
        let refusal = match receipt.outcome {
            Outcome::Applied { .. } => None,
            Outcome::Rejected(refusal) => Some(refusal),
        };
        Run {
            program: receipt.program,
            refusal,
            nested: receipt.nested.into_iter().map(Run::from).collect(),
        }
    }
}

impl TxRow {
    /// The op as its program described it, once the host has answered.
    pub fn op(&self) -> Option<&Description> {
        self.op.get()
    }
}

/// The longest payload a snapshot keeps for a row not yet described; a
/// longer one is kept as its bytes' description ([`decode::bytes`]).
const KEPT_PAYLOAD: usize = 4 << 10;

/// A [`TxRow`] as the snapshot holds it: the op where it was described,
/// else a short payload, never a 1 MB push.
#[derive(Serialize, Deserialize)]
struct StoredTx<'a> {
    hash: [u8; 32],
    height: u64,
    time: u64,
    signer: Cow<'a, [u8]>,
    seq: u64,
    target: Cow<'a, str>,
    op: Option<Description>,
    payload: Cow<'a, [u8]>,
    run: Cow<'a, Option<Run>>,
}

impl Serialize for TxRow {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (op, payload) = match self.op.get() {
            Some(op) => (Some(op.clone()), &[][..]),
            None if self.payload.len() <= KEPT_PAYLOAD => (None, &self.payload[..]),
            None => (Some(decode::bytes(&self.target, &self.payload)), &[][..]),
        };
        StoredTx {
            hash: self.hash,
            height: self.height,
            time: self.time,
            signer: Cow::Borrowed(&self.signer),
            seq: self.seq,
            target: Cow::Borrowed(&self.target),
            op,
            payload: Cow::Borrowed(payload),
            run: Cow::Borrowed(&self.run),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TxRow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let stored = StoredTx::deserialize(d)?;
        Ok(TxRow {
            hash: stored.hash,
            height: stored.height,
            time: stored.time,
            signer: stored.signer.into_owned(),
            seq: stored.seq,
            target: stored.target.into_owned(),
            payload: stored.payload.into_owned(),
            op: stored.op.map(Into::into).unwrap_or_default(),
            asked: Default::default(),
            run: stored.run.into_owned(),
        })
    }
}

/// A block read into rows, its transactions newest first.
pub(crate) fn rows(block: Block) -> (BlockRow, Vec<TxRow>) {
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
            hash: tx.hash,
            height: block.height,
            time: block.time,
            signer: tx.signer,
            seq: tx.seq,
            target: tx.target,
            payload: tx.payload,
            op: Default::default(),
            asked: Default::default(),
            run: tx.receipt.map(Run::from),
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
    /// the last page was refused, in the host's words
    pub failed: Option<String>,
}

impl Chain {
    pub(crate) fn top(&self) -> Option<u64> {
        self.blocks.first().map(|block| block.height)
    }

    /// Now, as the chain tells it: the newest block's time.
    pub fn now(&self) -> u64 {
        self.blocks.first().map_or(0, |block| block.time)
    }

    pub fn block(&self, height: u64) -> Option<&BlockRow> {
        let index = self.top()?.checked_sub(height)? as usize;
        self.blocks
            .get(index)
            .filter(|block| block.height == height)
    }

    /// Folds in one page. `before` is what the page was asked with: `None`
    /// reads down from the head, `Some` below the oldest block held.
    pub(crate) fn land(&mut self, before: Option<u64>, page: Vec<Block>) {
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
