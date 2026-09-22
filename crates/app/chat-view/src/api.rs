//! What this view says to the host: the chat program by the doors in
//! `view_wire::doors`, and the host's session facts.
use crate::chat::{ChatMsg, ChatViewQuery, ChatViewReply};
use ducktape_view_guest::doors::Program;
pub use ducktape_view_guest::doors::{
    ClipboardRead, ClipboardWrite, Drops, Id, Live, Pick, Props, Release, SelectedFile, Session,
    Submit, Visible,
};

pub struct ChatApi;
impl Program for ChatApi {
    const NAME: &'static str = "chat";
    type Op = ChatMsg;
    type Query = ChatViewQuery;
    type Reply = ChatViewReply;
}

/// Every write in chat is authored by an account: a key that holds none
/// reads and nothing more.
pub fn holds_account(session: &Session) -> bool {
    session.account.starts_with("acct:")
}
