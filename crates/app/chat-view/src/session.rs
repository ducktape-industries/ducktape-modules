//! Who reads: the session the host hands over, the account identity says
//! the seated key holds, and what that lets her do in the open room.
use chat::Party;
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Loaded;

use crate::Chat;
use crate::api::Session;
use crate::queries::{channels, resolve_me, roster};

/// Why the reader may not write in the open room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// every write is an account's: a key that holds none only reads
    NoAccount,
    Archived,
    /// members-only, and she is not on the roster
    NotMember,
}

impl Chat {
    pub(crate) fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.account != prev.account
            || self.session.endpoint != prev.endpoint
            || self.session.chain != prev.chain;
        if reader_changed {
            self.load_names(cx);
            self.refresh_me(cx);
        }
        if reader_changed || (prev.connected && !self.session.connected) {
            for draft in self.drafts.values_mut() {
                draft.retire_device_requests();
            }
            self.reads.cursors.clear();
            self.reads.kept = None;
            self.create = None;
        }
        if self.session.connected && (reader_changed || !prev.connected) {
            self.load_kept(cx);
        }
        if !prev.connected && self.session.connected {
            self.load_channels(cx);
            self.refresh(cx);
        }
    }

    pub(crate) fn visibility_changed(&mut self, visible: bool, cx: &mut Context<Self>) {
        if !visible {
            self.create = None;
        }
        if self.reads.visible == visible {
            return;
        }
        self.reads.visible = visible;
        self.reads.entering = visible;
        if visible && self.session.connected {
            self.reread_channels(cx);
        }
    }

    pub(crate) fn load_names(&mut self, cx: &mut Context<Self>) {
        self.names = cx.load(roster(cx.host()), |chat| &mut chat.names);
    }

    pub(crate) fn load_channels(&mut self, cx: &mut Context<Self>) {
        self.channels = cx.load(channels(cx.host()), |chat| &mut chat.channels);
    }

    /// Re-asks identity for the account the seated key holds now: on every
    /// key change and on identity's live heads, so a key that gains an
    /// account while this view stays open writes without a relaunch. The
    /// rooms are looked at again once she is known: the relaunch recount
    /// of what is meant for her waits on her account.
    pub(crate) fn refresh_me(&mut self, cx: &mut Context<Self>) {
        let me = resolve_me(cx.host(), self.session.account.clone());
        self.me = Loaded::Loading(cx.spawn(async move |this, cx| {
            let me = me.await;
            let _ = this.update(cx, |chat, cx| {
                chat.me = Loaded::from(me);
                if let Some(list) = chat.channels.ready().cloned() {
                    chat.channels_landed(list, cx);
                }
                cx.notify();
            });
        }));
    }

    /// The reader's account number, once identity has answered.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.me.ready().copied().flatten()
    }

    /// The party chat writes the reader as; none with no key seated.
    pub(crate) fn me(&self) -> Option<Party> {
        Party::reader(self.my_account(), &self.session.account)
    }

    /// The reader, as a query's `viewer`.
    pub(crate) fn viewer(&self) -> Vec<Party> {
        self.me().into_iter().collect()
    }

    pub(crate) fn holds_account(&self) -> bool {
        self.my_account().is_some()
    }

    /// Why the reader may not write in the open room; none when she may.
    pub(crate) fn write_gate(&self) -> Option<Gate> {
        if !self.holds_account() {
            return Some(Gate::NoAccount);
        }
        let info = self.room_info()?;
        if info.channel.archived {
            return Some(Gate::Archived);
        }
        let seated = self
            .room
            .as_ref()
            .and_then(|room| room.members.ready())
            .is_some_and(|members| members.iter().any(|m| Some(&m.party) == self.me().as_ref()));
        if info.channel.members_only() && !seated {
            return Some(Gate::NotMember);
        }
        None
    }

    pub(crate) fn may_write(&self) -> bool {
        self.write_gate().is_none()
    }
}
