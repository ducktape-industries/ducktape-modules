//! Settings: the node this app talks to, who the seated key is (its
//! account, its keys, the agents it manages), invites to the network, and
//! the app's own preferences.
//!
//! The state is one struct (`state.rs`). Reads are typed asks of identity
//! and valset (`queries.rs`), re-read on their live heads (`watch.rs`);
//! presses land in `actions.rs`; `ui/` draws and never mutates.
mod actions;
mod api;
mod queries;
mod state;
mod ui;
mod watch;

pub use state::Settings;

use ducktape_view_guest::{Context, IntoElement, Render, View, Window, export_view};

impl View for Settings {
    const PREFERRED_WINDOW_SIZE: &'static str = "820,940";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            // an invite lasts a week unless the reader picks otherwise
            ttl: 1,
            ..Self::default()
        };
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx);
        self.read_status(cx);
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

export_view!(
    Settings,
    "Settings",
    "Node, account, invites and app preferences.",
    [
        "chain",
        "module",
        "op",
        "invite",
        "host",
        "clock",
        "clipboard"
    ]
);

#[cfg(test)]
mod tests;
