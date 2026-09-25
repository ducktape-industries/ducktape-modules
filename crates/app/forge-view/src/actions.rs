//! The operations the reader issues, and the forms that stage them.
//!
//! An operation is optimistic: the row that issued it says "Submitting…"
//! straight away, keeps saying so while the block that carries it is on its
//! way, and a refusal replaces it with the reason inline. Nothing is guessed
//! into the lists — the next query reconciles them.
use ducktape_view_guest::methods::HostId;
use ducktape_view_guest::view::Submit;
use ducktape_view_guest::{Context, Window};

use crate::api::{ChatApi, SubmitForge};
use crate::state::{ChangeForm, Forge, NewRepo, Pending, Progress, change_key};
use forge::{Mergeability, Op, Revision, Settings, valid_repo_name};

/// Why a change cannot merge from this view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MergeBlock {
    Loading,
    NotOpen,
    EndpointGone,
    Comparing,
    UpToDate,
    Unrelated,
    Diverged,
}

impl MergeBlock {
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Loading => "This change has not loaded yet",
            Self::NotOpen => "This change is no longer open",
            Self::EndpointGone => "One of the endpoints of this change no longer exists",
            Self::Comparing => "Comparing the endpoints…",
            Self::UpToDate => "The target already contains this change",
            Self::Unrelated => "The endpoints share no history",
            Self::Diverged => "The endpoints diverged: merge with git and push the result",
        }
    }
}

impl Forge {
    /// One operation, optimistic in `scope` until a query reconciles it.
    pub(crate) fn submit(&mut self, op: Op, scope: String, label: &str, cx: &mut Context<Self>) {
        self.next_pending += 1;
        let id = self.next_pending;
        self.pending.push(Pending {
            id,
            scope,
            label: label.to_owned(),
            progress: Progress::Submitting,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.host().ask::<SubmitForge>(op).await;
            // the view is gone: nobody is waiting on this row
            let _ = this.update(cx, |forge, cx| {
                cx.notify();
                let Some(op) = forge.pending.iter_mut().find(|op| op.id == id) else {
                    return;
                };
                match result {
                    Ok(_) => {
                        op.progress = Progress::Accepted;
                        forge.refresh(cx);
                    }
                    Err(refusal) => op.progress = Progress::Refused(refusal.sentence),
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
            form.error = format!(
                "A repository name is 1–{} bytes of letters, digits, dot, dash or underscore",
                forge::MAX_REPO_NAME
            );
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

    /// Why merging is not offered, or `None` when it is.
    ///
    /// The program CASes both heads over a result the client publishes, and
    /// a view holds no Git: it can name the source commit as the result of a
    /// fast-forward and nothing else.
    pub(crate) fn merge_block(&self) -> Option<MergeBlock> {
        let Some((change, source, target, _)) = self.change() else {
            return Some(MergeBlock::Loading);
        };
        if change.state != forge::ChangeState::Open {
            return Some(MergeBlock::NotOpen);
        }
        if source.is_none() || target.is_none() {
            return Some(MergeBlock::EndpointGone);
        }
        match self.compare().map(|c| c.mergeability) {
            None => Some(MergeBlock::Comparing),
            Some(Mergeability::FastForward) => None,
            Some(Mergeability::UpToDate) => Some(MergeBlock::UpToDate),
            Some(Mergeability::Unrelated) => Some(MergeBlock::Unrelated),
            Some(Mergeability::Diverged) => Some(MergeBlock::Diverged),
        }
    }

    pub(crate) fn merge(&mut self, cx: &mut Context<Self>) {
        if self.merge_block().is_some() {
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
        let Some(party) = chat::Party::parse(&typed) else {
            self.notice = "Grant takes an account number or a key in hex".into();
            cx.notify();
            return;
        };
        if let Some(form) = &mut self.repo_settings {
            form.grant.clear();
        }
        self.submit(
            Op::Grant { repo, party },
            "settings".into(),
            "Granting write access",
            cx,
        );
    }

    pub(crate) fn revoke(&mut self, party: chat::Party, cx: &mut Context<Self>) {
        let repo = self.repo_name();
        self.submit(
            Op::Revoke { repo, party },
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
                let message_id = host.ask::<HostId>("message".into()).await?;
                host.ask::<Submit<ChatApi>>(chat::Op::PostMessage {
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
