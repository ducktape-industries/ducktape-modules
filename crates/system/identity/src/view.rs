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

/// What an account is, as members and explorer label it: "Person",
/// "Agent · managed by eddy · suspended", "Module · chat". The badge and
/// the note are [`Kind`](crate::Kind)'s; `name_of` names the manager, or
/// its number stands in.
pub fn kind(
    kind: &crate::Kind,
    name_of: impl FnOnce(crate::AccountNumber) -> Option<String>,
) -> String {
    let what = kind
        .badge(|manager| name_of(manager).unwrap_or_else(|| format!("#{manager}")))
        .unwrap_or_else(|| "Person".into());
    match kind.note() {
        Some(note) => format!("{what} · {note}"),
        None => what,
    }
}
