//! A module on the host (wasm32, the `module` feature): the host imports, the
//! one store a call runs over, and what [`entrypoint!`](crate::entrypoint)
//! expands to. Nothing here is named by a module's own code.

use abi::{GuestCall, HostOp, HostReply, Invocation};

use crate::{Env, Error, Reads, Writes};

#[link(wasm_import_module = "ducktape")]
unsafe extern "C" {
    fn host_call(ptr: u32, len: u32) -> u32;
    fn host_take(ptr: u32);
}

fn host(op: &HostOp) -> HostReply {
    let request = abi::encode(op);
    let len = unsafe { host_call(request.as_ptr() as u32, request.len() as u32) };
    let mut reply = vec![0u8; len as usize];
    unsafe { host_take(reply.as_mut_ptr() as u32) };
    match abi::decode(&reply) {
        Ok(reply) => reply,
        Err(fault) => panic!("{fault}"),
    }
}

/// The host as a store. A query gets it as `&impl Reads`; its return data
/// is the host's `Respond`, an execute's is `Output`.
pub struct Host {
    query: bool,
}

impl Reads for Host {
    fn host(&self, op: &HostOp) -> HostReply {
        host(op)
    }
}

impl Writes for Host {
    fn host_mut(&mut self, op: HostOp) -> HostReply {
        let op = match op {
            HostOp::Output(bytes) if self.query => HostOp::Respond(bytes),
            op => op,
        };
        host(&op)
    }
}

/// A decoded call, handed to the module's entry points.
pub enum Call {
    Init(Vec<u8>),
    Execute(Vec<u8>),
    Query(Vec<u8>),
}

pub fn alloc(len: u32) -> u32 {
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr();
    core::mem::forget(buffer);
    ptr as u32
}

/// The `call` export: the invocation decoded, the entry points run over the
/// host, the result encoded back.
pub fn serve(
    ptr: u32,
    len: u32,
    run: impl FnOnce(&mut Host, &Env, Call) -> Result<(), Error>,
) -> u64 {
    let request = unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) };
    let reply: abi::GuestReply = match abi::decode::<Invocation>(&request) {
        Ok(Invocation { env, call }) => {
            let (query, call) = match call {
                GuestCall::Init(bytes) => (false, Call::Init(bytes)),
                GuestCall::Execute(bytes) => (false, Call::Execute(bytes)),
                GuestCall::Query(bytes) => (true, Call::Query(bytes)),
            };
            run(&mut Host { query }, &env.into(), call).map_err(Into::into)
        }
        Err(fault) => Err(abi::Refusal::new(abi::reason::PROTOCOL, fault.sentence)),
    };
    let bytes = abi::encode(&reply);
    let len = bytes.len() as u64;
    let ptr = bytes.as_ptr() as u64;
    core::mem::forget(bytes);
    (ptr << 32) | len
}
