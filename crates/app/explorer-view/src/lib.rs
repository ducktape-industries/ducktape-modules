//! Explorer: the chain as this node keeps it. Finalized blocks and the
//! transactions they carry come from the node's block archive
//! (`chain.blocks`, `chain.block`); who signed them from `identity`; the
//! validators from `valset`; the programs from `module-registry`.
//!
//! What the node does not keep is not shown: there are no receipts, so a
//! transaction is "in block N", never applied or rejected; there is no
//! per-block state root or write set; and nothing indexes an account's
//! history, so an account's activity is what a scan of the recent window
//! finds. The window is the last [`WINDOW`] blocks, read a page at a time
//! and then followed at the head as `chain.heads` pushes it.
//!
//! The state is one struct (`state.rs`) and the window (`chain.rs`). Reads
//! are typed (`queries.rs`) and loaded in `load.rs`, re-read on the live
//! heads `watch.rs` follows; where the reader goes lands in `actions.rs`;
//! `ui/` draws.
mod actions;
mod chain;
mod decode;
mod load;
mod queries;
mod state;
pub(crate) mod ui;
mod watch;

pub use chain::{BlockRow, Chain, TxRow};
pub use state::{Accounts, Explorer, Network, Note, Route};

use ducktape_view_guest::{Context, IntoElement, Render, View, Window, export_view};

/// The recent window the explorer reads: activity, search by transaction
/// hash and the transaction list reach this far back and no further.
pub const WINDOW: usize = 1_000;
/// Blocks per `chain.blocks` page (the node caps a page at 100). Small, so
/// one reply's decoding stays well inside a tick's fuel.
const PAGE: u32 = 20;
/// How often the head is re-read, in milliseconds, where `chain.heads` is
/// refused or ends: the fallback, not the way the head is followed.
const TICK: i64 = 2_000;

impl View for Explorer {
    const PREFERRED_WINDOW_SIZE: &'static str = "1100,720";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx);
        self.read_all(cx);
    }
}

impl Render for Explorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

export_view!(
    Explorer,
    "Explorer",
    "The chain as this node keeps it: blocks, transactions, accounts and programs.",
    ["chain", "module", "host", "clock", "clipboard"]
);

#[cfg(test)]
mod tests;
