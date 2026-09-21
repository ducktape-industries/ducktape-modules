//! The `admission` program: the valset's one writer. `Enroll` from any signed
//! frame seats the signer as a resident at the address it names; a key that
//! already holds a standing keeps it and only its address moves. The invite
//! check lands here later. The types and rules are always built; the
//! `program` feature adds the wasm32 program over the host (`program.rs`).
#[cfg(feature = "program")]
mod program;
mod rules;

pub use rules::execute;

use borsh::{BorshDeserialize, BorshSerialize};

pub const PROGRAM: &str = "admission";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Enroll { address: String },
}
