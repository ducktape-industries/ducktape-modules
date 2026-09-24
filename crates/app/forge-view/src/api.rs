//! The programs this view speaks to, by the doors in `view_wire::doors`:
//! forge (its own) and chat (the discussion threads).
use ducktape_view_guest::doors::{Query, Submit};

pub use ducktape_view_guest::doors::{Props, Session};

/// The forge and chat programs, by their own markers (named apart from
/// this view's `Forge` and the chat module's `Chat`).
pub use chat::view::Chat as ChatApi;
pub use forge::view::Forge as ForgeProgram;
pub type Ask = Query<ForgeProgram>;
pub type SubmitForge = Submit<ForgeProgram>;
