//! The two contexts a module's entry points receive. Every method is one
//! host call: on wasm32 through the `ducktape.*` imports, natively through
//! the [`MockHost`](crate::MockHost) the context was made over. The read
//! surface is defined once, on [`QueryCtx`]; an [`ExecCtx`] derefs to it and
//! adds the writes, so anything that reads takes `&QueryCtx` and accepts
//! either.

use std::ops::Deref;

use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, Message,
    Root, Scheme,
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Env, Error, MessageId, ModuleId, Range};

/// A query's context: the env and the reads. It has no write methods.
pub struct QueryCtx {
    env: Env,
    #[cfg(not(target_arch = "wasm32"))]
    host: crate::MockHost,
}

/// An execute's (or init's) context: every read of [`QueryCtx`] and the
/// writes: state, blobs, and what leaves the module (`send`, `call`,
/// `event`, `set_return_data`).
pub struct ExecCtx {
    reads: QueryCtx,
}

impl Deref for ExecCtx {
    type Target = QueryCtx;

    fn deref(&self) -> &QueryCtx {
        &self.reads
    }
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    use abi::{HostOp, HostReply};

    #[link(wasm_import_module = "ducktape")]
    unsafe extern "C" {
        fn host_call(ptr: u32, len: u32) -> u32;
        fn host_take(ptr: u32);
    }

    pub(super) fn host(op: &HostOp) -> HostReply {
        let request = abi::encode(op);
        let len = unsafe { host_call(request.as_ptr() as u32, request.len() as u32) };
        let mut reply = vec![0u8; len as usize];
        unsafe { host_take(reply.as_mut_ptr() as u32) };
        match abi::decode(&reply) {
            Ok(reply) => reply,
            Err(fault) => panic!("{fault}"),
        }
    }
}

fn protocol(expected: &str, got: HostReply) -> ! {
    panic!("host answered {got:?} where {expected} was expected")
}

#[cfg(target_arch = "wasm32")]
impl QueryCtx {
    pub(crate) fn new(env: Env) -> QueryCtx {
        QueryCtx { env }
    }
}

#[cfg(target_arch = "wasm32")]
impl ExecCtx {
    pub(crate) fn new(env: Env) -> ExecCtx {
        ExecCtx {
            reads: QueryCtx::new(env),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl QueryCtx {
    pub(crate) fn over(host: crate::MockHost, env: Env) -> QueryCtx {
        QueryCtx { env, host }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ExecCtx {
    pub(crate) fn over(host: crate::MockHost, env: Env) -> ExecCtx {
        ExecCtx {
            reads: QueryCtx::over(host, env),
        }
    }
}

impl QueryCtx {
    /// Who called, at what height and time, on which chain.
    pub fn env(&self) -> &Env {
        &self.env
    }

    fn host(&self, op: HostOp) -> HostReply {
        #[cfg(target_arch = "wasm32")]
        return ffi::host(&op);
        #[cfg(not(target_arch = "wasm32"))]
        return self.host.serve(op);
    }

    fn done(&self, op: HostOp) {
        match self.host(op) {
            HostReply::Done => {}
            other => protocol("done", other),
        }
    }

    pub fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(HostOp::Get(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    pub fn scan(&self, range: Range) -> Vec<Entry> {
        match self.host(HostOp::Scan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    pub fn committed_get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(HostOp::CommittedGet(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    pub fn committed_scan(&self, range: Range) -> Vec<Entry> {
        match self.host(HostOp::CommittedScan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    pub fn record<T: BorshDeserialize>(&self, key: impl AsRef<[u8]>) -> Result<Option<T>, Error> {
        self.get(key)
            .map(|bytes| abi::decode(&bytes).map_err(crate::kernel::error_from))
            .transpose()
    }

    pub fn records<T: BorshDeserialize>(&self, range: Range) -> Result<Vec<(Vec<u8>, T)>, Error> {
        self.scan(range)
            .into_iter()
            .map(|entry| {
                Ok((
                    entry.key,
                    abi::decode(&entry.value).map_err(crate::kernel::error_from)?,
                ))
            })
            .collect()
    }

    pub fn blob_get(&self, id: BlobId) -> Option<Blob> {
        match self.host(HostOp::BlobGet(id)) {
            HostReply::Blob(blob) => blob,
            other => protocol("blob", other),
        }
    }

    pub fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        match self.host(HostOp::BlobStat(id)) {
            HostReply::BlobHeader(header) => header,
            other => protocol("blob header", other),
        }
    }

    pub fn blob_read(&self, id: BlobId, offset: u64, len: u64) -> Option<Vec<u8>> {
        match self.host(HostOp::BlobRead { id, offset, len }) {
            HostReply::Value(bytes) => bytes,
            other => protocol("value", other),
        }
    }

    pub fn root(&self, module: impl Into<ModuleId>) -> Option<Root> {
        match self.host(HostOp::Root(module.into())) {
            HostReply::Root(root) => root,
            other => protocol("root", other),
        }
    }

    /// Another module's answer to `request`, raw.
    pub fn query(
        &self,
        module: impl Into<ModuleId>,
        request: impl Into<Vec<u8>>,
    ) -> Result<Vec<u8>, Error> {
        match self.host(HostOp::Query {
            program: module.into(),
            request: request.into(),
        }) {
            HostReply::Query(answer) => answer.map_err(crate::kernel::error_from),
            other => protocol("query answer", other),
        }
    }

    /// Another module's answer to `request`, borsh both ways.
    pub fn ask<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        module: impl Into<ModuleId>,
        request: &Q,
    ) -> Result<R, Error> {
        abi::decode(&self.query(module, abi::encode(request))?).map_err(crate::kernel::error_from)
    }

    pub fn sha256(&self, bytes: impl Into<Vec<u8>>) -> [u8; 32] {
        match self.host(HostOp::Crypto(CryptoOp::Sha256(bytes.into()))) {
            HostReply::Crypto(CryptoReply::Digest(digest)) => digest,
            other => protocol("digest", other),
        }
    }

    pub fn verify(
        &self,
        scheme: Scheme,
        key: impl Into<Vec<u8>>,
        namespace: impl Into<Vec<u8>>,
        message: impl Into<Vec<u8>>,
        signature: impl Into<Vec<u8>>,
    ) -> Result<bool, Error> {
        match self.host(HostOp::Crypto(CryptoOp::Verify {
            scheme,
            key: key.into(),
            namespace: namespace.into(),
            message: message.into(),
            signature: signature.into(),
        })) {
            HostReply::Crypto(CryptoReply::Verified(valid)) => Ok(valid),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("verdict", other),
        }
    }

    /// A query's answer, handed to the host once by [`crate::query`].
    pub(crate) fn respond(&self, bytes: impl Into<Vec<u8>>) {
        self.done(HostOp::Respond(bytes.into()))
    }
}

impl ExecCtx {
    pub fn set(&self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.done(HostOp::Set {
            key: key.into(),
            value: value.into(),
        })
    }

    pub fn delete(&self, key: impl Into<Vec<u8>>) {
        self.done(HostOp::Delete(key.into()))
    }

    pub fn put<T: BorshSerialize>(&self, key: impl Into<Vec<u8>>, record: &T) {
        self.set(key, abi::encode(record))
    }

    pub fn blob_put(
        &self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Error> {
        match self.host(HostOp::BlobPut {
            hash,
            kind: kind.into(),
            body: body.into(),
        }) {
            HostReply::BlobId(id) => Ok(id),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("blob id", other),
        }
    }

    /// Sends `payload` to `target`, delivered after this block; no reply.
    pub fn send(&self, target: impl Into<ModuleId>, payload: impl Into<Vec<u8>>) -> MessageId {
        self.message(target, payload, false)
    }

    /// Sends `payload` to `target`; its outcome comes back as a reply.
    pub fn call(&self, target: impl Into<ModuleId>, payload: impl Into<Vec<u8>>) -> MessageId {
        self.message(target, payload, true)
    }

    fn message(
        &self,
        target: impl Into<ModuleId>,
        payload: impl Into<Vec<u8>>,
        reply: bool,
    ) -> MessageId {
        match self.host(HostOp::Emit(Message {
            target: target.into(),
            payload: payload.into(),
            reply,
        })) {
            HostReply::Item(item) => item.into(),
            other => protocol("item", other),
        }
    }

    pub fn event(&self, payload: impl Into<Vec<u8>>) {
        self.done(HostOp::Event(payload.into()))
    }

    /// The execute's return value (the last one set wins).
    pub fn set_return_data(&self, bytes: impl Into<Vec<u8>>) {
        self.done(HostOp::Output(bytes.into()))
    }
}
