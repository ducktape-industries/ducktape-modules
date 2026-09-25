//! What every system module checks: the frame's origin.

use guest::{Env, Error, ModuleId, Origin, code};

pub fn external(env: &Env) -> Result<Vec<u8>, Error> {
    match &env.origin {
        Origin::Signed(signer) => Ok(signer.clone()),
        other => Err(Error::new(
            code::UNAUTHORIZED,
            format!("only a signed frame may do this, not {other:?}"),
        )),
    }
}

pub fn program(env: &Env) -> Result<ModuleId, Error> {
    match &env.origin {
        Origin::Module(program) => Ok(program.clone()),
        other => Err(Error::new(
            code::UNAUTHORIZED,
            format!("only a program may do this, not {other:?}"),
        )),
    }
}

pub fn from(env: &Env, program: &str) -> Result<(), Error> {
    let sent_by_program = matches!(&env.origin, Origin::Module(sender) if sender == program);
    let sent_by_system = env.origin == Origin::Root;
    let authorized = sent_by_program || sent_by_system;
    if !authorized {
        return Err(Error::new(
            code::UNAUTHORIZED,
            format!("only {program} may do this, not {:?}", env.origin),
        ));
    }
    Ok(())
}
