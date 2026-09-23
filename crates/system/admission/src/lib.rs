//! The `admission` program: the valset's one writer. It enrolls a signer as a
//! resident (by invite while the door is closed), lets a member leave, and
//! tallies validators' votes on members and on the door. The `program`
//! feature adds the wasm32 program over the host (`program.rs`).
#[cfg(feature = "program")]
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use rules::execute;

pub use abi::admission::{Grant, INVITE_NAMESPACE, Invite, Motion, Op, Voted};
pub use module_registry::ADMISSION as PROGRAM;
