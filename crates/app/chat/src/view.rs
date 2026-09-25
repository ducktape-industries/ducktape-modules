//! The marker a view names this program by in `rpc.query`/`op.submit`.
use ducktape_view_guest::doors::Program;

pub struct Chat;
impl Program for Chat {
    const NAME: &'static str = crate::PROGRAM;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}
