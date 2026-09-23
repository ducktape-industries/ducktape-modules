//! The `module-registry` program: the roster of programs a network runs, and
//! the scheduled changes to it. The root of the boot set: `valset` and
//! `identity` link this crate for the origin ([`helpers`]) conventions every
//! system program shares, and for the authority every system program takes
//! its governance ops from.
//!
//! The types and rules are always built; a view links them with `program`
//! off. The `program` feature adds the wasm32 program over the host
//! (`program.rs`).
pub mod helpers;
#[cfg(feature = "program")]
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use rules::{execute, init, query};
pub use store::{Page, PageReply};

/// The program whose frames this registry accepts as governance.
pub const AUTHORITY: &str = "governance";
/// The program whose frames `valset` accepts: the one that enrolls a joiner.
pub const ADMISSION: &str = "admission";

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
