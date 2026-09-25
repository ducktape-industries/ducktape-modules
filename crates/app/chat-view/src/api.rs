//! What this view says to the host: the chat program by the doors in
//! `view_wire::doors`, and the host's session facts.
pub use ducktape_view_guest::doors::{
    ClipboardRead, ClipboardWrite, HostId, HostProps, HostRoute, HostVisible, Live, Query as Ask,
    Session, Submit,
};

/// The chat program, by its own marker (named apart from this view's `Chat`).
pub use ::chat::view::Chat as ChatApi;
