//! The marker a view names this program by in `rpc.query`/`op.submit`.
use ducktape_view_guest::doors::Program;

pub struct Registry;
impl Program for Registry {
    const NAME: &'static str = crate::PROGRAM;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}
