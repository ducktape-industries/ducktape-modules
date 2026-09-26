//! The loaders: the head and the window it pulls (`chain.status`,
//! `chain.blocks`), the lists the system programs answer, and what an op
//! says it does (`module.describe`). Each first read is `cx.load`; a
//! re-read is `cx.refresh`, which keeps what is on screen.
use ducktape_view_guest::Context;
use ducktape_view_guest::methods::{
    BlockPage, ChainBlocks, ChainStatus, ClockTicks, Head, ModuleDescribe,
};
use ducktape_view_guest::view::Loadable;

use crate::chain::{BlockRow, TxRow};
use crate::watch::log;
use crate::{Explorer, PAGE, TICK, WINDOW, decode, queries};

impl Explorer {
    /// Everything, again: the boot, a restore, a retry.
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        self.read_head(cx);
        self.read_accounts(cx);
        self.read_validators(cx);
        self.read_network(cx);
        self.pull(cx);
    }

    /// A head `chain.heads` pushed: the status moves to it without a read,
    /// and the window follows.
    pub(crate) fn at_head(&mut self, head: Head, cx: &mut Context<Self>) {
        match &mut self.status {
            Loadable::Ready(status) => {
                if head.height > status.height {
                    status.height = head.height;
                    status.tip = head.id;
                }
                self.pull(cx);
            }
            _ => self.read_head(cx),
        }
    }

    /// `chain.heads` was refused or ended: the head is read on the clock
    /// instead, from then on.
    pub(crate) fn poll_head(&mut self, cx: &mut Context<Self>) {
        if self.polling.is_some() {
            return;
        }
        let ticks = cx.host().subscribe::<ClockTicks>(TICK);
        self.polling = Some(cx.for_each(ticks, |view, tick, _, cx| match tick {
            Ok(()) => view.read_head(cx),
            Err(refusal) => log(cx, "the clock", &refusal),
        }));
    }

    pub(crate) fn read_head(&mut self, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ChainStatus>(());
        if self.status.ready().is_some() {
            cx.refresh(ask, |view, status, cx| {
                view.status = Loadable::Ready(status);
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

    pub(crate) fn read_accounts(&mut self, cx: &mut Context<Self>) {
        let work = queries::accounts(cx.host());
        match self.accounts.ready() {
            Some(_) => cx.refresh(work, |view, accounts, _| {
                view.accounts = Loadable::Ready(accounts)
            }),
            None => self.accounts = cx.load(work, |view| &mut view.accounts),
        }
    }

    pub(crate) fn read_validators(&mut self, cx: &mut Context<Self>) {
        let work = queries::validators(cx.host());
        match self.validators.ready() {
            Some(_) => cx.refresh(work, |view, keys, _| {
                view.validators = Loadable::Ready(keys)
            }),
            None => self.validators = cx.load(work, |view| &mut view.validators),
        }
    }

    pub(crate) fn read_network(&mut self, cx: &mut Context<Self>) {
        let work = queries::network(cx.host());
        match self.network.ready() {
            Some(_) => cx.refresh(work, |view, network, _| {
                view.network = Loadable::Ready(network)
            }),
            None => self.network = cx.load(work, |view| &mut view.network),
        }
    }

    /// Reads the next page the window wants, if any: the head when the
    /// status is past it, else older blocks until the window is full.
    pub(crate) fn pull(&mut self, cx: &mut Context<Self>) {
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
        let ask = cx.host().ask::<ChainBlocks>(BlockPage {
            before,
            limit: PAGE,
        });
        cx.spawn(async move |this, cx| {
            let page = ask.await;
            // the view is gone: nothing is waiting for the page
            let _ = this.update(cx, |view, cx| {
                view.pulling = false;
                match page {
                    Ok(page) => {
                        let was = (view.chain.top(), view.chain.blocks.len());
                        view.chain.land(before, page);
                        // a page that moved nothing is not asked for again
                        // until the head moves
                        if (view.chain.top(), view.chain.blocks.len()) != was {
                            view.pull(cx);
                        }
                    }
                    Err(refusal) => view.chain.failed = Some(refusal.message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks the host what `tx` does, once, the first time it is drawn; a
    /// program that says nothing, or a refusal, leaves its bytes.
    ///
    /// ponytail: drawing asks (through `TxRow`'s cells) so only rows on
    /// screen are described, not every transaction of a 1,000-block
    /// window; a render that stays pure needs the visible rows computed
    /// ahead of it.
    pub(crate) fn describe(&self, tx: &TxRow, cx: &mut Context<Self>) {
        if tx.op.get().is_some() || tx.asked.replace(true) {
            return;
        }
        let ask = cx
            .host()
            .ask::<ModuleDescribe>((tx.target.clone(), tx.payload.clone()));
        let hash = tx.hash;
        cx.spawn(async move |this, cx| {
            let described = ask.await;
            let _ = this.update(cx, |view, cx| {
                let Some(tx) = view.tx(&hash) else {
                    return;
                };
                let op = match described {
                    Ok(Some(op)) => op,
                    Ok(None) => decode::bytes(&tx.target, &tx.payload),
                    Err(refusal) => {
                        log(cx, "a description", &refusal);
                        decode::bytes(&tx.target, &tx.payload)
                    }
                };
                let _ = tx.op.set(op);
                cx.notify();
            });
        })
        .detach();
    }

    /// A transaction by hash, in the window or the block opened outside it.
    pub(crate) fn tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        self.chain
            .txs
            .iter()
            .find(|tx| &tx.hash == hash)
            .or_else(|| self.opened_tx(hash))
    }

    pub(crate) fn opened_tx(&self, hash: &[u8; 32]) -> Option<&TxRow> {
        match self.opened.ready() {
            Some(Some((_, txs))) => txs.iter().find(|tx| &tx.hash == hash),
            _ => None,
        }
    }

    /// A block and its transactions, in the window or opened outside it.
    pub(crate) fn block(&self, height: u64) -> Option<(BlockRow, Vec<&TxRow>)> {
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
}
