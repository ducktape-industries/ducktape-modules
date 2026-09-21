use abi::{Env, Refusal, Scan};
use guest::{Execute, Program, Query as QueryCtx, Reads};
use modules::admission;
use modules::program::{bytes_key, invalid, wrong_state};
use modules::valset::{Genesis, Membership, Op, Query, Reply, Standing};

const MEMBER: &str = "m/";
const KEY_LEN: usize = 32;

struct Valset;

fn key(member: &[u8]) -> Vec<u8> {
    bytes_key(MEMBER, member)
}

impl Program for Valset {
    fn init(ctx: &mut Execute, _env: &Env, params: &[u8]) -> Result<(), Refusal> {
        let genesis: Genesis = abi::decode(params)?;
        for member in genesis.validators {
            let membership = Membership {
                key: member.key,
                address: member.address,
                standing: Standing::Validator,
            };
            set(ctx, membership)?;
        }
        Ok(())
    }

    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        modules::program::from(env, admission::PROGRAM)?;
        match abi::decode(payload)? {
            Op::Set(membership) => set(ctx, membership),
            Op::Remove { key } => remove(ctx, &key),
        }
    }

    fn query(ctx: &mut QueryCtx, _env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = match abi::decode(request)? {
            Query::Validators => Reply::Validators(
                memberships(ctx)?
                    .into_iter()
                    .filter(|membership| membership.standing == Standing::Validator)
                    .map(|membership| membership.key)
                    .collect(),
            ),
            Query::Members => {
                Reply::Members(memberships(ctx)?.iter().map(Membership::member).collect())
            }
            Query::Memberships => Reply::Memberships(memberships(ctx)?),
            Query::Membership { key: member } => Reply::Membership(ctx.record(key(&member))?),
        };
        ctx.reply(&reply);
        Ok(())
    }
}

fn memberships(ctx: &impl Reads) -> Result<Vec<Membership>, Refusal> {
    Ok(ctx
        .records::<Membership>(Scan::prefix(MEMBER))?
        .into_iter()
        .map(|(_, membership)| membership)
        .collect())
}

fn set(ctx: &mut Execute, membership: Membership) -> Result<(), Refusal> {
    let key_is_ed25519 = membership.key.len() == KEY_LEN;
    if !key_is_ed25519 {
        return Err(invalid("a member key is a 32-byte ed25519 public key"));
    }
    let demotes = membership.standing == Standing::Resident;
    if demotes {
        unseat(ctx, &membership.key)?;
    }
    ctx.put(key(&membership.key), &membership);
    Ok(())
}

fn remove(ctx: &mut Execute, member: &[u8]) -> Result<(), Refusal> {
    unseat(ctx, member)?;
    ctx.delete(key(member));
    Ok(())
}

fn unseat(ctx: &impl Reads, member: &[u8]) -> Result<(), Refusal> {
    let seated = ctx
        .record::<Membership>(key(member))?
        .is_some_and(|membership| membership.standing == Standing::Validator);
    if !seated {
        return Ok(());
    }
    let other_validators = memberships(ctx)?
        .iter()
        .any(|membership| membership.standing == Standing::Validator && membership.key != member);
    if !other_validators {
        return Err(wrong_state("the last validator cannot be unseated"));
    }
    Ok(())
}

guest::program!(Valset);
