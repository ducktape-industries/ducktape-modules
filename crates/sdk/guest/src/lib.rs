//! The minimal module SDK. A module is a type implementing [`Module`] and one
//! [`export!`]; its entry points receive an [`ExecCtx`] or a [`QueryCtx`],
//! whose methods are the whole host surface: `env`, `sender` (the account a
//! write acts as, which the host resolved through the identity role), raw
//! state (`get`, `set`, `delete`, `scan`), blobs, `event`, `set_return_data`,
//! `sha256` and `verify`, and the two ways to another module: `emit` (a write
//! it runs in this frame, replying or not) and `query` (a read). On wasm32
//! they are host calls; natively the same contexts run over a [`MockHost`].
//! [`Env`] adds the origin checks (`signer`, `sending_module`, `sent_by`) and
//! `authority`, a stub that admits anyone for now.
//! A message emitted with [`Reply::Wanted`] comes back to its emitter in
//! the same frame as [`Module::reply`], with its id and outcome; a module
//! that never wants a reply leaves `reply` at its default.
//!
//! ```ignore
//! use guest::{Error, ExecCtx, Module, QueryCtx};
//!
//! pub struct Counter;
//!
//! impl Module for Counter {
//!     type Op = u64;
//!     type Query = ();
//!     type Response = u64;
//!
//!     fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
//!         let n: u64 = ctx.record("n")?.unwrap_or(0);
//!         ctx.put("n", &(n + by));
//!         Ok(())
//!     }
//!
//!     fn query(ctx: &QueryCtx, (): ()) -> Result<u64, Error> {
//!         Ok(ctx.record("n")?.unwrap_or(0))
//!     }
//! }
//!
//! guest::export!(Counter);
//! ```
//!
//! `store` is the optional typed layer on top (`Map`/`Set`/`Item`, pages).

mod ctx;
mod identity;
pub mod kernel;
#[cfg(not(target_arch = "wasm32"))]
mod mock;
mod module;
mod origin;
pub mod refuse;

pub use abi;
pub use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, Message, Root, Scheme,
};
pub use ctx::{ExecCtx, QueryCtx, Reply};
pub use kernel::{
    AccountNumber, Cause, Env, Error, MessageId, ModuleId, Order, Origin, Outcome, Principal,
    Range, Roles, code,
};
#[cfg(not(target_arch = "wasm32"))]
pub use mock::{
    Execute, MAX_DEPTH, MockChain, MockHost, MockState, Sibling, Verifier, blob_id, identity_role,
};
#[cfg(target_arch = "wasm32")]
pub use module::exports;
pub use module::{Module, execute, query};
pub use refuse::{
    already_exists, capacity, corrupt, decoded, invalid, not_found, stale, unauthorized,
    wrong_state,
};
