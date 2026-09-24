//! Forge: repositories, code, commits, refs and the Changes a reviewer
//! lives in, on the view-guest `View` shape.
//!
//! The forge program answers everything this screen shows, in borsh, through
//! one door (`rpc.query_bytes`); conversation is chat's, through the door
//! chat-view uses. Reads are a cache keyed by the query itself: [`Forge::sync`]
//! asks what the current screen needs, issues what is missing, and drops what
//! the reader has navigated away from. `render` never mutates — what an event
//! changes lands in `actions`.
mod api;
mod queries;
mod state;
mod tree;

mod actions;
mod review;
mod ui;

use std::collections::BTreeSet;

use ducktape_view_guest::doors::{Live, Route, Visible};
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{Context, IntoElement, Render, View, Window, export_view};
use futures::StreamExt;

use api::Props;
use forge::{
    Bounds, Change, ChangeFilter, ChangeState, Comparison, PageReply, Query, RefInfo, Reply,
    RepoInfo, Review, Revision,
};
use queries::PAGE;
pub use state::Forge;
use state::{ChangeTab, Filter, Nav, RepoTab, change_key};

/// How many branches the Refs screen compares against the default head in
/// one pass. Beyond that the screen says so rather than walking a fleet of
/// refs through the program's compare budget.
const COMPARED_REFS: usize = 20;

impl View for Forge {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180,760";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut forge = Self::default();
        forge.restored(window, cx);
        forge
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watches.clear();
        let mut props = cx.host().subscribe::<Props>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while let Some(item) = props.next().await {
                if this
                    .update(cx, |forge, cx| {
                        match item {
                            Ok(session) => forge.session_changed(session, cx),
                            Err(refusal) => {
                                forge.notice =
                                    format!("Couldn’t read the session: {}", refusal.sentence)
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        // `duck://<chain>/forge/<name>`: a link opened into this view names
        // the repository to open
        let mut routes = cx.host().subscribe::<Route>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while let Some(route) = routes.next().await {
                let Ok(route) = route else { break };
                if this
                    .update(cx, |forge, cx| forge.open_route(&route, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        // identity's own block matters too: a key that gains an account
        // while this view is open (Settings, then back to Forge) writes no
        // session change of its own, only an identity block.
        for module in ["forge", "chat", identity::PROGRAM] {
            let mut live = cx.host().subscribe::<Live>(module.into());
            self.watches.push(cx.spawn(async move |this, cx| {
                while live.next().await.is_some() {
                    if this.update(cx, |forge, cx| forge.reconcile(cx)).is_err() {
                        break;
                    }
                }
            }));
        }
        let mut visible = cx.host().subscribe::<Visible>(());
        self.watches.push(cx.spawn(async move |this, cx| {
            while let Some(item) = visible.next().await {
                if this
                    .update(cx, |forge, cx| {
                        if item.unwrap_or(false) {
                            forge.refresh(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        if self.names.is_idle() {
            self.names = cx.load(queries::roster(cx.host()), |forge| &mut forge.names);
        }
        if self.me.is_idle() {
            self.refresh_me(cx);
        }
        self.sync(cx);
    }
}

impl Render for Forge {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

/// The open change, as its screens read it: the record, its two current
/// endpoints (either can be gone) and the reviews landed so far.
pub(crate) type OpenChange<'a> = (
    &'a Change,
    &'a Option<String>,
    &'a Option<String>,
    &'a PageReply<Review>,
);

/// A read, in the three states a screen draws.
pub(crate) enum Stage<'a> {
    Loading,
    Failed(&'a Refusal),
    Ready(&'a Reply),
}

impl Forge {
    fn session_changed(&mut self, next: api::Session, cx: &mut Context<Self>) {
        let reader_changed = next.account != self.session.account;
        self.session = next;
        if reader_changed {
            self.names = cx.load(queries::roster(cx.host()), |forge| &mut forge.names);
            self.refresh_me(cx);
            self.data.clear();
        }
        self.sync(cx);
    }

    /// Re-asks identity for the account the seated key holds now. Called on
    /// every key change and on identity's own live stream, so a key that
    /// gains an account while this view stays open re-enables writes
    /// without a relaunch.
    fn refresh_me(&mut self, cx: &mut Context<Self>) {
        let key = self.session.account.clone();
        self.me = cx.load(queries::resolve_me(cx.host(), key), |forge| &mut forge.me);
    }

    /// The reader's account number, once identity has answered.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.me.ready().copied().flatten()
    }

    /// The reader's handle as chat and forge would write it: `acct:<n>`
    /// once the seated key holds an account, `user:<hex>` while it is
    /// seated but holds none, "" with no key seated at all.
    pub(crate) fn my_handle(&self) -> String {
        match self.my_account() {
            Some(number) => format!("acct:{number}"),
            None if self.session.account.is_empty() => String::new(),
            None => format!("user:{}", self.session.account),
        }
    }

    /// One read, once. Landing it advances whatever depends on it.
    pub(crate) fn read(&mut self, query: Query, cx: &mut Context<Self>) {
        if self.data.contains_key(&query) {
            return;
        }
        let landing = query.clone();
        let asked = query.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = queries::fetch(cx.host(), asked).await;
            let _ = this.update(cx, |forge, cx| {
                forge.data.insert(
                    landing,
                    match result {
                        Ok(reply) => Loaded::Ready(reply),
                        Err(refusal) => Loaded::Failed(refusal),
                    },
                );
                cx.notify();
                forge.sync(cx);
            });
        });
        self.data.insert(query, Loaded::Loading(task));
    }

    /// Ask again for everything on screen, keeping the rows already there
    /// until the fresh ones land.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        for query in self.data.keys().cloned().collect::<Vec<_>>() {
            let landing = query.clone();
            cx.refresh(queries::fetch(cx.host(), query), move |forge, reply, _| {
                forge.data.insert(landing.clone(), Loaded::Ready(reply));
            });
        }
        for channel in self.messages.keys().cloned().collect::<Vec<_>>() {
            let viewer = self.viewer();
            cx.refresh(
                queries::conversation(cx.host(), channel.clone(), viewer),
                move |forge, rows, _| {
                    forge.messages.insert(channel.clone(), Loaded::Ready(rows));
                },
            );
        }
        cx.notify();
        self.sync(cx);
    }

    /// A new block landed: retire what it carried, then re-read. Cheap
    /// enough to also cover an identity block, so a gained account is never
    /// missed for lack of its own dedicated watch.
    pub(crate) fn reconcile(&mut self, cx: &mut Context<Self>) {
        self.pending.retain(|op| !op.accepted);
        self.refresh_me(cx);
        self.refresh(cx);
    }

    /// Retry one read the reader asked to retry.
    pub(crate) fn retry(&mut self, query: Query, cx: &mut Context<Self>) {
        self.data.remove(&query);
        self.read(query, cx);
        cx.notify();
    }

    /// Issue what this screen needs and drop what it does not.
    pub(crate) fn sync(&mut self, cx: &mut Context<Self>) {
        let needed = self.needed();
        let keys: BTreeSet<&Query> = needed.iter().collect();
        self.data.retain(|query, _| keys.contains(query));
        for query in needed {
            self.read(query, cx);
        }
        if let Some(channel) = self.open_channel() {
            if !self.messages.contains_key(&channel) {
                let viewer = self.viewer();
                let landing = channel.clone();
                let task = cx.spawn(async move |this, cx| {
                    let result = queries::conversation(cx.host(), landing.clone(), viewer).await;
                    let _ = this.update(cx, |forge, cx| {
                        forge.messages.insert(
                            landing,
                            match result {
                                Ok(rows) => Loaded::Ready(rows),
                                Err(refusal) => Loaded::Failed(refusal),
                            },
                        );
                        cx.notify();
                    });
                });
                self.messages.insert(channel, Loaded::Loading(task));
            }
        } else {
            self.messages.clear();
        }
    }

    /// Every question the current screen has. A question whose arguments are
    /// not known yet (a tree needs its commit) simply is not asked until the
    /// read that answers it lands.
    fn needed(&self) -> Vec<Query> {
        let mut wanted = vec![Query::Repos { page: PAGE }];
        let Some(repo) = self.nav.repo.clone() else {
            return wanted;
        };
        wanted.push(Query::Repo {
            repo: repo.clone(),
            page: PAGE,
        });
        wanted.push(Query::Refs {
            repo: repo.clone(),
            page: PAGE,
        });
        wanted.push(Query::Activity { repo: repo.clone() });
        if let Some(n) = self.nav.change {
            wanted.extend(self.change_reads(&repo, n));
            return wanted;
        }
        match self.nav.tab {
            RepoTab::Readme => {
                wanted.extend(self.tree_query(Vec::new()));
                if let Some((_, oid)) = self.readme() {
                    wanted.push(Query::Blob {
                        repo,
                        oid,
                        range: None,
                    });
                }
            }
            RepoTab::Code => {
                wanted.extend(self.tree_queries());
                if let Some((_, oid)) = &self.nav.blob {
                    wanted.push(Query::Blob {
                        repo,
                        oid: oid.clone(),
                        range: None,
                    });
                }
            }
            RepoTab::Commits => {
                wanted.push(Query::Log {
                    repo: repo.clone(),
                    from: self.revision(),
                    page: PAGE,
                });
                if let Some(commit) = self.nav.commit.clone() {
                    let base = self.commit_parent(&commit);
                    wanted.push(Query::Diff {
                        repo,
                        base,
                        head: commit,
                        path: None,
                        page: PAGE,
                    });
                }
            }
            RepoTab::Changes => wanted.push(self.changes_query(&repo)),
            RepoTab::Refs => {
                let head = self.head_name();
                for name in self.branches().into_iter().take(COMPARED_REFS) {
                    if name == head {
                        continue;
                    }
                    wanted.push(Query::Compare {
                        repo: repo.clone(),
                        from: Revision::Ref(name),
                        into: Revision::Ref(head.clone()),
                    });
                }
            }
            RepoTab::Settings => {}
        }
        wanted
    }

    /// The reads one open change needs: its record, how it compares with its
    /// target, and the diff of that comparison.
    fn change_reads(&self, repo: &str, n: u64) -> Vec<Query> {
        let mut wanted = vec![Query::Change {
            repo: repo.to_owned(),
            n,
            page: PAGE,
        }];
        let Some((change, source_head, _, _)) = self.change() else {
            return wanted;
        };
        wanted.push(Query::Compare {
            repo: repo.to_owned(),
            from: change.from.clone(),
            into: Revision::Ref(change.into.clone()),
        });
        if self.nav.change_tab == ChangeTab::Commits {
            wanted.push(Query::Log {
                repo: repo.to_owned(),
                from: change.from.clone(),
                page: PAGE,
            });
        }
        if let (ChangeTab::Files, Some(head), Some(comparison)) =
            (self.nav.change_tab, source_head.clone(), self.compare())
        {
            wanted.push(Query::Diff {
                repo: repo.to_owned(),
                base: comparison.base.clone(),
                head,
                path: None,
                page: PAGE,
            });
        }
        wanted
    }

    pub(crate) fn changes_query(&self, repo: &str) -> Query {
        let me = self.me_key();
        match self.filter {
            Filter::Judgment => Query::Judgment {
                key: me.unwrap_or_default(),
                page: PAGE,
            },
            other => Query::Changes {
                repo: repo.to_owned(),
                filter: ChangeFilter {
                    state: match other {
                        Filter::Merged => Some(ChangeState::Merged),
                        Filter::Closed => Some(ChangeState::Closed),
                        Filter::Open => Some(ChangeState::Open),
                        _ => None,
                    },
                    author: (other == Filter::Authored).then(|| me.clone().unwrap_or_default()),
                    involves: (other == Filter::Involves).then(|| me.unwrap_or_default()),
                },
                page: PAGE,
            },
        }
    }

    // ---------------------------------------------------------- accessors

    pub(crate) fn stage(&self, query: &Query) -> Stage<'_> {
        match self.data.get(query) {
            Some(Loaded::Ready(reply)) => Stage::Ready(reply),
            Some(Loaded::Failed(refusal)) => Stage::Failed(refusal),
            _ => Stage::Loading,
        }
    }

    pub(crate) fn ready(&self, query: &Query) -> Option<&Reply> {
        match self.stage(query) {
            Stage::Ready(reply) => Some(reply),
            _ => None,
        }
    }

    pub(crate) fn repo_name(&self) -> String {
        self.nav.repo.clone().unwrap_or_default()
    }

    pub(crate) fn repo_query(&self) -> Query {
        Query::Repo {
            repo: self.repo_name(),
            page: PAGE,
        }
    }

    pub(crate) fn refs_query(&self) -> Query {
        Query::Refs {
            repo: self.repo_name(),
            page: PAGE,
        }
    }

    pub(crate) fn repo(&self) -> Option<(&RepoInfo, &Bounds, &PageReply<Vec<u8>>)> {
        match self.ready(&self.repo_query())? {
            Reply::Repo {
                repo,
                bounds,
                writers,
                ..
            } => Some((repo, bounds, writers)),
            _ => None,
        }
    }

    pub(crate) fn refs(&self) -> Option<&[RefInfo]> {
        match self.ready(&self.refs_query())? {
            Reply::Refs { page, .. } => Some(&page.items),
            _ => None,
        }
    }

    pub(crate) fn branches(&self) -> Vec<Vec<u8>> {
        self.refs()
            .unwrap_or_default()
            .iter()
            .filter(|info| info.name.starts_with(b"refs/heads/"))
            .map(|info| info.name.clone())
            .collect()
    }

    /// The default head of the repo, or `refs/heads/main` before it lands.
    pub(crate) fn default_head(&self) -> Vec<u8> {
        self.repo()
            .map(|(info, _, _)| info.repo.settings.head.clone())
            .unwrap_or_else(|| b"refs/heads/main".to_vec())
    }

    /// The ref the reader is browsing.
    pub(crate) fn head_name(&self) -> Vec<u8> {
        self.nav.rev.clone().unwrap_or_else(|| self.default_head())
    }

    pub(crate) fn revision(&self) -> Revision {
        self.nav.revision(&self.default_head())
    }

    /// The commit the browsed ref points at, once refs have landed.
    pub(crate) fn head_oid(&self) -> Option<String> {
        let name = self.head_name();
        self.refs()?
            .iter()
            .find(|info| info.name == name)
            .map(|info| info.target.clone())
    }

    /// The README of the root tree: `README.md` over any other `README*`.
    pub(crate) fn readme(&self) -> Option<(Vec<u8>, String)> {
        let Reply::Tree { page, .. } = self.ready(&self.tree_query(Vec::new())?)? else {
            return None;
        };
        page.items
            .iter()
            .filter(|entry| {
                entry.kind != forge::EntryKind::Directory
                    && String::from_utf8_lossy(&entry.name)
                        .to_lowercase()
                        .starts_with("readme")
            })
            .min_by_key(|entry| !entry.name.eq_ignore_ascii_case(b"readme.md"))
            .map(|entry| (entry.name.clone(), entry.oid.clone()))
    }

    pub(crate) fn commit_parent(&self, oid: &str) -> Option<String> {
        let log = self.ready(&Query::Log {
            repo: self.repo_name(),
            from: self.revision(),
            page: PAGE,
        })?;
        let Reply::Log { page, .. } = log else {
            return None;
        };
        page.items
            .iter()
            .find(|commit| commit.oid == oid)?
            .parents
            .first()
            .cloned()
    }

    pub(crate) fn change_query(&self) -> Option<Query> {
        Some(Query::Change {
            repo: self.nav.repo.clone()?,
            n: self.nav.change?,
            page: PAGE,
        })
    }

    pub(crate) fn change(&self) -> Option<OpenChange<'_>> {
        match self.ready(&self.change_query()?)? {
            Reply::Change {
                change,
                source_head,
                target_head,
                reviews,
                ..
            } => Some((change, source_head, target_head, reviews)),
            _ => None,
        }
    }

    pub(crate) fn compare_query(&self) -> Option<Query> {
        let (change, _, _, _) = self.change()?;
        Some(Query::Compare {
            repo: self.nav.repo.clone()?,
            from: change.from.clone(),
            into: Revision::Ref(change.into.clone()),
        })
    }

    pub(crate) fn compare(&self) -> Option<&Comparison> {
        match self.ready(&self.compare_query()?)? {
            Reply::Compare { comparison, .. } => Some(comparison),
            _ => None,
        }
    }

    pub(crate) fn diff_query(&self) -> Option<Query> {
        let (_, source_head, _, _) = self.change()?;
        Some(Query::Diff {
            repo: self.nav.repo.clone()?,
            base: self.compare()?.base.clone(),
            head: source_head.clone()?,
            path: None,
            page: PAGE,
        })
    }

    /// The reader's signing key, joined from the roster: `host.props` names
    /// an account and forge is keyed by keys.
    pub(crate) fn me_key(&self) -> Option<Vec<u8>> {
        self.names.ready()?.key_of(self.my_account()?)
    }

    pub(crate) fn viewer(&self) -> Vec<String> {
        let me = self.my_handle();
        if me.is_empty() { Vec::new() } else { vec![me] }
    }

    /// The hidden chat channel of the open change.
    pub(crate) fn open_channel(&self) -> Option<String> {
        let (change, _, _, _) = self.change()?;
        (self.nav.change_tab == ChangeTab::Conversation).then(|| change.channel.clone())
    }

    pub(crate) fn review(&self) -> Option<&state::ReviewSession> {
        self.reviews
            .get(&change_key(self.nav.repo.as_deref()?, self.nav.change?))
    }

    pub(crate) fn pending_in(&self, scope: &str) -> Vec<&state::Pending> {
        self.pending.iter().filter(|op| op.scope == scope).collect()
    }

    pub(crate) fn nav(&self) -> &Nav {
        &self.nav
    }
}

export_view!(
    Forge,
    "Forge",
    "Repositories, code, commits and the changes waiting on your judgment.",
    ["rpc", "op", "host"]
);

#[cfg(test)]
mod tests;
