//! The marker a view names this module by in `program.query`/`op.submit`.
use ducktape_view_guest::methods::Program;

pub struct Valset;
impl Program for Valset {
    const NAME: &'static str = crate::MODULE;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}
