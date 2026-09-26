//! Chat: channels, direct messages, threads and search.
//!
//! The view's state is one struct per pane (`state.rs`). Rows come from the
//! `chat` program through its own types (`queries.rs`), are folded with the
//! name directory into what a frame draws (`message.rs`, `names.rs`) at
//! render time, and `render` (`ui/`) never mutates: what an event changes
//! lands in the handlers of `room`, `actions`, `search` and `compose`. The
//! rich editor is the SDK's composer; `composer.rs` is its whole boundary.
mod actions;
mod api;
mod compose;
mod composer;
mod emoji;
mod kept;
mod links;
mod message;
mod names;
mod notices;
mod queries;
mod room;
mod search;
mod session;
mod state;
mod ui;
mod watch;

pub use state::*;

use ducktape_view_guest::view::View;
use ducktape_view_guest::{Context, IntoElement, Render, Window, export_view};

/// Rows asked per page.
const PAGE: usize = 64;
/// Rows a room keeps on screen before paging older.
const WINDOW: usize = 256;

impl View for Chat {
    const PREFERRED_WINDOW_SIZE: &'static str = "1180,760";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut chat = Self::default();
        chat.restored(window, cx);
        chat
    }

    /// Follows the host again and re-reads what the snapshot showed.
    fn restored(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // a send in flight when the snapshot was taken never came back:
        // park its body as a failed send the composer can restore
        for draft in self.drafts.values_mut() {
            draft.retire_device_requests();
        }
        self.menu = None;
        self.watch(cx);
        if self.names.is_idle() {
            self.load_names(cx);
        }
        if self.channels.is_idle() {
            self.load_channels(cx);
        }
        if let Some(room) = &self.room {
            let (id, thread) = (room.id.clone(), room.thread.as_ref().map(|t| t.root));
            self.open(id, window, cx);
            if let Some(root) = thread {
                self.open_thread(root, cx);
            }
        }
        if !self.search.query.is_empty() {
            self.search_now(cx);
        }
    }
}

impl Render for Chat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, cx)
    }
}

export_view!(
    Chat,
    "Chat",
    "Channels, direct messages, threads, search and the live call of this workspace.",
    [
        "module",
        "op",
        "host",
        "link",
        "clipboard",
        "notify",
        "store"
    ]
);

#[cfg(test)]
mod tests;
