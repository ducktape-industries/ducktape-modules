//! Where the reader is: every event that moves between screens, tabs,
//! files and panels. A move clears the notice and re-syncs the reads.
use ducktape_view_guest::Context;

use crate::Stage;
use crate::state::{ChangeTab, Dock, Filter, Forge, RepoTab, SettingsForm, change_key};
use crate::ui::markdown::Target;
use forge::Reply;

impl Forge {
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.notice.clear();
        cx.notify();
        self.sync(cx);
    }

    pub(crate) fn open_repos(&mut self, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.moved(cx);
    }

    /// A `host.route` item: `<name>` opens that repository, `<name>/<n>`
    /// its change `n` (the path chat links a change's room by, its id
    /// `forge:<name>:<n>`); anything else opens the list.
    pub(crate) fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        match route.split('/').collect::<Vec<_>>().as_slice() {
            [name] if !name.is_empty() => self.open_repo((*name).to_owned(), cx),
            [name, n] if !name.is_empty() && n.parse::<u64>().is_ok() => {
                self.open_repo((*name).to_owned(), cx);
                self.open_change(n.parse().ok(), cx);
            }
            _ => self.open_repos(cx),
        }
    }

    pub(crate) fn open_repo(&mut self, name: String, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.nav.repo = Some(name);
        self.moved(cx);
    }

    pub(crate) fn open_tab(&mut self, tab: RepoTab, cx: &mut Context<Self>) {
        // the tree keeps what it had open across tabs
        let kept = std::mem::take(&mut self.nav);
        self.nav.repo = kept.repo;
        self.nav.rev = kept.rev;
        self.nav.expanded = kept.expanded;
        self.nav.cursor = kept.cursor;
        self.nav.blob = kept.blob;
        self.nav.goto = kept.goto;
        self.nav.tab = tab;
        if tab == RepoTab::Settings {
            let head = self.default_head();
            let (allow_force, allow_delete) = self
                .repo()
                .map(|(info, _, _)| {
                    (
                        info.repo.settings.allow_force,
                        info.repo.settings.allow_delete,
                    )
                })
                .unwrap_or((false, false));
            self.repo_settings = Some(SettingsForm {
                head,
                allow_force,
                allow_delete,
                grant: String::new(),
            });
        }
        self.moved(cx);
    }

    pub(crate) fn pick_ref(&mut self, name: Vec<u8>, cx: &mut Context<Self>) {
        self.nav.rev = Some(name);
        self.nav.expanded.clear();
        self.nav.cursor = None;
        self.nav.blob = None;
        self.nav.commit = None;
        self.moved(cx);
    }

    pub(crate) fn open_file(&mut self, path: Vec<u8>, oid: String, cx: &mut Context<Self>) {
        self.nav.cursor = Some(path.clone());
        self.nav.blob = Some((path, oid));
        self.moved(cx);
    }

    /// A pressed link in a document whose folder is `dir`: the web through
    /// the host, a file of this repository in the Code tab (the root README
    /// in its own tab).
    pub(crate) fn follow_link(&mut self, dir: &[u8], dest: &str, cx: &mut Context<Self>) {
        match crate::ui::markdown::target(dir, dest) {
            Some(Target::Web(url)) => cx.host().open_link(&url),
            Some(Target::Path(path)) => self.open_path(path, cx),
            None => {}
        }
    }

    /// Opens a file by its path alone: its folders unfold, and the file
    /// opens once the tree that holds it names its blob.
    pub(crate) fn open_path(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        if self.readme().is_some_and(|(name, _)| name == path) {
            return self.open_tab(RepoTab::Readme, cx);
        }
        let mut dir = &path[..];
        while let Some(at) = dir.iter().rposition(|b| *b == b'/') {
            dir = &dir[..at];
            self.nav.expanded.insert(dir.to_vec());
        }
        self.nav.goto = Some(path);
        self.open_tab(RepoTab::Code, cx);
    }

    /// Lands a pending [`Self::open_path`] once its folder's tree is read.
    pub(crate) fn land_goto(&mut self) {
        let Some(path) = self.nav.goto.clone() else {
            return;
        };
        let split = path.iter().rposition(|b| *b == b'/');
        let (dir, name) = match split {
            Some(at) => (path[..at].to_vec(), &path[at + 1..]),
            None => (Vec::new(), &path[..]),
        };
        let Some(query) = self.tree_query(dir) else {
            return;
        };
        let entry = match self.stage(&query) {
            Stage::Loading => return,
            Stage::Failed(_) => None,
            Stage::Ready(Reply::Tree { page, .. }) => page
                .items
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| (entry.kind, entry.oid.clone())),
            Stage::Ready(_) => None,
        };
        self.nav.goto = None;
        self.nav.cursor = Some(path.clone());
        match entry {
            Some((forge::EntryKind::Directory, _)) => {
                self.nav.expanded.insert(path);
            }
            Some((_, oid)) => self.nav.blob = Some((path, oid)),
            None => self.notice = format!("No {} on this ref.", String::from_utf8_lossy(&path)),
        }
    }

    pub(crate) fn nav_close_blob(&mut self, cx: &mut Context<Self>) {
        self.nav.blob = None;
        self.moved(cx);
    }

    pub(crate) fn open_commit(&mut self, oid: Option<String>, cx: &mut Context<Self>) {
        self.nav.commit = oid;
        self.moved(cx);
    }

    pub(crate) fn open_change(&mut self, n: Option<u64>, cx: &mut Context<Self>) {
        self.nav.change = n;
        self.nav.change_tab = ChangeTab::default();
        self.nav.diff_path = None;
        self.nav.dock = None;
        self.reply.clear();
        self.moved(cx);
    }

    pub(crate) fn open_change_tab(&mut self, tab: ChangeTab, cx: &mut Context<Self>) {
        self.nav.change_tab = tab;
        self.nav.diff_path = None;
        self.moved(cx);
    }

    pub(crate) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.moved(cx);
    }

    pub(crate) fn toggle_dock(&mut self, dock: Dock, cx: &mut Context<Self>) {
        self.nav.dock = (self.nav.dock != Some(dock)).then_some(dock);
        self.layout.dock_open = self.nav.dock.is_some();
        cx.notify();
    }

    pub(crate) fn single_file(&mut self, path: Option<Vec<u8>>, cx: &mut Context<Self>) {
        self.nav.diff_path = path;
        cx.notify();
    }

    pub(crate) fn toggle_viewed(&mut self, path: &[u8], cx: &mut Context<Self>) {
        let Some(key) = self.file_key(path) else {
            return;
        };
        if !self.viewed.remove(&key) {
            self.viewed.insert(key);
        }
        cx.notify();
    }

    pub(crate) fn file_key(&self, path: &[u8]) -> Option<String> {
        Some(format!(
            "{}:{}",
            change_key(self.nav().repo.as_deref()?, self.nav().change?),
            String::from_utf8_lossy(path)
        ))
    }

    pub(crate) fn measured(&mut self, width: f32, height: f32, cx: &mut Context<Self>) {
        if (self.layout.width, self.layout.height) == (width, height) {
            return;
        }
        self.layout.width = width;
        self.layout.height = height;
        cx.notify();
    }
}
