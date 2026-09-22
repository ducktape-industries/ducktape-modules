// The wasm32 program over the rules: guest contexts as the store, the bytes decoded and answered.

use abi::{Env, Refusal};
use guest::{Execute, Program, Query as QueryCtx};
use store::decoded;

use crate::{Op, PROGRAM, Query};

struct Identity;

impl Program for Identity {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        crate::rules::execute(ctx, env, decoded::<Op>(PROGRAM, "Op", payload)?)
    }

    fn query(ctx: &mut QueryCtx, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = crate::rules::query(ctx, env, decoded::<Query>(PROGRAM, "Query", request)?)?;
        ctx.reply(&reply);
        Ok(())
    }
}

guest::program!(Identity);
