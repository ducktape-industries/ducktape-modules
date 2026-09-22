//! Writing a review: what is staged, at which anchor, and the one operation
//! it all becomes. A refusal keeps every draft.
use ducktape_view_guest::Context;

use crate::state::{Forge, PendingComment, ReviewSession, change_key};
use forge::{Op, ReviewDraft, Verdict};

impl Forge {
    // -------------------------------------------------------- the review

    fn review_key(&self) -> Option<String> {
        Some(change_key(self.nav().repo.as_deref()?, self.nav().change?))
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
        } else if review.comments.len() >= forge::MAX_REVIEW_COMMENTS
            && review
                .staged(&open.path, open.new_side, open.line)
                .is_none()
        {
            review.error = format!(
                "A review carries at most {} line comments",
                forge::MAX_REVIEW_COMMENTS
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
        if matches!(verdict, Verdict::Comment)
            && review.body.trim().is_empty()
            && comments.is_empty()
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
}
