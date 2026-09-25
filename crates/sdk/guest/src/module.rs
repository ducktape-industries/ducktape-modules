//! What a module is ([`Module`]), the plain functions that decode an
//! invocation and answer it, and [`export!`](crate::export), which only
//! points the two wasm exports at them.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{ExecCtx, QueryCtx, Refusal, decoded};

/// A module: its op, query and response types, and what it does with each.
pub trait Module {
    type Op: BorshDeserialize;
    type Query: BorshDeserialize;
    type Response: BorshSerialize;

    /// Genesis, with the params the network was founded with. Nothing by
    /// default; override only where genesis needs it.
    fn init(_ctx: &ExecCtx, _params: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Self::Op) -> Result<(), Refusal>;

    fn query(ctx: &QueryCtx, query: Self::Query) -> Result<Self::Response, Refusal>;
}

/// An execute's payload decoded as `M::Op` (refused as invalid input when it
/// does not decode), then run.
pub fn execute<M: Module>(ctx: &ExecCtx, payload: &[u8]) -> Result<(), Refusal> {
    let op = decoded::<M::Op>(&ctx.env().me, "Op", payload)?;
    M::execute(ctx, op)
}

/// A query's request decoded as `M::Query`, answered, and the borsh of the
/// response handed to the host.
pub fn query<M: Module>(ctx: &QueryCtx, request: &[u8]) -> Result<(), Refusal> {
    let query = decoded::<M::Query>(&ctx.env().me, "Query", request)?;
    let response = M::query(ctx, query)?;
    ctx.respond(abi::encode(&response));
    Ok(())
}

/// The two wasm exports' bodies, over the host's calling convention: the
/// host writes an [`Invocation`](abi::Invocation) into memory `alloc` gave
/// it, and `call` answers a [`GuestReply`](abi::GuestReply) as `ptr << 32 | len`.
#[cfg(target_arch = "wasm32")]
pub mod exports {
    use abi::{GuestCall, GuestReply, Invocation, Refusal, reason};

    use super::{Module, execute, query};
    use crate::{ExecCtx, QueryCtx};

    pub fn alloc(len: u32) -> u32 {
        let mut buffer = Vec::<u8>::with_capacity(len as usize);
        let ptr = buffer.as_mut_ptr();
        core::mem::forget(buffer);
        ptr as u32
    }

    pub fn call<M: Module>(ptr: u32, len: u32) -> u64 {
        let request = unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) };
        let reply = match abi::decode::<Invocation>(&request) {
            Ok(invocation) => dispatch::<M>(invocation),
            Err(fault) => Err(Refusal::new(reason::PROTOCOL, fault.sentence)),
        };
        leak(abi::encode(&reply))
    }

    fn dispatch<M: Module>(Invocation { env, call }: Invocation) -> GuestReply {
        match call {
            GuestCall::Init(params) => M::init(&ExecCtx::new(env), &params),
            GuestCall::Execute(payload) => execute::<M>(&ExecCtx::new(env), &payload),
            GuestCall::Query(request) => query::<M>(&QueryCtx::new(env), &request),
        }
    }

    fn leak(bytes: Vec<u8>) -> u64 {
        let len = bytes.len() as u64;
        let ptr = bytes.as_ptr() as u64;
        core::mem::forget(bytes);
        (ptr << 32) | len
    }
}

/// The module's two wasm exports, `alloc` and `call`, forwarding to
/// [`exports::alloc`](crate::exports) and `exports::call::<$module>`, where
/// the decoding, the error mapping and the response encoding live.
/// `guest::export!(Counter);` expands to:
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// #[unsafe(no_mangle)]
/// pub extern "C" fn alloc(len: u32) -> u32 {
///     guest::exports::alloc(len)
/// }
///
/// #[cfg(target_arch = "wasm32")]
/// #[unsafe(no_mangle)]
/// pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
///     guest::exports::call::<Counter>(ptr, len)
/// }
/// ```
///
/// The exports are global symbols, so a wasm crate has one `export!`. A
/// module crate another crate links (forge links chat) puts its `export!`
/// behind a feature only its own wasm build turns on.
#[macro_export]
macro_rules! export {
    ($module:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(len: u32) -> u32 {
            $crate::exports::alloc(len)
        }

        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
            $crate::exports::call::<$module>(ptr, len)
        }
    };
}
