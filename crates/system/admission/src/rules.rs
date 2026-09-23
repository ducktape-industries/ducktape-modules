// The rules over any store: who may enroll, who may leave, and what a validators' vote enacts.

use abi::{Env, Refusal, Scheme};
use module_registry::helpers::external;
use store::{
    Item, Map, Reads, Set, Writes, capacity, invalid, not_found, unauthorized, wrong_state,
};
use valset::{MAX_MEMBERS, Membership, Standing};

use crate::{INVITE_NAMESPACE, Invite, Motion, Op, Voted};

const DOOR: Item<bool> = Item::new("door");
const SPENT: Set<Vec<u8>> = Set::new("spent/");
const VOTES: Map<Vec<u8>, Vec<Vec<u8>>> = Map::new("votes/");

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Refusal> {
    let signer = external(env)?;
    match op {
        Op::Enroll { address, invite } => enroll(store, env, signer, address, invite),
        Op::Leave => leave(store, signer),
        Op::Vote(motion) => vote(store, signer, motion),
    }
}

fn enroll(
    store: &mut impl Writes,
    env: &Env,
    key: Vec<u8>,
    address: String,
    invite: Option<Invite>,
) -> Result<(), Refusal> {
    let standing = match valset::membership(store, &key)? {
        Some(held) => held.standing,
        None => {
            admit(store, env, invite)?;
            Standing::Resident
        }
    };
    let membership = Membership {
        key,
        address,
        standing,
    };
    store.emit(valset::PROGRAM, abi::encode(&valset::Op::Set(membership)));
    Ok(())
}

fn admit(store: &mut impl Writes, env: &Env, invite: Option<Invite>) -> Result<(), Refusal> {
    let full = valset::members(store)?.len() >= MAX_MEMBERS;
    if full {
        return Err(capacity(format!(
            "the network holds its {MAX_MEMBERS} members"
        )));
    }
    if door_open(store)? {
        return Ok(());
    }
    let Some(invite) = invite else {
        return Err(unauthorized(
            "the door is closed: enrolling takes an invite",
        ));
    };
    redeem(store, env, &invite)
}

fn redeem(store: &mut impl Writes, env: &Env, invite: &Invite) -> Result<(), Refusal> {
    let grant = &invite.grant;
    let for_this_network = grant.network == env.network;
    if !for_this_network {
        return Err(invalid("the invite names another network"));
    }
    let expired = grant.expires <= env.time;
    if expired {
        return Err(invalid("the invite expired"));
    }
    let signed = store.verify(
        Scheme::Ed25519,
        invite.issuer.clone(),
        INVITE_NAMESPACE,
        grant.preimage(),
        invite.signature.clone(),
    )?;
    if !signed {
        return Err(unauthorized("the invite's signature does not verify"));
    }
    let issued_by_validator = valset::standing(store, &invite.issuer)? == Some(Standing::Validator);
    if !issued_by_validator {
        return Err(unauthorized("the invite's issuer is not a validator"));
    }
    let nonce = [invite.issuer.as_slice(), &grant.nonce].concat();
    if SPENT.has(store, &nonce) {
        return Err(wrong_state("the invite was already redeemed"));
    }
    SPENT.insert(store, &nonce);
    Ok(())
}

fn leave(store: &mut impl Writes, key: Vec<u8>) -> Result<(), Refusal> {
    let Some(held) = valset::membership(store, &key)? else {
        return Err(not_found("the signer is not a member"));
    };
    unseatable(store, &held)?;
    remove(store, key)
}

fn vote(store: &mut impl Writes, voter: Vec<u8>, motion: Motion) -> Result<(), Refusal> {
    let validators = valset::validators(store)?;
    let is_validator = validators.contains(&voter);
    if !is_validator {
        return Err(unauthorized("only a validator votes"));
    }
    admissible(store, &motion)?;
    let record = abi::encode(&motion);
    let mut voters = VOTES.get(store, &record)?.unwrap_or_default();
    voters.retain(|held| validators.contains(held));
    if !voters.contains(&voter) {
        voters.push(voter);
    }
    let needed = quorum(validators.len());
    let passed = voters.len() >= needed;
    if !passed {
        VOTES.put(store, &record, &voters);
        let counted = Voted::Counted {
            votes: voters.len() as u64,
            needed: needed as u64,
        };
        store.output(abi::encode(&counted));
        return Ok(());
    }
    VOTES.remove(store, &record);
    enact(store, motion)?;
    store.output(abi::encode(&Voted::Enacted));
    Ok(())
}

fn admissible(store: &impl Reads, motion: &Motion) -> Result<(), Refusal> {
    match motion {
        Motion::Promote { key } => promotable(store, key),
        Motion::Demote { key } => demotable(store, key),
        Motion::Remove { key } => removable(store, key),
        Motion::Door { open } => door_moves(store, *open),
    }
}

fn enact(store: &mut impl Writes, motion: Motion) -> Result<(), Refusal> {
    match motion {
        Motion::Promote { key } => reseat(store, key, Standing::Validator),
        Motion::Demote { key } => reseat(store, key, Standing::Resident),
        Motion::Remove { key } => remove(store, key),
        Motion::Door { open } => door(store, open),
    }
}

fn promotable(store: &impl Reads, key: &[u8]) -> Result<(), Refusal> {
    holding(store, key, Standing::Resident).map(drop)
}

fn demotable(store: &impl Reads, key: &[u8]) -> Result<(), Refusal> {
    let held = holding(store, key, Standing::Validator)?;
    unseatable(store, &held)
}

fn removable(store: &impl Reads, key: &[u8]) -> Result<(), Refusal> {
    let Some(held) = valset::membership(store, key)? else {
        return Err(not_found("the key is not a member"));
    };
    unseatable(store, &held)
}

fn holding(store: &impl Reads, key: &[u8], standing: Standing) -> Result<Membership, Refusal> {
    let Some(held) = valset::membership(store, key)? else {
        return Err(not_found("the key is not a member"));
    };
    let holds_it = held.standing == standing;
    if !holds_it {
        return Err(wrong_state(format!(
            "the member is a {:?}, not a {standing:?}",
            held.standing
        )));
    }
    Ok(held)
}

fn unseatable(store: &impl Reads, held: &Membership) -> Result<(), Refusal> {
    let seated = held.standing == Standing::Validator;
    let last = seated && valset::validators(store)?.len() == 1;
    if last {
        return Err(wrong_state("the last validator cannot be unseated"));
    }
    Ok(())
}

fn door_moves(store: &impl Reads, open: bool) -> Result<(), Refusal> {
    let unchanged = door_open(store)? == open;
    if unchanged {
        return Err(wrong_state(format!(
            "the door is already {}",
            door_word(open)
        )));
    }
    Ok(())
}

fn reseat(store: &mut impl Writes, key: Vec<u8>, standing: Standing) -> Result<(), Refusal> {
    let Some(held) = valset::membership(store, &key)? else {
        return Err(not_found("the key is not a member"));
    };
    let membership = Membership { standing, ..held };
    store.emit(valset::PROGRAM, abi::encode(&valset::Op::Set(membership)));
    Ok(())
}

fn remove(store: &mut impl Writes, key: Vec<u8>) -> Result<(), Refusal> {
    for motion in [
        Motion::Promote { key: key.clone() },
        Motion::Demote { key: key.clone() },
        Motion::Remove { key: key.clone() },
    ] {
        VOTES.remove(store, &abi::encode(&motion));
    }
    store.emit(valset::PROGRAM, abi::encode(&valset::Op::Remove { key }));
    Ok(())
}

fn door(store: &mut impl Writes, open: bool) -> Result<(), Refusal> {
    DOOR.put(store, &open);
    Ok(())
}

fn door_open(store: &impl Reads) -> Result<bool, Refusal> {
    Ok(DOOR.get(store)?.unwrap_or(false))
}

fn quorum(validators: usize) -> usize {
    validators - (validators - 1) / 3
}

fn door_word(open: bool) -> &'static str {
    match open {
        true => "open",
        false => "closed",
    }
}
