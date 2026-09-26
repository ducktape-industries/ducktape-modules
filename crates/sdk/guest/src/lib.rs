//! The minimal module SDK. A module is a type implementing [`Module`] and one
//! [`export!`]; its entry points receive an [`ExecCtx`] or a [`QueryCtx`],
//! whose methods are the whole host surface: `env`, raw state (`get`, `set`,
//! `delete`, `scan`), blobs, `send`/`call`, `event`, `set_return_data`,
//! `query`/`ask` of another module, `sha256` and `verify`. On wasm32 they are host calls;
//! natively the same contexts run over a [`MockHost`].
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
pub use ctx::{ExecCtx, QueryCtx};
pub use kernel::{
    AccountNumber, Cause, Env, Error, MessageId, ModuleId, Order, Origin, Outcome, Principal,
    Range, code,
};
#[cfg(not(target_arch = "wasm32"))]
pub use mock::{MockHost, MockState, Sibling, Verifier, blob_id};
#[cfg(target_arch = "wasm32")]
pub use module::exports;
pub use module::{Module, execute, query};
pub use refuse::{
    already_exists, capacity, corrupt, decoded, invalid, not_found, stale, unauthorized,
    wrong_state,
};
