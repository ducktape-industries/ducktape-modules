//! What Settings follows while it is open: the session, the live heads of
//! identity and valset, and the clock the node status is re-read on.
//! Every follower says what a refusal means to it; none ends on one.
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::{Changes, ClockTicks};
use ducktape_view_guest::view::Loadable;

use crate::Settings;
use crate::api::{HostSession, Identity, Valset};

/// How often the node status is re-read, in milliseconds.
const STATUS_TICK: i64 = 1_000;

impl Settings {
    /// Subscribes every follower; the ones before are dropped with them.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let session = host.subscribe::<HostSession>(());
        let identity = host.subscribe::<Changes<Identity>>(());
        let valset = host.subscribe::<Changes<Valset>>(());
        let ticks = host.subscribe::<ClockTicks>(STATUS_TICK);
        self.followers = vec![
            cx.for_each(session, |view, session, _, cx| match session {
                Ok(session) => view.session_changed(session, cx),
                Err(refusal) => view.account = Loadable::Failed(refusal),
            }),
            // an account, key or agent changed: the reader's may be among them
            cx.for_each(identity, |view, head, _, cx| match head {
                Ok(_) => view.refresh_account(cx),
                Err(refusal) => log(cx, "identity's live heads", &refusal),
            }),
            // a key started or stopped validating
            cx.for_each(valset, |view, head, _, cx| match head {
                Ok(_) => view.refresh_account(cx),
                Err(refusal) => log(cx, "valset's live heads", &refusal),
            }),
            cx.for_each(ticks, |view, tick, _, cx| match tick {
                Ok(()) => view.read_status(cx),
                Err(refusal) => log(cx, "the clock", &refusal),
            }),
        ];
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
pub(crate) fn log(cx: &mut Context<Settings>, what: &str, refusal: &Error) {
    cx.host()
        .log(format!("settings: {what} refused: {refusal}"));
}
