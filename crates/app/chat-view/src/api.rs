//! What this view says to the host: the chat program by the doors in
//! `view_wire::doors`, and the host's session facts.
pub use ducktape_view_guest::doors::{
    ClipboardRead, ClipboardWrite, Id, Live, Props, Route, Session, Submit, Visible,
};

/// The chat program, by its own marker (named apart from this view's `Chat`).
pub use ::chat::view::Chat as ChatApi;
