//! What this view says to the host: the chat program by the doors in
//! `view_wire::doors`, and the host's session facts.
pub use ducktape_view_guest::doors::{
    ClipboardRead, ClipboardWrite, Drops, Id, Live, Pick, Props, Release, Route, SelectedFile,
    Session, Submit, Visible,
};

/// The chat program, by its own marker (named apart from this view's `Chat`).
pub use ::chat::view::Chat as ChatApi;
