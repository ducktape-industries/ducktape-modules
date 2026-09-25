//! The marker a view names identity by in `program.query`/`op.submit`/`program.changes`.
//! Who the reader is comes with the session (`Session.account`); no view
//! asks identity for it.
use ducktape_view_guest::methods::Program;

use crate::{Op, Query, Reply};

pub struct Identity;
impl Program for Identity {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
}
