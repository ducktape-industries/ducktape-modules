//! Forge: repositories, code, commits, refs and the Changes a reviewer
//! lives in, on the view-guest `View` shape.
//!
//! The forge program answers everything this screen shows, in borsh, through
//! one door (`rpc.query_bytes`); conversation is chat's, through the door
//! chat-view uses. Reads are a cache keyed by the query itself: `sync` asks
//! what the current screen needs, issues what is missing, and drops what the
//! reader has navigated away from. `render` never mutates — what an event
//! changes lands in `actions`.
//!
//! - `state`: what the view holds; `select`: what the screens read out of it.
//! - `sync`: which reads the screen needs; `queries`: how one read is asked.
//! - `navigate`, `actions`, `review`, `tree`: what an event changes.
//! - `ui`: the screens; `ui::markdown`: documents and their links.
mod api;
mod queries;
mod select;
mod state;
mod sync;
mod tree;

mod actions;
mod navigate;
mod review;
mod ui;

use ducktape_view_guest::doors::{HostRoute, HostVisible, RpcLive};
use ducktape_view_guest::{Context, IntoElement, Render, View, Window, export_view};

use api::HostProps;
pub(crate) use select::Stage;
pub use state::Forge;

impl View for Forge {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180,760";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut forge = Self::default();
        forge.restored(window, cx);
        forge
    }

    /// Every stream this view follows, restarted after a snapshot. A refused
    /// item says so in the notice; none ends its stream.
    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watches.clear();
        let props = cx.host().subscribe::<HostProps>(());
        self.watches.push(cx.follow(props, |forge, item, _, cx| {
            match item {
                Ok(session) => forge.session_changed(session, cx),
                Err(refusal) => {
                    forge.notice = format!("Couldn’t read the session: {}", refusal.sentence)
                }
            }
            cx.notify();
        }));
        // `duck://<chain>/forge/<name>`: a link opened into this view names
        // the repository to open
        let routes = cx.host().subscribe::<HostRoute>(());
        self.watches
            .push(cx.follow(routes, |forge, route, _, cx| match route {
                Ok(route) => forge.open_route(&route, cx),
                Err(refusal) => {
                    forge.notice = format!("Couldn’t follow the link: {}", refusal.sentence);
                    cx.notify();
                }
            }));
        // identity's own block matters too: a key that gains an account
        // while this view is open (Settings, then back to Forge) writes no
        // session change of its own, only an identity block. A refused item
        // is a block this view cannot see into; the next one reconciles.
        for module in [forge::PROGRAM, chat::PROGRAM, identity::PROGRAM] {
            let live = cx.host().subscribe::<RpcLive>(module.into());
            self.watches
                .push(cx.follow(live, |forge, _, _, cx| forge.reconcile(cx)));
        }
        let visible = cx.host().subscribe::<HostVisible>(());
        self.watches.push(cx.follow(visible, |forge, shown, _, cx| {
            if shown.unwrap_or(false) {
                forge.refresh(cx);
            }
        }));
        if self.names.is_idle() {
            self.names = cx.load(queries::roster(cx.host()), |forge| &mut forge.names);
        }
        if self.me.is_idle() {
            self.refresh_me(cx);
        }
        self.sync(cx);
    }
}

impl Render for Forge {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

export_view!(
    Forge,
    "Forge",
    "Repositories, code, commits and the changes waiting on your judgment.",
    ["rpc", "op", "host", "clipboard"]
);

#[cfg(test)]
mod tests;
