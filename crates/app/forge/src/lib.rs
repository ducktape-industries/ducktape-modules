//! forge: a git server as a ducktape module, `gitcore` (objects, packs,
//! walks, diffs, the wire) over `store`. The rules run natively over
//! `store::testing::MockHost` (tests, fixtures); the `module` feature adds the wasm32
//! module over the host.
//!
//! A write is an [`Op`] run as a [`Principal`] (an account: the signer resolved
//! by identity's [`principal_of`](identity::principal_of), which refuses a key that
//! holds none), a read
//! a [`Query`] answered by a [`Reply`]. The layout, in reading order:
//!
//! - `contract.rs`, `read_contract.rs`, `review_contract.rs`: the wire
//! - `state.rs`: every table and index, declared once
//! - `ops.rs`: [`execute`] and the repository ops; `changes.rs` the change ops
//! - `queries.rs`: [`query`]; `reads.rs`, `diffs.rs`, `change_queries.rs`
//!   answer its object and change questions
//! - `objects.rs`: git objects over the store's blobs
//! - `discussion.rs`: what forge asks of and posts into chat
//! - `description.rs`: [`describe`], an op in a person's words
//! - `store::entrypoint!` in this file (`module` feature): the wasm32 glue

// The wire, as a view and a git client see it.
mod contract;
mod read_contract;
mod review_contract;

// The state and the rules over it.
mod change_queries;
mod changes;
mod description;
mod diffs;
mod discussion;
mod objects;
mod ops;
mod queries;
mod reads;
mod state;

#[cfg(feature = "view")]
pub mod view;

pub use contract::*;
pub use description::describe;
pub use ops::{MODULE, execute, init};
pub use queries::query;

describe::export!(Op, describe);

store::entrypoint! {
    init: Bounds => init,
    execute: Op => execute,
    sender: identity::principal_of,
    query: Query => query,
}

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
/// (Grant, Revoke, ChangeOpen and ChangeEdit name principals since the stage
/// refound that made forge account-keyed; their names and order held.)
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Create",
            "Configure",
            "Grant",
            "Revoke",
            "Push",
            "Merge",
            "ChangeOpen",
            "ChangeEdit",
            "ChangeClose",
            "ReviewSubmit",
        ]
    );
}
