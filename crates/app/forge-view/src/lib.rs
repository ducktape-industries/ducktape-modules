//! Forge: repositories, code, commits, refs and the Changes a reviewer
//! lives in, on the view-guest `View` shape.
//!
//! The forge program answers everything this screen shows, in borsh, through
//! one method (`module.query`); conversation is chat's, through the method
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

use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::{Changes, HostRoute, HostVisible};
use ducktape_view_guest::{Context, IntoElement, Render, View, Window, export_view};

use api::{ChatApi, ForgeProgram, HostSession};
use chat::view::Identity;
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
        let props = cx.host().subscribe::<HostSession>(());
        self.watches.push(cx.for_each(props, |forge, item, _, cx| {
            match item {
                Ok(session) => forge.session_changed(session, cx),
                Err(refusal) => {
                    forge.notice = format!("Couldn’t read the session: {}", refusal.message)
                }
            }
            cx.notify();
        }));
        // `duck://<chain>/forge/<name>`: a link opened into this view names
        // the repository to open
        let routes = cx.host().subscribe::<HostRoute>(());
        self.watches
            .push(cx.for_each(routes, |forge, route, _, cx| match route {
                Ok(route) => forge.open_route(&route, cx),
                Err(refusal) => {
                    forge.notice = format!("Couldn’t follow the link: {}", refusal.message);
                    cx.notify();
                }
            }));
        // a block to any program the screens read re-reads them; a refused
        // item is a block this view cannot see into: the host's log keeps
        // why, and the next block reconciles
        let forge = cx.host().subscribe::<Changes<ForgeProgram>>(());
        let chat = cx.host().subscribe::<Changes<ChatApi>>(());
        let identity = cx.host().subscribe::<Changes<Identity>>(());
        self.watches.extend([
            cx.for_each(forge, |forge, head, _, cx| match head {
                Ok(_) => forge.reconcile(cx),
                Err(refusal) => log(cx, "forge's live heads", &refusal),
            }),
            cx.for_each(chat, |forge, head, _, cx| match head {
                Ok(_) => forge.reconcile(cx),
                Err(refusal) => log(cx, "chat's live heads", &refusal),
            }),
            cx.for_each(identity, |forge, head, _, cx| match head {
                Ok(_) => forge.reconcile(cx),
                Err(refusal) => log(cx, "identity's live heads", &refusal),
            }),
        ]);
        let visible = cx.host().subscribe::<HostVisible>(());
        self.watches
            .push(cx.for_each(visible, |forge, shown, _, cx| match shown {
                Ok(true) => forge.refresh(cx),
                Ok(false) => {}
                Err(refusal) => log(cx, "visibility", &refusal),
            }));
        if self.names.is_idle() {
            self.names = cx.load(chat::view::roster(cx.host()), |forge| &mut forge.names);
        }
        self.sync(cx);
    }
}

/// A refusal nothing on screen waits for, kept in the host's log.
fn log(cx: &mut Context<Forge>, what: &str, refusal: &Error) {
    cx.host().log(format!("forge: {what} refused: {refusal}"));
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
    ["module", "op", "host", "link", "clipboard"]
);

#[cfg(test)]
mod tests;
