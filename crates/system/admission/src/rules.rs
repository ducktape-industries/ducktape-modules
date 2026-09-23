// The rules over any store: a signed frame enrolls its signer; the standing it holds is kept.

use abi::{Env, Refusal};
use module_registry::helpers::external;
use store::Writes;
use valset::{Membership, Standing};

use crate::Op;

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Refusal> {
    let key = external(env)?;
    match op {
        Op::Enroll { address } => enroll(store, key, address),
    }
}

fn enroll(store: &mut impl Writes, key: Vec<u8>, address: String) -> Result<(), Refusal> {
    let standing = valset::standing(store, &key)?.unwrap_or(Standing::Resident);
    let membership = Membership {
        key,
        address,
        standing,
    };
    store.emit(valset::PROGRAM, abi::encode(&valset::Op::Set(membership)));
    Ok(())
}
