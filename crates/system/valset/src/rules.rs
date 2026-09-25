// The rules over any store: governance-only writes, the last validator kept seated.

use module_registry::AUTHORITY;
use store::{Env, Error};
use store::{Map, Reads, Writes, invalid, wrong_state};

use crate::{Genesis, Membership, Op, Query, Reply, Role};

const MEMBERS: Map<Vec<u8>, Membership> = Map::new("m/");
const KEY_LEN: usize = 32;

pub fn init(store: &mut impl Writes, genesis: Genesis) -> Result<(), Error> {
    for member in genesis.validators {
        set(
            store,
            Membership {
                key: member.key,
                address: member.address,
                role: Role::Validator,
            },
        )?;
    }
    Ok(())
}

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Error> {
    env.require_module(AUTHORITY)?;
    match op {
        Op::Set(membership) => set(store, membership),
        Op::Remove { key } => remove(store, &key),
    }
}

pub fn query(store: &impl Reads, env: &Env, query: Query) -> Result<Reply, Error> {
    Ok(match query {
        Query::Validators => Reply::Validators(
            memberships(store)?
                .into_iter()
                .filter(|membership| membership.role == Role::Validator)
                .map(|membership| membership.key)
                .collect(),
        ),
        Query::Members => {
            Reply::Members(memberships(store)?.iter().map(Membership::member).collect())
        }
        Query::Memberships { page } => Reply::Memberships(
            MEMBERS
                .range(store, &page, env.height)?
                .map(|(_, membership)| membership),
        ),
        Query::Membership { key } => Reply::Membership(MEMBERS.get(store, &key)?),
    })
}

fn memberships(store: &impl Reads) -> Result<Vec<Membership>, Error> {
    Ok(MEMBERS
        .all(store)?
        .into_iter()
        .map(|(_, membership)| membership)
        .collect())
}

fn set(store: &mut impl Writes, membership: Membership) -> Result<(), Error> {
    let key_is_ed25519 = membership.key.len() == KEY_LEN;
    if !key_is_ed25519 {
        return Err(invalid("a member key is a 32-byte ed25519 public key"));
    }
    let demotes = membership.role == Role::Resident;
    if demotes {
        unseat(store, &membership.key)?;
    }
    MEMBERS.put(store, &membership.key, &membership);
    Ok(())
}

fn remove(store: &mut impl Writes, member: &Vec<u8>) -> Result<(), Error> {
    unseat(store, member)?;
    MEMBERS.remove(store, member);
    Ok(())
}

fn unseat(store: &impl Reads, member: &Vec<u8>) -> Result<(), Error> {
    let seated = MEMBERS
        .get(store, member)?
        .is_some_and(|membership| membership.role == Role::Validator);
    if !seated {
        return Ok(());
    }
    let other_validators = memberships(store)?
        .iter()
        .any(|membership| membership.role == Role::Validator && &membership.key != member);
    if !other_validators {
        return Err(wrong_state("the last validator cannot be unseated"));
    }
    Ok(())
}
