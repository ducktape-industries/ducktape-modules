//! The marker a view names this program by in `module.query`/`op.submit`.
use ducktape_view_guest::methods::Module;

pub struct Registry;
impl Module for Registry {
    const NAME: &'static str = crate::MODULE;
    type Op = crate::Op;
    type Query = crate::Query;
    type Reply = crate::Reply;
}
