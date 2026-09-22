//! The `valset` program: who validates and who resides on a network. The
//! types and rules are always built; a view links them with `program` off.
//! The `program` feature adds the wasm32 program over the host (`program.rs`).
#[cfg(feature = "program")]
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use rules::{execute, init, query};

use borsh::{BorshDeserialize, BorshSerialize};
use module_registry::{Page, PageReply};

pub use abi::valset::{Genesis, Member, PROGRAM};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Standing {
    Validator,
    Resident,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Membership {
    pub key: Vec<u8>,
    pub address: String,
    pub standing: Standing,
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
    Memberships { page: Page },
    Membership { key: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Validators(Vec<Vec<u8>>),
    Members(Vec<Member>),
    Memberships(PageReply<Membership>),
    Membership(Option<Membership>),
}

/// The ask another program makes of valset.
pub fn standing(ctx: &impl store::Reads, key: &[u8]) -> Result<Option<Standing>, abi::Refusal> {
    match ctx.ask::<Query, Reply>(PROGRAM, &Query::Membership { key: key.to_vec() })? {
        Reply::Membership(membership) => Ok(membership.map(|membership| membership.standing)),
        other => Err(abi::Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            format!("valset answered Membership with {other:?}"),
        )),
    }
}
