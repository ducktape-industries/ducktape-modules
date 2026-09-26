//! The marker a view names identity by in `module.query`/`op.submit`/`module.changes`.
//! Who the reader is comes with the session (`Session.account`); no view
//! asks identity for it.
use ducktape_view_guest::methods::Module;

use crate::{Op, Query, Reply};

pub struct Identity;
impl Module for Identity {
    const NAME: &'static str = crate::MODULE;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
}
