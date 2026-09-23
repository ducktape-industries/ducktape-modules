//! The programs this view speaks to, by the doors in `view_wire::doors`:
//! forge (its own) and chat (the discussion threads).
use ducktape_view_guest::doors::{Program, Query, Submit};

pub use ducktape_view_guest::doors::{Props, Session};

pub struct ForgeProgram;
impl Program for ForgeProgram {
    const NAME: &'static str = "forge";
    type Op = forge::Op;
    type Query = forge::Query;
    type Reply = forge::Reply;
}
pub type Ask = Query<ForgeProgram>;
pub type SubmitForge = Submit<ForgeProgram>;

pub struct ChatApi;
impl Program for ChatApi {
    const NAME: &'static str = "chat";
    type Op = chat::ChatMsg;
    type Query = chat::ChatViewQuery;
    type Reply = chat::ChatViewReply;
}
