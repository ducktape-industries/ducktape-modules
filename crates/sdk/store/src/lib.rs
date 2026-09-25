//! The module SDK: what a module holds in its hands.
//!
//! - [`Reads`] and [`Writes`]: the store a call runs over. A query gets
//!   `&impl Reads`, an execute `&mut impl Writes`; both are one required
//!   method over the host, so a module's rules are written once, generic
//!   over the store, and run the same natively ([`testing::MockHost`]) and
//!   on the host ([`entrypoint!`], the `module` feature).
//! - [`Map`], [`Set`], [`Item`]: typed tables; [`PageRequest`] and
//!   [`PageResponse`]: the one paging vocabulary.
//! - [`Env`], [`Origin`], [`Error`] and the rest of `kernel.rs`: the kernel's
//!   types under the SDK's names; `origin.rs`: the origin guards.
//! - [`error`]: the error constructors (`invalid`, `not_found`, ...).

pub mod error;
#[cfg(all(feature = "module", target_arch = "wasm32"))]
#[doc(hidden)]
pub mod guest;
mod kernel;
mod key;
mod origin;
mod page;
mod table;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;

pub use error::{
    already_exists, capacity, corrupt, decoded, invalid, not_found, stale, unauthorized,
    wrong_state,
};
pub use kernel::*;
pub use key::KeyCodec;
pub use page::{Cursor, Listing, PageRequest, PageResponse};
pub use table::{Item, Map, Set};

use abi::{CryptoOp, CryptoReply, HostOp, HostReply};
use borsh::{BorshDeserialize, BorshSerialize};

fn protocol(expected: &str, got: HostReply) -> ! {
    panic!("host answered {got:?} where {expected} was expected")
}

fn done(reply: HostReply) {
    match reply {
        HostReply::Done => {}
        other => protocol("done", other),
    }
}

pub trait Reads {
    /// The host's read ops, in the kernel's terms. A module never calls it;
    /// a store implements it.
    #[doc(hidden)]
    fn host(&self, op: &HostOp) -> HostReply;

    fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(&HostOp::Get(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    fn scan(&self, range: Range) -> Vec<Entry> {
        match self.host(&HostOp::Scan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    fn committed_get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(&HostOp::CommittedGet(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    fn committed_scan(&self, range: Range) -> Vec<Entry> {
        match self.host(&HostOp::CommittedScan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    fn record<T: BorshDeserialize>(&self, key: impl AsRef<[u8]>) -> Result<Option<T>, Error> {
        self.get(key).map(|bytes| decode(&bytes)).transpose()
    }

    fn records<T: BorshDeserialize>(&self, range: Range) -> Result<Vec<(Vec<u8>, T)>, Error> {
        self.scan(range)
            .into_iter()
            .map(|entry| Ok((entry.key, decode(&entry.value)?)))
            .collect()
    }

    fn blob_get(&self, id: BlobId) -> Option<Blob> {
        match self.host(&HostOp::BlobGet(id)) {
            HostReply::Blob(blob) => blob,
            other => protocol("blob", other),
        }
    }

    fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        match self.host(&HostOp::BlobStat(id)) {
            HostReply::BlobHeader(header) => header,
            other => protocol("blob header", other),
        }
    }

    fn blob_read(&self, id: BlobId, offset: u64, len: u64) -> Option<Vec<u8>> {
        match self.host(&HostOp::BlobRead { id, offset, len }) {
            HostReply::Value(bytes) => bytes,
            other => protocol("value", other),
        }
    }

    fn root(&self, module: impl Into<ModuleId>) -> Option<Root> {
        match self.host(&HostOp::Root(module.into())) {
            HostReply::Root(root) => root,
            other => protocol("root", other),
        }
    }

    /// Another module's query, in bytes.
    fn query(
        &self,
        module: impl Into<ModuleId>,
        request: impl Into<Vec<u8>>,
    ) -> Result<Vec<u8>, Error> {
        match self.host(&HostOp::Query {
            program: module.into(),
            request: request.into(),
        }) {
            HostReply::Query(answer) => answer.map_err(Error::from),
            other => protocol("query answer", other),
        }
    }

    /// Another module's query, typed.
    fn ask<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        module: impl Into<ModuleId>,
        request: &Q,
    ) -> Result<R, Error> {
        decode(&self.query(module, encode(request))?)
    }

    fn sha256(&self, bytes: impl Into<Vec<u8>>) -> [u8; 32] {
        match self.host(&HostOp::Crypto(CryptoOp::Sha256(bytes.into()))) {
            HostReply::Crypto(CryptoReply::Digest(digest)) => digest,
            other => protocol("digest", other),
        }
    }

    fn verify(
        &self,
        scheme: Scheme,
        key: impl Into<Vec<u8>>,
        namespace: impl Into<Vec<u8>>,
        message: impl Into<Vec<u8>>,
        signature: impl Into<Vec<u8>>,
    ) -> Result<bool, Error> {
        match self.host(&HostOp::Crypto(CryptoOp::Verify {
            scheme,
            key: key.into(),
            namespace: namespace.into(),
            message: message.into(),
            signature: signature.into(),
        })) {
            HostReply::Crypto(CryptoReply::Verified(valid)) => Ok(valid),
            HostReply::Refused(refusal) => Err(refusal.into()),
            other => protocol("verdict", other),
        }
    }
}

/// What an execute may do beyond reading: the state and blob writes, and
/// what leaves the module ([`send`](Writes::send), [`call`](Writes::call),
/// [`event`](Writes::event), [`set_return_data`](Writes::set_return_data)).
/// [`testing::MockHost`] records the latter for a test to read.
pub trait Writes: Reads {
    /// The host's write ops, in the kernel's terms. A module never calls it;
    /// a store implements it.
    #[doc(hidden)]
    fn host_mut(&mut self, op: HostOp) -> HostReply;

    fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        done(self.host_mut(HostOp::Set {
            key: key.into(),
            value: value.into(),
        }))
    }

    fn delete(&mut self, key: impl Into<Vec<u8>>) {
        done(self.host_mut(HostOp::Delete(key.into())))
    }

    fn blob_put(
        &mut self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Error> {
        match self.host_mut(HostOp::BlobPut {
            hash,
            kind: kind.into(),
            body: body.into(),
        }) {
            HostReply::BlobId(id) => Ok(id),
            HostReply::Refused(refusal) => Err(refusal.into()),
            other => protocol("blob id", other),
        }
    }

    /// Sends `payload` to `target` after this call commits; no reply.
    fn send(&mut self, target: impl Into<ModuleId>, payload: impl Into<Vec<u8>>) -> MessageId {
        message(self, target.into(), payload.into(), false)
    }

    /// Sends `payload` to `target` after this call commits; its outcome
    /// comes back to this module as [`Cause::Reply`].
    fn call(&mut self, target: impl Into<ModuleId>, payload: impl Into<Vec<u8>>) -> MessageId {
        message(self, target.into(), payload.into(), true)
    }

    /// A log line for indexers; nothing reads it back.
    fn event(&mut self, payload: impl Into<Vec<u8>>) {
        done(self.host_mut(HostOp::Event(payload.into())))
    }

    /// What this call returns to its caller (the last one set wins).
    fn set_return_data(&mut self, bytes: impl Into<Vec<u8>>) {
        done(self.host_mut(HostOp::Output(bytes.into())))
    }
}

fn message<W: Writes + ?Sized>(
    store: &mut W,
    target: ModuleId,
    payload: Vec<u8>,
    reply: bool,
) -> MessageId {
    match store.host_mut(HostOp::Emit(Message {
        target,
        payload,
        reply,
    })) {
        HostReply::Item(item) => item.into(),
        other => protocol("item", other),
    }
}

/// What a query returns: a borsh value, or [`Raw`] bytes already in the
/// caller's format (forge answers a git client in git's own bytes).
pub trait Response {
    fn into_bytes(self) -> Vec<u8>;
}

impl<T: BorshSerialize> Response for T {
    fn into_bytes(self) -> Vec<u8> {
        encode(&self)
    }
}

/// A query answer that is its own bytes, not borsh.
pub struct Raw(pub Vec<u8>);

impl Response for Raw {
    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// A module's wasm32 entry points, typed. Every module's glue is this one
/// line; the bytes are decoded (refused as `invalid_input` when they do not
/// decode) and a query's answer is its return data.
///
/// ```ignore
/// store::entrypoint! {
///     init: Genesis => rules::init,   // optional: init(store, Genesis)
///     execute: Op => rules::execute,  // execute(store, &Env, Op)
///     query: Query => rules::query,   // query(&store, &Env, Query) -> Result<impl Response, Error>
/// }
/// ```
///
/// With `sender: resolve,` after `execute`, the execute entry is
/// `execute(store, &Env, sender, Op)`, the sender being
/// `resolve(&store, &env.origin)?` (chat and forge: `identity::principal_of`).
#[macro_export]
macro_rules! entrypoint {
    (
        $(init: $genesis:ty => $init:path,)?
        execute: $op:ty => $execute:path,
        $(sender: $sender:path,)?
        query: $query:ty => $answer:path $(,)?
    ) => {
        #[cfg(all(feature = "module", target_arch = "wasm32"))]
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(len: u32) -> u32 {
            $crate::guest::alloc(len)
        }

        #[cfg(all(feature = "module", target_arch = "wasm32"))]
        #[unsafe(no_mangle)]
        pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
            use $crate::guest::Call;
            $crate::guest::serve(ptr, len, |host, env, call| match call {
                Call::Init(_bytes) => {
                    $(return $init(host, $crate::decoded::<$genesis>(&env.module, stringify!($genesis), &_bytes)?);)?
                    #[allow(unreachable_code)]
                    Ok(())
                }
                Call::Execute(bytes) => {
                    let op = $crate::decoded::<$op>(&env.module, stringify!($op), &bytes)?;
                    $crate::entrypoint!(@execute $execute, host, env, op $(, $sender)?)
                }
                Call::Query(bytes) => {
                    let query = $crate::decoded::<$query>(&env.module, stringify!($query), &bytes)?;
                    let answer = $answer(&*host, env, query)?;
                    $crate::Writes::set_return_data(host, $crate::Response::into_bytes(answer));
                    Ok(())
                }
            })
        }
    };
    (@execute $execute:path, $host:ident, $env:ident, $op:ident) => {
        $execute($host, $env, $op)
    };
    (@execute $execute:path, $host:ident, $env:ident, $op:ident, $sender:path) => {{
        let sender = $sender(&*$host, &$env.origin)?;
        $execute($host, $env, sender, $op)
    }};
}
