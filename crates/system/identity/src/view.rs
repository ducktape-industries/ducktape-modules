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

/// What an account is, as a view labels it: "Person", "Agent · managed by
/// eddy", "Module · chat", and its status where it does not act. `accounts`
/// names the manager.
pub fn kind(account: &crate::Account, accounts: &[crate::Account]) -> String {
    use crate::{Category, Status};
    let what = match (&account.module, account.manager) {
        (Some(module), _) => format!("Module · {module}"),
        (None, Some(manager)) => {
            let label = match account.category {
                Some(Category::Agent) => "Agent",
                None => "Managed",
            };
            let by = accounts
                .iter()
                .find(|other| other.number == manager)
                .map_or_else(|| format!("#{manager}"), |other| other.name.clone());
            format!("{label} · managed by {by}")
        }
        (None, None) => "Person".into(),
    };
    match account.status {
        Status::Active => what,
        Status::Suspended => format!("{what} · suspended"),
        Status::Revoked => format!("{what} · revoked"),
    }
}
