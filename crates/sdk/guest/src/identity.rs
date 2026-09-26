//! What the identity role says about an account a module is asked to name.

use abi::role::identity::{Kind, Query, Reply, Standing};

use crate::{AccountNumber, Error, QueryCtx, code, invalid, wrong_state};

impl QueryCtx {
    /// Refused unless the identity role profiles `number` as a person or
    /// an agent that acts: no absent account, no module's, no managed
    /// account suspended or revoked.
    pub fn require_person_or_agent(&self, number: AccountNumber) -> Result<(), Error> {
        let asked = Query::Profile(number);
        let Reply::Profile(profile) =
            self.ask::<Query, Reply>(&self.env().roles.identity, &asked)?
        else {
            return Err(Error::new(
                code::UNEXPECTED_REPLY,
                "identity answered Profile with something else",
            ));
        };
        let Some(profile) = profile else {
            return Err(invalid(format!("there is no account {number}")));
        };
        match profile.kind {
            Kind::Person
            | Kind::Managed {
                standing: Standing::Active,
                ..
            } => Ok(()),
            Kind::Managed {
                standing: Standing::Suspended,
                ..
            } => Err(wrong_state(format!(
                "account {number} is suspended: only people and agents that act are named"
            ))),
            Kind::Managed {
                standing: Standing::Revoked,
                ..
            } => Err(wrong_state(format!(
                "account {number} is revoked: only people and agents that act are named"
            ))),
            Kind::Module(module) => Err(invalid(format!(
                "account {number} is module {module}'s: only people and agents are named"
            ))),
        }
    }
}
