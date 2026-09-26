//! What a module is ([`Module`]), the plain functions that decode an
//! invocation and answer it, and [`export!`](crate::export), which only
//! points the two wasm exports at them.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Cause, Error, ExecCtx, MessageId, Outcome, QueryCtx, decoded};

/// A module: its op, query and response types, and what it does with each.
pub trait Module {
    type Op: BorshDeserialize;
    type Query: BorshDeserialize;
    type Response: BorshSerialize;

    /// Genesis, with the params the network was founded with. Nothing by
    /// default; override only where genesis needs it.
    fn init(_ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Self::Op) -> Result<(), Error>;

    /// The outcome of message `id`, which this module emitted with
    /// [`Reply::Wanted`](crate::Reply::Wanted). A refusal here fails the
    /// frame; Ok absorbs the outcome. By default an applied message is
    /// absorbed and a refused one's refusal is returned, failing the frame
    /// as if the message had been sent with [`Reply::None`](crate::Reply::None);
    /// override to accept a refusal.
    fn reply(_ctx: &ExecCtx, _id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        match outcome {
            Outcome::Applied { .. } => Ok(()),
            Outcome::Rejected(refusal) => Err(refusal.clone()),
        }
    }

    fn query(ctx: &QueryCtx, query: Self::Query) -> Result<Self::Response, Error>;
}

/// An execute's payload decoded as `M::Op` (refused as invalid input when it
/// does not decode), then run; a [`Cause::Reply`] carries no op (the kernel
/// sends an empty payload) and runs [`Module::reply`] instead.
pub fn execute<M: Module>(ctx: &ExecCtx, payload: &[u8]) -> Result<(), Error> {
    if let Cause::Reply { id, outcome } = &ctx.env().cause {
        return M::reply(ctx, id, outcome);
    }
    let op = decoded::<M::Op>(&ctx.env().module, "Op", payload)?;
    M::execute(ctx, op)
}

/// A query's request decoded as `M::Query`, answered, and the borsh of the
/// response handed to the host.
pub fn query<M: Module>(ctx: &QueryCtx, request: &[u8]) -> Result<(), Error> {
    let query = decoded::<M::Query>(&ctx.env().module, "Query", request)?;
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
        let env = env.into();
        let reply = match call {
            GuestCall::Init(params) => M::init(&ExecCtx::new(env), &params),
            GuestCall::Execute(payload) => execute::<M>(&ExecCtx::new(env), &payload),
            GuestCall::Query(request) => query::<M>(&QueryCtx::new(env), &request),
        };
        reply.map_err(crate::kernel::refusal_from)
    }

    fn leak(bytes: Vec<u8>) -> u64 {
        let len = bytes.len() as u64;
        let ptr = bytes.as_ptr() as u64;
        core::mem::forget(bytes);
        (ptr << 32) | len
    }
}

/// The module's two wasm exports, `alloc` and `call`, forwarding to
/// `exports::alloc` (wasm32 only) and `exports::call::<$module>`, where
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{MockHost, Origin, Principal, code};

    /// A module whose op, a `u64`, never decodes from a reply's empty
    /// payload, and which keeps the replies it gets.
    struct Asker;

    impl Module for Asker {
        type Op = u64;
        type Query = ();
        type Response = ();

        fn execute(_: &ExecCtx, _: u64) -> Result<(), Error> {
            Err(Error::new(code::WRONG_STATE, "a reply is not an op"))
        }

        fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
            ctx.put("reply", &(id, outcome));
            Ok(())
        }

        fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn a_reply_runs_reply_with_its_id_and_outcome_not_the_op() {
        let id = MessageId {
            module: "asker".into(),
            seq: 3,
        };
        let outcome = Outcome::Rejected(Error::new(code::NOT_FOUND, "no such thing"));
        let env = crate::Env {
            chain_id: b"n".to_vec(),
            height: 1,
            time: 2,
            module: "asker".into(),
            origin: Origin::Module("target".into()),
            sender: Some(Principal::Account(7)),
            roles: MockHost::roles(),
            cause: Cause::Reply {
                id: id.clone(),
                outcome: outcome.clone(),
            },
        };
        let host = MockHost::default();
        execute::<Asker>(&host.exec(env.clone()), &[]).unwrap();
        let kept: Option<(MessageId, Outcome)> = host.query(env).record("reply").unwrap();
        assert_eq!(kept, Some((id, outcome)));
    }
}
