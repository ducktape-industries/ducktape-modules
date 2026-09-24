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
#[cfg(feature = "view")]
pub mod view;

pub use rules::{execute, init, query};
pub use store::{Page, PageReply};

/// The program whose frames `valset` and this registry accept as governance.
pub const AUTHORITY: &str = "governance";

use abi::ProgramId;
use borsh::{BorshDeserialize, BorshSerialize};

pub use abi::module_registry::{Entry, Genesis, PROGRAM, View};

pub const CODE_KIND: &str = "program";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Change {
    Set(Entry),
    Remove(ProgramId),
    /// A view with no program behind it, listed under its name.
    SetView(View),
    RemoveView(ProgramId),
}

impl Change {
    pub fn program(&self) -> &str {
        match self {
            Change::Set(entry) => &entry.program,
            Change::SetView(view) => &view.name,
            Change::Remove(program) | Change::RemoveView(program) => program,
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
    Scheduled {
        page: Page,
    },
    Program(ProgramId),
    /// The view-only entries at a height, by name.
    Views(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Programs(Vec<Entry>),
    Scheduled(PageReply<Scheduled>),
    Program { height: u64, entry: Option<Entry> },
    Views(Vec<View>),
}

/// An op as a person reads it: a title and its fields.
pub fn describe(op: &Op) -> (String, Vec<(&'static str, String)>) {
    match op {
        Op::Publish { body } => ("Publish".into(), vec![("body", abi::preview(body))]),
        Op::Schedule(Scheduled { height, change }) => {
            let (verb, code) = match change {
                Change::Set(entry) => ("Set", Some(entry.code)),
                Change::Remove(_) => ("Remove", None),
                Change::SetView(view) => ("Set view", Some(view.view)),
                Change::RemoveView(_) => ("Remove view", None),
            };
            let mut fields = vec![
                ("change", verb.to_string()),
                ("program", change.program().to_string()),
                ("height", height.to_string()),
            ];
            fields.extend(code.map(|code| ("code", abi::hex(code.digest()))));
            if let Change::Set(entry) = change {
                fields.push(("params", abi::preview(&entry.params)));
            }
            (format!("Schedule · {}", change.program()), fields)
        }
        Op::Cancel { height, program } => (
            format!("Cancel · {program}"),
            vec![("program", program.clone()), ("height", height.to_string())],
        ),
    }
}
