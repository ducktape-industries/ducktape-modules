//! Everything an event changes: where the reader is, what she has staged,
//! and the operations she issues.
//!
//! An operation is optimistic: the row that issued it says "Submitting…"
//! straight away, keeps saying so while the block that carries it is on its
//! way, and a refusal replaces it with the reason inline. Nothing is guessed
//! into the lists — the next query reconciles them.
use ducktape_view_guest::caps::Id;
use ducktape_view_guest::view::Submit;
use ducktape_view_guest::{Context, Window};

use crate::api::{ChatApi, SubmitForge};
use crate::contract::{
    Mergeability, Op, ReviewDraft, Revision, Settings, Verdict, valid_repo_name,
};
use crate::state::{
    ChangeForm, ChangeTab, Dock, Filter, Forge, NewRepo, Pending, PendingComment, RepoTab,
    ReviewSession, SettingsForm, change_key, unhex,
};

impl Forge {
    // --------------------------------------------------------- navigation

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.notice.clear();
        cx.notify();
        self.sync(cx);
    }

    pub(crate) fn open_repos(&mut self, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.moved(cx);
    }

    pub(crate) fn open_repo(&mut self, name: String, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.nav.repo = Some(name);
        self.moved(cx);
    }

    pub(crate) fn open_tab(&mut self, tab: RepoTab, cx: &mut Context<Self>) {
        let repo = self.nav.repo.clone();
        let rev = self.nav.rev.clone();
        self.nav = Default::default();
        self.nav.repo = repo;
        self.nav.rev = rev;
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
        self.nav.path.clear();
        self.nav.blob = None;
        self.nav.commit = None;
        self.moved(cx);
    }

    pub(crate) fn open_dir(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        self.nav.path = path;
        self.nav.blob = None;
        self.tree_search.clear();
        self.moved(cx);
    }

    pub(crate) fn open_file(&mut self, path: Vec<u8>, oid: String, cx: &mut Context<Self>) {
        self.nav.blob = Some((path, oid));
        self.moved(cx);
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

    // ----------------------------------------------------------- the ops

    /// One operation, optimistic in `scope` until a query reconciles it.
    pub(crate) fn submit(&mut self, op: Op, scope: String, label: &str, cx: &mut Context<Self>) {
        self.next_pending += 1;
        let id = self.next_pending;
        self.pending.push(Pending {
            id,
            scope,
            label: label.to_owned(),
            error: String::new(),
            accepted: false,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.host().ask::<SubmitForge>(op).await;
            let _ = this.update(cx, |forge, cx| {
                cx.notify();
                let Some(op) = forge.pending.iter_mut().find(|op| op.id == id) else {
                    return;
                };
                match result {
                    Ok(()) => op.accepted = true,
                    Err(refusal) => op.error = refusal.sentence.clone(),
                }
                if forge.pending.iter().any(|op| op.id == id && op.accepted) {
                    forge.refresh(cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn create_repo(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.new_repo else {
            return;
        };
        let name = form.name.trim().to_owned();
        if !valid_repo_name(&name) {
            form.error =
                "A repository name is 1–37 bytes of letters, digits, dot, dash or underscore".into();
            cx.notify();
            return;
        }
        let hash = if form.sha256 {
            abi::HashKind::Sha256
        } else {
            abi::HashKind::Sha1
        };
        self.new_repo = None;
        self.submit(
            Op::Create {
                repo: name.clone(),
                hash,
            },
            "repos".into(),
            &format!("Creating {name}"),
            cx,
        );
    }

    pub(crate) fn start_repo(&mut self, cx: &mut Context<Self>) {
        self.new_repo = Some(NewRepo::default());
        cx.notify();
    }

    pub(crate) fn cancel_repo(&mut self, cx: &mut Context<Self>) {
        self.new_repo = None;
        cx.notify();
    }

    /// A Change draft from a ref comparison, or an edit of an open change.
    pub(crate) fn start_change(&mut self, from: Vec<u8>, cx: &mut Context<Self>) {
        self.form = Some(ChangeForm {
            from,
            into: self.default_head(),
            ..ChangeForm::default()
        });
        cx.notify();
    }

    pub(crate) fn start_edit(&mut self, cx: &mut Context<Self>) {
        let Some((change, _, _, _)) = self.change() else {
            return;
        };
        self.form = Some(ChangeForm {
            edit: Some(change.n),
            from: match &change.from {
                Revision::Ref(name) => name.clone(),
                Revision::Oid(oid) => oid.clone().into_bytes(),
            },
            into: change.into.clone(),
            title: change.title.clone(),
            body: change.body.clone(),
            reviewers: change.reviewers.clone(),
            error: String::new(),
        });
        cx.notify();
    }

    pub(crate) fn cancel_change(&mut self, cx: &mut Context<Self>) {
        self.form = None;
        cx.notify();
    }

    pub(crate) fn submit_change(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        let Some(form) = &mut self.form else { return };
        if form.title.trim().is_empty() {
            form.error = "A change needs a title".into();
            cx.notify();
            return;
        }
        let form = form.clone();
        self.form = None;
        let (op, label, scope) = match form.edit {
            Some(n) => (
                Op::ChangeEdit {
                    repo: repo.clone(),
                    n,
                    title: Some(form.title.clone()),
                    body: Some(form.body.clone()),
                    reviewers: Some(form.reviewers.clone()),
                },
                "Saving the change".to_owned(),
                change_key(&repo, n),
            ),
            None => (
                Op::ChangeOpen {
                    repo: repo.clone(),
                    from: Revision::Ref(form.from.clone()),
                    into: form.into.clone(),
                    title: form.title.clone(),
                    body: form.body.clone(),
                    reviewers: form.reviewers.clone(),
                },
                format!("Opening “{}”", form.title.trim()),
                "changes".to_owned(),
            ),
        };
        self.submit(op, scope, &label, cx);
    }

    pub(crate) fn close_change(&mut self, cx: &mut Context<Self>) {
        let (Some(repo), Some(n)) = (self.nav().repo.clone(), self.nav().change) else {
            return;
        };
        self.submit(
            Op::ChangeClose {
                repo: repo.clone(),
                n,
            },
            change_key(&repo, n),
            "Closing this change",
            cx,
        );
    }

    /// Why merging is not offered, as a sentence — empty when it is.
    ///
    /// The program CASes both heads over a result the client publishes, and
    /// a view holds no Git: it can name the source commit as the result of a
    /// fast-forward and nothing else.
    pub(crate) fn merge_refusal(&self) -> String {
        let Some((change, source, target, _)) = self.change() else {
            return "This change has not loaded yet".into();
        };
        if !matches!(change.state, crate::contract::ChangeState::Open) {
            return "This change is no longer open".into();
        }
        if source.is_none() || target.is_none() {
            return "One of the endpoints of this change no longer exists".into();
        }
        match self.compare().map(|c| c.mergeability) {
            None => "Comparing the endpoints…".into(),
            Some(Mergeability::FastForward) => String::new(),
            Some(Mergeability::UpToDate) => "The target already contains this change".into(),
            Some(Mergeability::Conflicts) => "The endpoints conflict".into(),
            Some(Mergeability::Unrelated) => "The endpoints share no history".into(),
            Some(Mergeability::Clean) => {
                "This merge needs a merge commit, which this view cannot build yet".into()
            }
        }
    }

    pub(crate) fn merge(&mut self, cx: &mut Context<Self>) {
        if !self.merge_refusal().is_empty() {
            return;
        }
        let Some(repo) = self.nav().repo.clone() else {
            return;
        };
        let Some((change, source, target, _)) = self.change() else {
            return;
        };
        let (Some(source), Some(target)) = (source.clone(), target.clone()) else {
            return;
        };
        let (n, from, into) = (change.n, change.from.clone(), change.into.clone());
        self.submit(
            Op::Merge {
                repo: repo.clone(),
                into,
                from,
                expected_into: target,
                expected_from: source.clone(),
                result: source,
                change: Some(n),
            },
            change_key(&repo, n),
            "Merging this change",
            cx,
        );
    }

    // -------------------------------------------------------- the review

    fn review_key(&self) -> Option<String> {
        Some(change_key(
            self.nav().repo.as_deref()?,
            self.nav().change?,
        ))
    }

    /// Start a review pinned at the endpoints on screen. Later pushes make
    /// the pin outdated; they never move it.
    pub(crate) fn start_review(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        let Some((_, source, _, _)) = self.change() else {
            return;
        };
        let Some(commit) = source.clone() else { return };
        let base = self.compare().and_then(|c| c.base.clone());
        self.reviews.insert(
            key,
            ReviewSession {
                commit,
                base,
                ..ReviewSession::default()
            },
        );
        cx.notify();
    }

    pub(crate) fn cancel_review(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.review_key() {
            self.reviews.remove(&key);
        }
        cx.notify();
    }

    /// The gutter number is the button: it opens the composer for one anchor,
    /// carrying whatever is already staged there.
    pub(crate) fn open_comment(
        &mut self,
        path: Vec<u8>,
        new_side: bool,
        line: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.review_key() else { return };
        if !self.reviews.contains_key(&key) {
            self.start_review(cx);
        }
        let staged = self
            .reviews
            .get(&key)
            .and_then(|review| review.staged(&path, new_side, line).cloned());
        if let Some(review) = self.reviews.get_mut(&key) {
            review.error.clear();
            review.open = Some(staged.unwrap_or(PendingComment {
                path,
                new_side,
                line,
                body: String::new(),
            }));
        }
        cx.notify();
    }

    pub(crate) fn typed_comment(&mut self, body: String, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(open) = self.reviews.get_mut(&key).and_then(|r| r.open.as_mut()) {
            open.body = body;
        }
        cx.notify();
    }

    pub(crate) fn stage_comment(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        let Some(review) = self.reviews.get_mut(&key) else {
            return;
        };
        let Some(open) = review.open.take() else {
            return;
        };
        if open.body.trim().is_empty() {
            review.comments.retain(|staged| !staged.anchors(&open));
        } else if review.comments.len() >= crate::contract::MAX_REVIEW_COMMENTS
            && review.staged(&open.path, open.new_side, open.line).is_none()
        {
            review.error = format!(
                "A review carries at most {} line comments",
                crate::contract::MAX_REVIEW_COMMENTS
            );
            review.open = Some(open);
        } else {
            review.stage(open);
        }
        cx.notify();
    }

    pub(crate) fn discard_comment(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.open = None;
        }
        cx.notify();
    }

    pub(crate) fn typed_review_body(&mut self, body: String, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.body = body;
        }
        cx.notify();
    }

    pub(crate) fn finishing(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(key) = self.review_key() else { return };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.finishing = on;
        }
        cx.notify();
    }

    /// One verdict and every staged comment as exactly one operation. A
    /// refusal keeps the drafts.
    pub(crate) fn finish_review(&mut self, verdict: Verdict, cx: &mut Context<Self>) {
        let (Some(repo), Some(n)) = (self.nav().repo.clone(), self.nav().change) else {
            return;
        };
        let key = change_key(&repo, n);
        let Some(review) = self.reviews.get(&key).cloned() else {
            return;
        };
        let comments = review.line_comments();
        if matches!(verdict, Verdict::Comment) && review.body.trim().is_empty() && comments.is_empty()
        {
            if let Some(review) = self.reviews.get_mut(&key) {
                review.error = "A comment review needs a body or a line comment".into();
            }
            cx.notify();
            return;
        }
        let draft = ReviewDraft {
            commit_oid: review.commit.clone(),
            base_oid: review.base.clone(),
            verdict,
            body: review.body.clone(),
            comments,
        };
        if let Some(review) = self.reviews.get_mut(&key) {
            review.finishing = false;
            review.error.clear();
        }
        self.submit(
            Op::ReviewSubmit {
                repo,
                n,
                review: draft,
            },
            key,
            "Submitting this review",
            cx,
        );
    }

    /// A published review's pin against the head on screen.
    pub(crate) fn outdated(&self, commit_oid: &str) -> bool {
        self.change()
            .and_then(|(_, source, _, _)| source.clone())
            .is_some_and(|head| head != commit_oid)
    }

    // ------------------------------------------------------ repo settings

    pub(crate) fn configure(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        let Some(form) = self.repo_settings.clone() else {
            return;
        };
        self.submit(
            Op::Configure {
                repo,
                settings: Settings {
                    head: form.head,
                    allow_force: form.allow_force,
                    allow_delete: form.allow_delete,
                },
            },
            "settings".into(),
            "Saving these settings",
            cx,
        );
    }

    pub(crate) fn grant(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        let typed = self
            .repo_settings
            .as_ref()
            .map(|form| form.grant.trim().to_owned())
            .unwrap_or_default();
        let key = unhex(&typed).or_else(|| {
            let names = self.names.ready()?;
            let number = typed.strip_prefix("acct:").unwrap_or(&typed).parse().ok()?;
            names.key_of(number)
        });
        let Some(key) = key else {
            self.notice = "Grant takes an account number or a key in hex".into();
            cx.notify();
            return;
        };
        if let Some(form) = &mut self.repo_settings {
            form.grant.clear();
        }
        self.submit(
            Op::Grant { repo, key },
            "settings".into(),
            "Granting write access",
            cx,
        );
    }

    pub(crate) fn revoke(&mut self, key: Vec<u8>, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        self.submit(
            Op::Revoke { repo, key },
            "settings".into(),
            "Revoking write access",
            cx,
        );
    }

    // ------------------------------------------------------ conversation

    /// A reply in the change's hidden channel. Chat owns every reply; forge
    /// owns only the change's own body.
    pub(crate) fn post_reply(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.reply.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let Some((change, _, _, _)) = self.change() else {
            return;
        };
        let channel = change.channel.clone();
        self.reply.clear();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let result = async {
                let message_id = host.ask::<Id>("message".into()).await?;
                host.ask::<Submit<ChatApi>>(chat::ChatMsg::PostMessage {
                    channel_id: channel,
                    message_id,
                    blocks: chat::parse_message(&text),
                    thread: None,
                })
                .await
            }
            .await;
            let _ = this.update(cx, |forge, cx| {
                cx.notify();
                match result {
                    Ok(_) => forge.refresh(cx),
                    Err(refusal) => {
                        forge.notice = format!("That didn’t go through: {}", refusal.sentence)
                    }
                }
            });
        })
        .detach();
    }
}
