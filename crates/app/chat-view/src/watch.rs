//! What chat follows while it is open: the session, routes opened into
//! it, whether it is on screen, and the live heads of chat and identity.
//! Every follower says what a refusal means to it; none ends on one.
use ducktape_view_guest::Context;

use crate::api::{Changes, ChatApi, HostRoute, HostSession, HostVisible};
use crate::{Chat, links};
use ::chat::view::Identity;

impl Chat {
    /// Subscribes every follower; the ones before are dropped with them.
    pub(crate) fn watch(&mut self, cx: &mut Context<Self>) {
        let host = cx.host();
        let props = host.subscribe::<HostSession>(());
        let changes = host.subscribe::<Changes<ChatApi>>(());
        let routes = host.subscribe::<HostRoute>(());
        let visible = host.subscribe::<HostVisible>(());
        let identity = host.subscribe::<Changes<Identity>>(());
        self.followers = vec![
            cx.for_each(props, |chat, props, _, cx| match props {
                Ok(next) => chat.session_changed(next, cx),
                Err(refusal) => {
                    chat.notice = format!("Couldn’t read the session: {}", refusal.message)
                }
            }),
            cx.for_each(changes, |chat, head, _, cx| match head {
                Ok(_) => chat.refresh(cx),
                Err(refusal) => cx.host().log_refused("chat", "chat's live heads", &refusal),
            }),
            // `duck://<chain>/chat/<channel>[/<seq>]`: a link opened into
            // this view (a notice's, say) names the room and the message
            cx.for_each(routes, |chat, route, window, cx| match route {
                Ok(route) => {
                    if let Some((channel, seq)) = links::route_target(&route) {
                        chat.search_clear();
                        chat.open_at(channel, seq, window, cx);
                        chat.settle_badge(cx);
                    }
                }
                Err(refusal) => cx.host().log_refused("chat", "the route", &refusal),
            }),
            cx.for_each(visible, |chat, visible, _, cx| match visible {
                Ok(visible) => chat.visibility_changed(visible, cx),
                Err(refusal) => cx.host().log_refused("chat", "visibility", &refusal),
            }),
            // re-read the roster on identity's heads, so a name another
            // signer claims replaces its "account N" fallback
            cx.for_each(identity, |chat, head, _, cx| match head {
                Ok(_) => chat.load_names(cx),
                Err(refusal) => cx
                    .host()
                    .log_refused("chat", "identity's live heads", &refusal),
            }),
        ];
    }
}
