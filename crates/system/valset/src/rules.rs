// The rules: writes from anyone for now (`Env::authority`), the last validator kept seated.

use guest::{Error, ExecCtx, QueryCtx, invalid, wrong_state};
use store::Map;

use crate::{Genesis, Membership, Role};

pub(crate) const MEMBERS: Map<Vec<u8>, Membership> = Map::new("m/");
const KEY_LEN: usize = 32;

pub(crate) fn init(ctx: &ExecCtx, genesis: Genesis) -> Result<(), Error> {
    for member in genesis.validators {
        set(
            ctx,
            Membership {
                key: member.key,
                address: member.address,
                role: Role::Validator,
            },
        )?;
    }
    Ok(())
}

pub(crate) fn memberships(ctx: &QueryCtx) -> Result<Vec<Membership>, Error> {
    Ok(MEMBERS
        .all(ctx)?
        .into_iter()
        .map(|(_, membership)| membership)
        .collect())
}

pub(crate) fn set(ctx: &ExecCtx, membership: Membership) -> Result<(), Error> {
    let key_is_ed25519 = membership.key.len() == KEY_LEN;
    if !key_is_ed25519 {
        return Err(invalid("a member key is a 32-byte ed25519 public key"));
    }
    let demotes = membership.role == Role::Resident;
    if demotes {
        unseat(ctx, &membership.key)?;
    }
    MEMBERS.put(ctx, &membership.key, &membership);
    Ok(())
}

pub(crate) fn remove(ctx: &ExecCtx, member: &Vec<u8>) -> Result<(), Error> {
    unseat(ctx, member)?;
    MEMBERS.remove(ctx, member);
    Ok(())
}

fn unseat(ctx: &QueryCtx, member: &Vec<u8>) -> Result<(), Error> {
    let seated = MEMBERS
        .get(ctx, member)?
        .is_some_and(|membership| membership.role == Role::Validator);
    if !seated {
        return Ok(());
    }
    let other_validators = memberships(ctx)?
        .iter()
        .any(|membership| membership.role == Role::Validator && &membership.key != member);
    if !other_validators {
        return Err(wrong_state("the last validator cannot be unseated"));
    }
    Ok(())
}
