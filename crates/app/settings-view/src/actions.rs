//! What the reader's presses do: reads again, each form's submit, and the
//! invite. A refusal lands on the form it came from.
use ducktape_view_guest::Context;
use ducktape_view_guest::methods::ClipboardWrite;
use ducktape_view_guest::view::Loadable;

use crate::api::{ChainStatus, CreateInvite, Identity, InviteCreate, Session, Submit};
use crate::state::{Form, Problem, TTL};
use crate::{Settings, queries};

impl Settings {
    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        if session.account.is_some() {
            self.create_account = Form::default();
        }
        self.session = session;
        self.read_account(cx);
    }

    /// The node status: read, or re-read with what is on screen kept.
    pub(crate) fn read_status(&mut self, cx: &mut Context<Self>) {
        let work = cx.host().ask::<ChainStatus>(());
        if self.status.ready().is_some() {
            cx.refresh(work, |view, status, _| {
                view.status = Loadable::Ready(status)
            });
        } else if !self.status.is_loading() {
            self.status = cx.load(work, |view| &mut view.status);
        }
        cx.notify();
    }

    /// Who the session's key is, read afresh: the boot, a new session, a
    /// retry.
    pub(crate) fn read_account(&mut self, cx: &mut Context<Self>) {
        let work = self.account_query(cx);
        self.account = cx.load(work, |view| &mut view.account);
        cx.notify();
    }

    /// The same account, re-read with what is on screen kept.
    pub(crate) fn refresh_account(&mut self, cx: &mut Context<Self>) {
        if self.account.ready().is_none() {
            return self.read_account(cx);
        }
        let work = self.account_query(cx);
        cx.refresh(work, |view, account, _| {
            view.account = Loadable::Ready(account)
        });
    }

    fn account_query(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Future<Output = Result<Option<queries::Account>, ducktape_view_guest::host::Error>> + 'static
    {
        let (signer, number) = (self.session.signer.clone(), self.session.account);
        queries::account(cx.host(), signer, number)
    }

    pub(crate) fn submit_create_account(&mut self, cx: &mut Context<Self>) {
        if self.create_account.busy {
            return;
        }
        let name = self.create_account.text.trim().to_string();
        if name.is_empty() {
            self.create_account.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        self.create_account.problem = None;
        self.create_account.busy = true;
        cx.notify();
        let op = identity::Op::Create {
            name,
            scheme: abi::Scheme::Ed25519,
        };
        let ask = cx.host().ask::<Submit<Identity>>(op);
        cx.spawn(async move |this, cx| {
            let result = ask.await;
            let _ = this.update(cx, |view, cx| {
                // created: the form stays busy until the host's session
                // names the new account, which re-reads it
                if let Err(refusal) = result {
                    view.create_account.busy = false;
                    view.create_account.problem = Some(Problem::Refused(refusal.message));
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn submit_rename_agent(&mut self, number: u64, cx: &mut Context<Self>) {
        let form = self.rename_agent.entry(number).or_default();
        let name = form.text.trim().to_string();
        if name.is_empty() {
            form.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        let op = identity::Op::SetName {
            account: number,
            name,
        };
        self.submit_agent_op(op, move |v| v.rename_agent.entry(number).or_default(), cx);
    }

    pub(crate) fn submit_create_agent(&mut self, cx: &mut Context<Self>) {
        let name = self.create_agent.text.trim().to_string();
        if name.is_empty() {
            self.create_agent.problem = Some(Problem::Empty);
            cx.notify();
            return;
        }
        let op = identity::Op::CreateAgent { name };
        self.submit_agent_op(op, |v| &mut v.create_agent, cx);
    }

    /// The agent's key request: the hex of an `AddKey` whose consent the
    /// new key signed, for one of the reader's agents; this key submits it
    /// as the manager.
    pub(crate) fn submit_agent_key(&mut self, cx: &mut Context<Self>) {
        let mine = |account: u64| {
            self.account
                .ready()
                .and_then(Option::as_ref)
                .is_some_and(|a| a.agents.iter().any(|agent| agent.number == account))
        };
        let op = abi::unhex(self.agent_key.text.trim())
            .and_then(|bytes| abi::decode::<identity::Op>(&bytes).ok())
            .filter(
                |op| matches!(op, identity::Op::AddKey { consent, .. } if mine(consent.account)),
            );
        let Some(op) = op else {
            self.agent_key.problem = Some(Problem::NotAKeyRequest);
            cx.notify();
            return;
        };
        self.submit_agent_op(op, |v| &mut v.agent_key, cx);
    }

    /// Suspends or resumes an agent.
    pub(crate) fn set_standing(&mut self, op: identity::Op, cx: &mut Context<Self>) {
        self.revoking = None;
        self.submit_agent_op(op, |v| &mut v.agent_standing, cx);
    }

    /// Revokes an agent on the second press: the first only asks.
    pub(crate) fn revoke(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.revoking == Some(number) {
            self.revoking = None;
            let op = identity::Op::Revoke { account: number };
            self.submit_agent_op(op, |v| &mut v.agent_standing, cx);
        } else {
            self.revoking = Some(number);
            cx.notify();
        }
    }

    /// Submits `op` from `form`; done, the form clears and the account is
    /// read again.
    fn submit_agent_op(
        &mut self,
        op: identity::Op,
        form: impl Fn(&mut Settings) -> &mut Form + 'static,
        cx: &mut Context<Self>,
    ) {
        if form(self).busy {
            return;
        }
        let pending = form(self);
        pending.busy = true;
        pending.problem = None;
        cx.notify();
        let ask = cx.host().ask::<Submit<Identity>>(op);
        cx.spawn(async move |this, cx| {
            let result = ask.await;
            let _ = this.update(cx, |view, cx| {
                let form = form(view);
                form.busy = false;
                match result {
                    Ok(_) => form.text.clear(),
                    Err(refusal) => form.problem = Some(Problem::Refused(refusal.message)),
                }
                view.refresh_account(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn mint_invite(&mut self, cx: &mut Context<Self>) {
        self.copied = Loadable::Idle;
        let ask = cx.host().ask::<InviteCreate>(CreateInvite {
            ttl_days: TTL[self.ttl],
        });
        self.invite = cx.load(ask, |view| &mut view.invite);
        cx.notify();
    }

    pub(crate) fn copy_invite(&mut self, cx: &mut Context<Self>) {
        let Some(invite) = self.invite.ready() else {
            return;
        };
        let ask = cx.host().ask::<ClipboardWrite>(invite.invite.clone());
        self.copied = cx.load(ask, |view| &mut view.copied);
        cx.notify();
    }
}
