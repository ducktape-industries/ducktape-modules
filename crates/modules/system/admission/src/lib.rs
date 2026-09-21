use abi::{Env, Refusal};
use guest::{Execute, Program, Query as QueryCtx};
use modules::admission::Op;
use modules::program::{external, unsupported};
use modules::valset::{self, Membership, Standing};

struct Admission;

impl Program for Admission {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let key = external(env)?;
        match abi::decode(payload)? {
            Op::Enroll { address } => enroll(ctx, key, address),
        }
    }

    fn query(_ctx: &mut QueryCtx, _env: &Env, _request: &[u8]) -> Result<(), Refusal> {
        Err(unsupported("admission answers no query"))
    }
}

fn enroll(ctx: &mut Execute, key: Vec<u8>, address: String) -> Result<(), Refusal> {
    let standing = valset::standing(ctx, &key)?.unwrap_or(Standing::Resident);
    let membership = Membership {
        key,
        address,
        standing,
    };
    ctx.emit(valset::PROGRAM, abi::encode(&valset::Op::Set(membership)));
    Ok(())
}

guest::program!(Admission);
