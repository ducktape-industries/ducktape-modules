//! The wasm32 glue: the signer resolved to a [`Party`] the way chat
//! resolves it, then the typed [`execute`](crate::execute) and
//! [`query`](crate::query).

use abi::{Env, Refusal};
use guest::{Execute, Program, Query};
use store::decoded;

use crate::{Frame, Op, PROGRAM};

struct Forge;

impl Program for Forge {
    fn init(ctx: &mut Execute, _env: &Env, params: &[u8]) -> Result<(), Refusal> {
        crate::init(ctx, params)
    }

    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let op = decoded::<Op>(PROGRAM, "Op", payload)?;
        let frame = Frame {
            party: chat::party_of(ctx, &env.origin)?,
            height: env.height,
            time: env.time,
        };
        crate::execute(ctx, &frame, op)
    }

    fn query(ctx: &mut Query, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let query = decoded::<crate::Query>(PROGRAM, "Query", request)?;
        let response = crate::query(ctx, env.height, query)?;
        ctx.respond(response);
        Ok(())
    }
}

guest::program!(Forge);
