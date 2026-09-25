//! What chat follows while it is open: the session, routes opened into
//! it, whether it is on screen, and the live heads of chat and identity.
//! Every follower says what a refusal means to it; none ends on one.
use std::future::Future;

use ducktape_view_guest::Context;
use ducktape_view_guest::host::Refusal;

use crate::api::{HostProps, HostRoute, HostVisible, RpcLive};
use crate::{Chat, links};

impl Chat {
    /// Subscribes every follower; the ones before are dropped with them.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let props = host.subscribe::<HostProps>(());
        let changes = host.subscribe::<RpcLive>(chat::PROGRAM.into());
        let routes = host.subscribe::<HostRoute>(());
        let visible = host.subscribe::<HostVisible>(());
        let identity = host.subscribe::<RpcLive>(identity::PROGRAM.into());
        self.followers = vec![
            cx.follow(props, |chat, props, _, cx| match props {
                Ok(next) => chat.session_changed(next, cx),
                Err(refusal) => {
                    chat.notice = format!("Couldn’t read the session: {}", refusal.sentence)
                }
            }),
            cx.follow(changes, |chat, head, _, cx| match head {
                Ok(_) => chat.refresh(cx),
                Err(refusal) => log(cx, "chat's live heads", &refusal),
            }),
            // `duck://<chain>/chat/<channel>[/<seq>]`: a link opened into
            // this view (a notice's, say) names the room and the message
            cx.follow(routes, |chat, route, window, cx| match route {
                Ok(route) => {
                    if let Some((channel, seq)) = links::route_target(&route) {
                        chat.search_clear();
                        chat.open_at(channel, seq, window, cx);
                        chat.settle_badge(cx);
                    }
                }
                Err(refusal) => log(cx, "the route", &refusal),
            }),
            cx.follow(visible, |chat, visible, _, cx| match visible {
                Ok(visible) => chat.visibility_changed(visible, cx),
                Err(refusal) => log(cx, "visibility", &refusal),
            }),
            // a key that gains an account while this view is open (Settings,
            // then back to Chat) writes an identity block, not a session
            // change: re-resolve the reader on it, and re-read the roster so
            // a name another signer claims replaces its "account N" fallback
            cx.follow(identity, |chat, head, _, cx| match head {
                Ok(_) => {
                    chat.load_names(cx);
                    chat.refresh_me(cx);
                }
                Err(refusal) => log(cx, "identity's live heads", &refusal),
            }),
        ];
    }

    /// Re-reads what is already on screen: the rows there stay until the
    /// fresh ones land, and stay if the read is refused (the host's log
    /// keeps why).
    pub(crate) fn reread<T: 'static>(
        &self,
        what: &'static str,
        work: impl Future<Output = Result<T, Refusal>> + 'static,
        land: impl FnOnce(&mut Chat, T, &mut Context<Chat>) + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |chat, cx| {
                match result {
                    Ok(value) => land(chat, value, cx),
                    Err(refusal) => log(cx, what, &refusal),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
pub(crate) fn log(cx: &mut Context<Chat>, what: &str, refusal: &Refusal) {
    cx.host().log(format!("chat: {what} refused: {refusal}"));
}
