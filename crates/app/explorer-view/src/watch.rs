//! What Explorer follows while it is open: the chain's heads, the session,
//! links opened into it, and the live heads of the programs whose lists it
//! shows. Every follower says what a refusal means to it; none ends on one.
use ducktape_view_guest::Context;
use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::{ChainHeads, Changes, HostRoute, HostSession};
use futures::StreamExt;
use identity::view::Identity;
use module_registry::view::Registry;
use valset::view::Valset;

use crate::Explorer;

impl Explorer {
    /// Subscribes every follower; the ones before are dropped with them.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        // `None` once the stream ends: the head is polled from then on
        let heads = host
            .subscribe::<ChainHeads>(())
            .map(Some)
            .chain(futures::stream::iter([None]));
        let session = host.subscribe::<HostSession>(());
        let routes = host.subscribe::<HostRoute>(());
        let identity = host.subscribe::<Changes<Identity>>(());
        let valset = host.subscribe::<Changes<Valset>>(());
        let registry = host.subscribe::<Changes<Registry>>(());
        self.followers = vec![
            cx.for_each(heads, |view, head, _, cx| match head {
                Some(Ok(head)) => view.at_head(head, cx),
                Some(Err(refusal)) => {
                    log(cx, "the chain's heads", &refusal);
                    view.poll_head(cx);
                }
                None => view.poll_head(cx),
            }),
            cx.for_each(session, |view, session, _, cx| match session {
                Ok(session) => view.session_chain = session.chain_id,
                Err(refusal) => log(cx, "the session", &refusal),
            }),
            // `duck://<chain>/explorer/<route>`
            cx.for_each(routes, |view, route, _, cx| match route {
                Ok(route) => view.open_route(&route, cx),
                Err(refusal) => log(cx, "the route", &refusal),
            }),
            // an account made, renamed or re-keyed, by anyone: a module's
            // `RegisterModule` included, which no transaction targets
            cx.for_each(identity, |view, head, _, cx| match head {
                Ok(_) => view.read_accounts(cx),
                Err(refusal) => log(cx, "identity's live heads", &refusal),
            }),
            cx.for_each(valset, |view, head, _, cx| match head {
                Ok(_) => view.read_validators(cx),
                Err(refusal) => log(cx, "valset's live heads", &refusal),
            }),
            cx.for_each(registry, |view, head, _, cx| match head {
                Ok(_) => view.read_network(cx),
                Err(refusal) => log(cx, "the registry's live heads", &refusal),
            }),
        ];
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
pub(crate) fn log(cx: &mut Context<Explorer>, what: &str, refusal: &Error) {
    cx.host()
        .log(format!("explorer: {what} refused: {refusal}"));
}
