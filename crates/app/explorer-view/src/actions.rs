//! Where the reader goes: a tab or a row, a search, a link opened into the
//! explorer, and a page's link copied out of it.
use ducktape_view_guest::Context;
use ducktape_view_guest::methods::{BlockRef, ChainBlock, ClipboardWrite};
use ducktape_view_guest::view::Loadable;

use crate::Explorer;
use crate::chain::rows;
use crate::state::{Note, Route, hash_of};

impl Explorer {
    pub(crate) fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        if let Route::Block(height) = route {
            let held = self.chain.block(height).is_some();
            let opened =
                matches!(self.opened.ready(), Some(Some((row, _))) if row.height == height);
            if !held && !opened {
                let ask = cx.host().ask::<ChainBlock>(BlockRef::Height(height));
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
    pub(crate) fn search(&mut self, cx: &mut Context<Self>) {
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
            return self.find_hash(hash, cx);
        }
        let number = query
            .strip_prefix('#')
            .and_then(|n| n.trim().parse::<u64>().ok());
        let lower = query.to_lowercase();
        let accounts = self.account_list();
        let named = |account: &&identity::Account| account.card.name.to_lowercase() == lower;
        let account = accounts
            .iter()
            .find(|account| Some(account.number) == number || named(account))
            .or_else(|| {
                accounts
                    .iter()
                    .find(|account| account.card.name.to_lowercase().contains(&lower))
            });
        if let Some(account) = account {
            return self.go(Route::Account(account.number), cx);
        }
        if let Some(entry) = self.runs(&lower) {
            return self.go(Route::Transactions(Some(entry.program.clone())), cx);
        }
        self.note = Some(Note::NotFound(query));
        cx.notify();
    }

    /// A hash, as search and a link read it: a transaction or a block in the
    /// window, else a block the node finds by id.
    fn find_hash(&mut self, hash: [u8; 32], cx: &mut Context<Self>) {
        if self.tx(&hash).is_some() {
            return self.go(Route::Tx(hash), cx);
        }
        if let Some(block) = self.chain.blocks.iter().find(|block| block.id == hash) {
            return self.go(Route::Block(block.height), cx);
        }
        let ask = cx.host().ask::<ChainBlock>(BlockRef::Id(hash));
        let blocks = self.chain.blocks.len() as u64;
        cx.spawn(async move |this, cx| {
            let found = ask.await;
            let _ = this.update(cx, |view, cx| {
                match found {
                    Ok(Some(block)) => {
                        let (row, txs) = rows(block);
                        let height = row.height;
                        view.opened = Loadable::Ready(Some((row, txs)));
                        view.go(Route::Block(height), cx);
                    }
                    Ok(None) => view.note = Some(Note::NoSuchHash { blocks }),
                    Err(refusal) => view.note = Some(Note::Refused(refusal.message)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A `host.route` item, the path of a `duck://…/explorer/<route>` link:
    /// a [`Route::path`], or `block/<hash>`, found like a searched hash.
    pub(crate) fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        if let Some(opened) = Route::from_path(route) {
            // a transaction outside the window reads as "not in the last N
            // blocks" until the window reaches it
            return self.go(opened, cx);
        }
        if let Some(hash) = route.strip_prefix("block/").and_then(hash_of) {
            return self.find_hash(hash, cx);
        }
        self.note = Some(Note::Unlinked(route.to_owned()));
        cx.notify();
    }

    /// `duck://<chain>/explorer/<route>`, while the session names a chain.
    pub(crate) fn link(&self, route: &Route) -> Option<String> {
        let path = route.path();
        let tail: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        ducklink::mint(&self.session_chain, "explorer", &tail)
    }

    pub(crate) fn copy_link(&mut self, link: String, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ClipboardWrite>(link);
        cx.spawn(async move |this, cx| {
            let copied = ask.await;
            let _ = this.update(cx, |view, cx| {
                view.note = Some(match copied {
                    Ok(()) => Note::Copied,
                    Err(refusal) => Note::Refused(refusal.message),
                });
                cx.notify();
            });
        })
        .detach();
    }
}
