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
    /// What the change does, in words: `Set`, `Remove`, `Set view`, `Remove view`.
    pub fn verb(&self) -> &'static str {
        match self {
            Change::Set(_) => "Set",
            Change::Remove(_) => "Remove",
            Change::SetView(_) => "Set view",
            Change::RemoveView(_) => "Remove view",
        }
    }

    /// The code a set lands; a removal has none.
    pub fn code(&self) -> Option<abi::BlobId> {
        match self {
            Change::Set(entry) => Some(entry.code),
            Change::SetView(view) => Some(view.view),
            Change::Remove(_) | Change::RemoveView(_) => None,
        }
    }

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

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let height = |height: &u64| field("height", Value::Text(height.to_string()));
    let (title, fields) = match op {
        Op::Publish { body } => ("Publish".into(), vec![field("body", Value::bytes(body))]),
        Op::Schedule(Scheduled { height: at, change }) => {
            let mut fields = vec![
                field("change", Value::text(change.verb())),
                field("program", Value::Program(change.program().into())),
                height(at),
            ];
            fields.extend(
                change
                    .code()
                    .map(|code| field("code", Value::Hash(code.digest().to_vec()))),
            );
            if let Change::Set(entry) = change {
                fields.push(field("params", Value::bytes(&entry.params)));
            }
            (format!("Schedule · {}", change.program()), fields)
        }
        Op::Cancel {
            height: at,
            program,
        } => (
            format!("Cancel · {program}"),
            vec![
                field("program", Value::Program(program.clone())),
                height(at),
            ],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Publish", "Schedule", "Cancel",]
    );
}
