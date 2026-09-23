// The wasm entry: guest contexts as the store, the Program the runtime dispatches into.

use abi::{Env, Refusal};
use guest::{Execute, Program, Query};

struct Forge;

impl Program for Forge {
    fn init(ctx: &mut Execute, _env: &Env, params: &[u8]) -> Result<(), Refusal> {
        crate::init(ctx, params)
    }

    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        crate::execute(ctx, env, payload)
    }

    fn query(ctx: &mut Query, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let response = crate::query(ctx, env, request)?;
        ctx.respond(response);
        Ok(())
    }
}

guest::program!(Forge);
