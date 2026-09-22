//! What every system program checks and builds: the frame's origin, keys under a prefix, refusals.

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

pub fn u64_key(prefix: &str, number: u64) -> Vec<u8> {
    let mut key = prefix.as_bytes().to_vec();
    key.extend_from_slice(&number.to_be_bytes());
    key
}

pub fn bytes_key(prefix: &str, bytes: &[u8]) -> Vec<u8> {
    let mut key = prefix.as_bytes().to_vec();
    key.extend_from_slice(bytes);
    key
}

pub fn not_found(what: impl Into<String>) -> Refusal {
    Refusal::new(reason::NOT_FOUND, what)
}

pub fn invalid(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::INVALID_INPUT, sentence)
}

pub fn already_exists(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::ALREADY_EXISTS, sentence)
}

pub fn wrong_state(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::WRONG_STATE, sentence)
}

pub fn unauthorized(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::UNAUTHORIZED, sentence)
}
