//! The `module-registry` program: the roster of programs a network runs, and
//! the scheduled changes to it. The root of the boot set: `valset` and
//! `identity` link this crate for the paging ([`Page`]) and origin
//! ([`helpers`]) conventions every system program shares, and for the
//! authority every system program takes its governance ops from.
//!
//! The types are always built; a view links them with `program` off. The
//! `program` feature adds the wasm32 program over the host (`program.rs`).
pub mod helpers;
mod page;
#[cfg(feature = "program")]
mod program;

pub use page::{Page, PageReply};

/// The program whose frames `valset` and this registry accept as governance.
pub const AUTHORITY: &str = "governance";

use abi::ProgramId;
use borsh::{BorshDeserialize, BorshSerialize};

pub use abi::module_registry::{Entry, Genesis, PROGRAM};

pub const CODE_KIND: &str = "program";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Change {
    Set(Entry),
    Remove(ProgramId),
}

impl Change {
    pub fn program(&self) -> &str {
        match self {
            Change::Set(entry) => &entry.program,
            Change::Remove(program) => program,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Scheduled {
    pub height: u64,
    pub change: Change,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Publish { body: Vec<u8> },
    Schedule(Scheduled),
    Cancel { height: u64, program: ProgramId },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    At(u64),
    Scheduled { page: Page },
    Program(ProgramId),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Programs(Vec<Entry>),
    Scheduled(PageReply<Scheduled>),
    Program { height: u64, entry: Option<Entry> },
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_host_contract_is_a_prefix_of_the_program_contract() {
        assert_eq!(
            abi::encode(&abi::module_registry::Query::At(9)),
            abi::encode(&super::Query::At(9))
        );
        let entry = abi::module_registry::Entry {
            program: "p".into(),
            code: abi::BlobId::Sha256([1; 32]),
            params: vec![2],
        };
        assert_eq!(
            abi::encode(&abi::module_registry::Reply::Programs(vec![entry.clone()])),
            abi::encode(&super::Reply::Programs(vec![entry]))
        );
    }
}
