//! What the screens read out of the view: the replies that have landed,
//! picked by the query that asked them, and the reader's own identity.
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;

use crate::queries::PAGE;
use crate::state::{self, ChangeTab, Forge, Nav, change_key};
use forge::{
    Bounds, Change, Comparison, PageResponse, Query, RefInfo, Reply, RepoInfo, Review, Revision,
};
use identity::Principal;

/// The open change, as its screens read it: the record, its two current
/// endpoints (either can be gone) and the reviews landed so far.
pub(crate) type OpenChange<'a> = (
    &'a Change,
    &'a Option<String>,
    &'a Option<String>,
    &'a PageResponse<Review>,
);

/// A read, in the three states a screen draws.
pub(crate) enum Stage<'a> {
    Loading,
    Failed(&'a Refusal),
    Ready(&'a Reply),
}

impl Forge {
    /// The reader's account number, as the host resolved it.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.session.account
    }

    /// The reader as a principal: her account; nobody while her seated key
    /// holds none. [`Principal::writer`] is the one rule every view gates its
    /// writes on, as identity's `principal_of` refuses them.
    pub(crate) fn me_principal(&self) -> Option<Principal> {
        Principal::writer(self.my_account())
    }

    /// Whether the reader writes at all: connected, and an account to write
    /// as. A key that holds none reads everything and writes nothing.
    pub(crate) fn may_write(&self) -> bool {
        self.session.connected && self.me_principal().is_some()
    }

    /// Whether a list on this screen stopped at its page budget: a read
    /// with a cursor left over, or a change's conversation cut short.
    pub(crate) fn cut_short(&self) -> bool {
        let reads = self.data.values().any(|loaded| match loaded {
            Loaded::Ready(reply) => crate::queries::cut_short(reply),
            _ => false,
        });
        let talk = self
            .messages
            .values()
            .any(|loaded| matches!(loaded, Loaded::Ready((_, true))));
        reads || talk
    }

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

    pub(crate) fn repo(&self) -> Option<(&RepoInfo, &Bounds, &PageResponse<Principal>)> {
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

    /// The principals chat marks as the reader's own: at most one.
    pub(crate) fn viewer(&self) -> Vec<Principal> {
        self.me_principal().into_iter().collect()
    }

    /// What a person or chat author is called: their account name once
    /// the roster has landed.
    pub(crate) fn principal_name(&self, principal: &Principal) -> String {
        match (principal, self.names.ready()) {
            // forge's own lines in a change's channel
            (Principal::Root, _) => "Forge".into(),
            (_, Some(names)) => names.member(principal),
            (_, None) => chat::view::unnamed(principal),
        }
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
