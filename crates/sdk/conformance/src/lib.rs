//! Role conformance: proof that a module fills a role the kernel calls
//! (`abi::role::{registry, validators, identity}`).
//!
//! Each role is a module here with a `Fixture` trait and a `run` that
//! checks the role's contract, one named function per rule. The suite
//! speaks the role's bytes alone: it encodes `abi::role` values, hands
//! them to the module's own decoding (`guest::execute`, `guest::query`)
//! over a [`MockHost`], and decodes the answer as the role's reply, so a
//! module whose first variants drift from the role fails here as it would
//! under the kernel. What the role leaves to the module (how an account
//! comes to hold a key, how a validator is seated, how code is scheduled)
//! the fixture does with the module's own ops.
//!
//! ```ignore
//! #[test]
//! fn fills_the_identity_role() {
//!     conformance::identity::run(&MyFixture);
//! }
//! ```

use borsh::{BorshDeserialize, BorshSerialize};
use guest::{Cause, Env, Error, MockHost, Module, Origin, Principal};

pub mod identity;
pub mod registry;
pub mod validators;

/// An env for `module` at `height`, as the kernel would send it.
pub fn env(module: &str, height: u64, origin: Origin, sender: Option<Principal>) -> Env {
    Env {
        chain_id: b"conformance".to_vec(),
        height,
        time: 0,
        module: module.into(),
        origin,
        sender,
        roles: MockHost::roles(),
        cause: Cause::Direct,
    }
}

/// The env of a founding or a kernel call: the chain itself.
pub fn root(module: &str, height: u64) -> Env {
    env(module, height, Origin::Root, Some(Principal::Root))
}

/// `M`'s init with the role's genesis, as founding runs it.
fn init<M: Module>(host: &MockHost, module: &str, genesis: &impl BorshSerialize) {
    M::init(&host.exec(root(module, 0)), &abi::encode(genesis))
        .unwrap_or_else(|e| panic!("{module}: init refused the role's genesis: {e:?}"));
}

/// `op`, in the role's bytes, run by `M` in `env`.
fn execute<M: Module>(host: &MockHost, env: Env, op: &impl BorshSerialize) -> Result<(), Error> {
    guest::execute::<M>(&host.exec(env), &abi::encode(op))
}

/// `query`, in the role's bytes, asked of `M` at `height`; its answer read
/// as the role's reply.
fn ask<M: Module, R: BorshDeserialize + std::fmt::Debug>(
    host: &MockHost,
    module: &str,
    height: u64,
    query: &(impl BorshSerialize + std::fmt::Debug),
) -> Result<R, Error> {
    let ctx = host.query(env(module, height, Origin::Root, None));
    guest::query::<M>(&ctx, &abi::encode(query))?;
    let bytes = std::mem::take(&mut host.borrow_mut().response);
    Ok(abi::decode(&bytes).unwrap_or_else(|e| {
        panic!("{module}: the answer to {query:?} is not the role's reply: {e}")
    }))
}

/// The role's value decodes as the module's `T` and encodes back to the
/// same bytes: the role's variants are the module's first, in order, with
/// the same fields.
#[track_caller]
fn same_bytes<T: BorshSerialize + BorshDeserialize>(
    module: &str,
    what: &str,
    role: &(impl BorshSerialize + std::fmt::Debug),
) {
    let bytes = abi::encode(role);
    let ours: T = abi::decode(&bytes).unwrap_or_else(|e| {
        panic!("{module}: the role's {role:?} does not decode as the module's {what}: {e}")
    });
    assert_eq!(
        abi::encode(&ours),
        bytes,
        "{module}: the role's {role:?} is not the module's {what} byte for byte: \
         the role's variants come first, in order, with the same fields"
    );
}
