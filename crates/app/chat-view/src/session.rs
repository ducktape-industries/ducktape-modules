//! Who reads: the session the host hands over (the seated key and the
//! account it holds), and what that lets them do in the open room.
use chat::Principal;
use ducktape_view_guest::Context;
use ducktape_view_guest::view::Loadable;

use crate::Chat;
use crate::api::Session;
use crate::queries::channels;
use chat::view::roster;

/// Why the reader may not write in the open room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// every write is an account's: a key that holds none only reads
    NoAccount,
    Archived,
    /// members-only, and they are not on the roster
    NotMember,
}

impl Chat {
    pub(crate) fn session_changed(&mut self, next: Session, cx: &mut Context<Self>) {
        let prev = std::mem::replace(&mut self.session, next);
        let reader_changed = self.session.signer != prev.signer
            || self.session.endpoint != prev.endpoint
            || self.session.chain_id != prev.chain_id;
        if reader_changed {
            self.load_names(cx);
        }
        // a key that gains an account while this view stays open writes
        // without a relaunch; the rooms are looked at again once they are
        // known, since the recount of what is meant for them waits on it
        if (reader_changed || self.session.account != prev.account)
            && let Some(list) = self.channels.ready().cloned()
        {
            self.channels_landed(list, cx);
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
        let list = channels(cx.host());
        let task = cx.spawn(async move |this, cx| {
            let result = list.await;
            let _ = this.update(cx, |chat, cx| {
                chat.channels = Loadable::from(result.map(|(rooms, more)| {
                    chat.channels_more = more;
                    rooms
                }));
                cx.notify();
            });
        });
        self.channels = Loadable::Loading(task);
    }

    /// The reader's account number, as the host resolved it.
    pub(crate) fn my_account(&self) -> Option<u64> {
        self.session.account
    }

    /// The principal chat writes the reader as: their account; none while their
    /// key holds none, since only an account writes.
    pub(crate) fn me(&self) -> Option<Principal> {
        Principal::writer(self.my_account())
    }

    /// The reader, as a query's `viewer`.
    pub(crate) fn viewer(&self) -> Vec<Principal> {
        self.me().into_iter().collect()
    }

    pub(crate) fn holds_account(&self) -> bool {
        self.me().is_some()
    }

    /// Why the reader may not write in the open room; none when they may.
    pub(crate) fn write_gate(&self) -> Option<Gate> {
        if !self.holds_account() {
            return Some(Gate::NoAccount);
        }
        let info = self.room_info()?;
        if info.channel.archived {
            return Some(Gate::Archived);
        }
        let me = self.me()?;
        let seated = self
            .room
            .as_ref()
            .and_then(|room| room.members.ready())
            .is_some_and(|members| members.iter().any(|m| m.principal == me));
        (!info.channel.admits(&me, seated)).then_some(Gate::NotMember)
    }

    pub(crate) fn may_write(&self) -> bool {
        self.write_gate().is_none()
    }
}
