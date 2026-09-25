// The wasm32 program over the rules: decode, then `execute_from`/`query`.

use abi::{Env, Refusal};
use guest::{Execute, Program, Query as QueryCtx};
use store::decoded;

use crate::{Op, PROGRAM, Query};

struct Chat;

impl Program for Chat {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let op = decoded::<Op>(PROGRAM, "Op", payload)?;
        crate::execute_from(ctx, env, op)
    }

    fn query(ctx: &mut QueryCtx, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let query = decoded::<Query>(PROGRAM, "Query", request)?;
        ctx.reply(&crate::query(ctx, env.height, query)?);
        Ok(())
    }
}

guest::program!(Chat);
