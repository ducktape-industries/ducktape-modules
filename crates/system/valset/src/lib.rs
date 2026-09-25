//! The `valset` module: who validates and who resides on a network. The
//! types, rules and [`Valset`] module are always built; a view links them
//! with `module` off. The `module` feature adds its wasm exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;
#[cfg(feature = "view")]
pub mod view;

pub use program::Valset;

use borsh::{BorshDeserialize, BorshSerialize};
use module_registry::{PageRequest, PageResponse};

pub use abi::valset::{Genesis, Member, PROGRAM as MODULE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Role {
    Validator,
    Resident,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Membership {
    pub key: Vec<u8>,
    pub address: String,
    pub role: Role,
}

impl Membership {
    pub fn member(&self) -> Member {
        Member {
            key: self.key.clone(),
            address: self.address.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Set(Membership),
    Remove { key: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    Validators,
    Members,
    Memberships { page: PageRequest },
    Membership { key: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Validators(Vec<Vec<u8>>),
    Members(Vec<Member>),
    Memberships(PageResponse<Membership>),
    Membership(Option<Membership>),
}

/// The ask another module makes of valset.
pub fn role(ctx: &guest::QueryCtx, key: &[u8]) -> Result<Option<Role>, guest::Error> {
    match ctx.ask::<Query, Reply>(MODULE, &Query::Membership { key: key.to_vec() })? {
        Reply::Membership(membership) => Ok(membership.map(|membership| membership.role)),
        other => Err(guest::Error::new(
            guest::code::UNEXPECTED_REPLY,
            format!("valset answered Membership with {other:?}"),
        )),
    }
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this module ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Set(membership) => (
            format!("Set · {}", membership.address),
            vec![
                field("key", Value::Key(membership.key.clone())),
                field("address", Value::text(&membership.address)),
                field(
                    "role",
                    Value::text(match membership.role {
                        Role::Validator => "validator",
                        Role::Resident => "resident",
                    }),
                ),
            ],
        ),
        Op::Remove { key } => ("Remove".into(), vec![field("key", Value::Key(key.clone()))]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Set", "Remove",]);
}
