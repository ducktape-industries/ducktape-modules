//! What every system program checks: the frame's origin.

use abi::{Env, Origin, ProgramId, Refusal, reason};

pub fn external(env: &Env) -> Result<Vec<u8>, Refusal> {
    match &env.origin {
        Origin::External(signer) => Ok(signer.clone()),
        other => Err(Refusal::new(
            reason::UNAUTHORIZED,
            format!("only a signed frame may do this, not {other:?}"),
        )),
    }
}

pub fn program(env: &Env) -> Result<ProgramId, Refusal> {
    match &env.origin {
        Origin::Program(program) => Ok(program.clone()),
        other => Err(Refusal::new(
            reason::UNAUTHORIZED,
            format!("only a program may do this, not {other:?}"),
        )),
    }
}

pub fn from(env: &Env, program: &str) -> Result<(), Refusal> {
    let sent_by_program = matches!(&env.origin, Origin::Program(sender) if sender == program);
    let sent_by_system = env.origin == Origin::System;
    let authorized = sent_by_program || sent_by_system;
    if !authorized {
        return Err(Refusal::new(
            reason::UNAUTHORIZED,
            format!("only {program} may do this, not {:?}", env.origin),
        ));
    }
    Ok(())
}
