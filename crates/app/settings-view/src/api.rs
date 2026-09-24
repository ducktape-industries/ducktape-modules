//! The programs this view reads, by the doors in `view_wire::doors`.
use ducktape_view_guest::doors::Program;

pub use ducktape_view_guest::doors::{
    Invite as MintInvite, Mint, Minted as Invite, NodeStatus as Status, Props, Session,
    Status as NodeStatus, Submit,
};

pub struct Identity;
impl Program for Identity {
    const NAME: &'static str = identity::PROGRAM;
    type Op = identity::Op;
    type Query = identity::Query;
    type Reply = identity::Reply;
}
pub struct Valset;
impl Program for Valset {
    const NAME: &'static str = valset::PROGRAM;
    type Op = ();
    type Query = valset::Query;
    type Reply = valset::Reply;
}
