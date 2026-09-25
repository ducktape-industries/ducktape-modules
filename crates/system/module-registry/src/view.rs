//! The marker a view names this program by in `program.query`/`op.submit`.
use ducktape_view_guest::methods::Program;

pub struct Registry;
impl Program for Registry {
    const NAME: &'static str = crate::MODULE;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}
