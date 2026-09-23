// The wasm32 program over the rules: guest contexts as the store, the bytes decoded.

use abi::{Env, Refusal};
use guest::{Execute, Program, Query as QueryCtx};
use store::{decoded, invalid};

use crate::{Op, PROGRAM};

struct Admission;

impl Program for Admission {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        crate::rules::execute(ctx, env, decoded::<Op>(PROGRAM, "Op", payload)?)
    }

    fn query(_ctx: &mut QueryCtx, _env: &Env, _request: &[u8]) -> Result<(), Refusal> {
        Err(invalid("admission answers no query"))
    }
}

guest::program!(Admission);
