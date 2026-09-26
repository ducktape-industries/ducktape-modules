// The rules: the signer resolved to its account, then the ops and queries.

use guest::{Env, Origin, Scheme};
use guest::{
    Error, ExecCtx, QueryCtx, already_exists, invalid, not_found, unauthorized, wrong_state,
};
use store::{Item, Map, PageRequest, Set};

use crate::{
    Account, AccountNumber, Admission, CONSENT_NAMESPACE, Consent, Control, Key, Reference, Reply,
    Status,
};

pub(crate) const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a/");
pub(crate) const OF_KEY: Map<Vec<u8>, AccountNumber> = Map::new("k/");
const GENERATION: Map<Vec<u8>, u64> = Map::new("g/");
/// `(controller, controlled)`: the program accounts an account controls.
pub(crate) const CONTROLLED: Set<(AccountNumber, AccountNumber)> = Set::new("c/");
const NEXT: Item<AccountNumber> = Item::new("next");

pub(crate) fn account(ctx: &QueryCtx, number: AccountNumber) -> Result<Account, Error> {
    ACCOUNTS
        .get(ctx, &number)?
        .ok_or_else(|| not_found(format!("account {number}")))
}

pub(crate) fn resolve(
    ctx: &QueryCtx,
    reference: &Reference,
) -> Result<Option<AccountNumber>, Error> {
    match reference {
        Reference::Account(number) => Ok(ACCOUNTS.has(ctx, number).then_some(*number)),
        Reference::Key(key) => OF_KEY.get(ctx, key),
    }
}

/// A page of profiles past `after`, at most `limit` (capped as every page
/// is); `next` names the last one while more remain.
pub(crate) fn profiles(
    ctx: &QueryCtx,
    after: Option<AccountNumber>,
    limit: u32,
) -> Result<Reply, Error> {
    let limit = u64::from(limit).clamp(1, PageRequest::MAX_LIMIT);
    let mut range = ACCOUNTS.prefix_of(&());
    if let Some(after) = after {
        range = range.after(ACCOUNTS.key(&after));
    }
    let mut accounts = ACCOUNTS.scan(ctx, range.limit(limit + 1))?;
    let more = accounts.len() as u64 > limit;
    accounts.truncate(limit as usize);
    let next = accounts.last().filter(|_| more).map(|(number, _)| *number);
    let profiles = accounts
        .iter()
        .map(|(_, account)| account.profile())
        .collect();
    Ok(Reply::Profiles { profiles, next })
}

pub(crate) fn generation(ctx: &QueryCtx, key: &Vec<u8>) -> Result<u64, Error> {
    Ok(GENERATION.get(ctx, key)?.unwrap_or(0))
}

fn next_number(ctx: &ExecCtx) -> Result<AccountNumber, Error> {
    let number = NEXT.get(ctx)?.unwrap_or(1);
    NEXT.put(ctx, &(number + 1));
    Ok(number)
}

fn admit_key(ctx: &ExecCtx, key: &Vec<u8>, number: AccountNumber) -> Result<(), Error> {
    if OF_KEY.has(ctx, key) {
        return Err(already_exists("this key already belongs to an account"));
    }
    OF_KEY.put(ctx, key, &number);
    GENERATION.put(ctx, key, &(generation(ctx, key)? + 1));
    Ok(())
}

fn named(name: String) -> Result<String, Error> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
}

pub(crate) fn create(ctx: &ExecCtx, name: String, scheme: Scheme) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let number = next_number(ctx)?;
    admit_key(ctx, &signer, number)?;
    ACCOUNTS.put(
        ctx,
        &number,
        &Account {
            number,
            name: named(name)?,
            control: Control::Keys(vec![Key {
                scheme,
                key: signer,
                label: None,
                added_at: env.time,
            }]),
            avatar: None,
            bio: None,
            updated_at: env.time,
        },
    );
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

pub(crate) fn add_key(
    ctx: &ExecCtx,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let mut account = account(ctx, consent.account)?;
    let Control::Keys(keys) = &mut account.control else {
        return Err(wrong_state("a program account holds no keys"));
    };
    let authorizer = keys
        .iter()
        .find(|key| key.key == consent.key)
        .ok_or_else(|| unauthorized("the consenting key is not on this account"))?;
    let expired = env.time > consent.expires_at;
    if expired {
        return Err(unauthorized("the consent has expired"));
    }
    let admission = Admission {
        network: env.chain_id.clone(),
        scheme,
        key: signer.clone(),
        generation: generation(ctx, &signer)?,
        account: consent.account,
        expires_at: consent.expires_at,
    };
    let consented = ctx.verify(
        authorizer.scheme,
        consent.key.clone(),
        CONSENT_NAMESPACE,
        admission.preimage(),
        consent.proof,
    )?;
    if !consented {
        return Err(unauthorized("the consent does not verify"));
    }
    admit_key(ctx, &signer, consent.account)?;
    keys.push(Key {
        scheme,
        key: signer,
        label,
        added_at: env.time,
    });
    keys.sort_by(|a, b| a.key.cmp(&b.key));
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &account.number, &account);
    Ok(())
}

pub(crate) fn remove_key(ctx: &ExecCtx, key: &Vec<u8>) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let mut account = account_of_key(ctx, &signer)?;
    let Control::Keys(keys) = &mut account.control else {
        return Err(wrong_state("a program account holds no keys"));
    };
    let remover_added_at = keys
        .iter()
        .find(|held| held.key == signer)
        .map(|held| held.added_at)
        .ok_or_else(|| unauthorized("the signer is not on this account"))?;
    let removed = keys
        .iter()
        .find(|held| &held.key == key)
        .ok_or_else(|| not_found("that key is not on this account"))?;
    let last = keys.len() == 1;
    if last {
        return Err(wrong_state("an account keeps its last key"));
    }
    let senior = removed.added_at < remover_added_at;
    if senior {
        return Err(unauthorized("a key removes only itself or a junior key"));
    }
    keys.retain(|held| &held.key != key);
    OF_KEY.remove(ctx, key);
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &account.number, &account);
    Ok(())
}

fn account_of_key(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Account, Error> {
    let number = OF_KEY
        .get(ctx, key)?
        .ok_or_else(|| unauthorized("this key holds no account"))?;
    account(ctx, number)
}

fn acts_for(env: &Env, account: &Account) -> Result<(), Error> {
    let acts = match (&env.origin, &account.control) {
        (Origin::Signed(key), Control::Keys(_)) => account.holds(key),
        (Origin::Module(program), Control::Program { executor, .. }) => program == executor,
        _ => false,
    };
    if !acts {
        return Err(unauthorized(format!(
            "{:?} does not act for account {}",
            env.origin, account.number
        )));
    }
    Ok(())
}

pub(crate) fn set_name(ctx: &ExecCtx, number: AccountNumber, name: String) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    acts_for(env, &account)?;
    account.name = named(name)?;
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn set_profile(
    ctx: &ExecCtx,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    acts_for(env, &account)?;
    account.avatar = avatar;
    account.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn create_program(
    ctx: &ExecCtx,
    name: String,
    controller: AccountNumber,
) -> Result<(), Error> {
    let env = ctx.env();
    let executor = env.sending_module()?;
    let controlling = account(ctx, controller)?;
    if !controlling.live() {
        return Err(wrong_state(format!("account {controller} is not live")));
    }
    let number = next_number(ctx)?;
    ACCOUNTS.put(
        ctx,
        &number,
        &Account {
            number,
            name: named(name)?,
            control: Control::Program {
                executor,
                controller,
                status: Status::Active,
            },
            avatar: None,
            bio: None,
            updated_at: env.time,
        },
    );
    CONTROLLED.insert(ctx, &(controller, number));
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

pub(crate) fn set_status(
    ctx: &ExecCtx,
    number: AccountNumber,
    status: Status,
) -> Result<(), Error> {
    let env = ctx.env();
    let program = env.sending_module()?;
    let mut account = account(ctx, number)?;
    let Control::Program {
        executor,
        status: current,
        ..
    } = &mut account.control
    else {
        return Err(wrong_state(format!(
            "account {number} is not a program account"
        )));
    };
    let executes = *executor == program;
    if !executes {
        return Err(unauthorized(format!(
            "{program} does not execute account {number}"
        )));
    }
    *current = status;
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

fn controller_of(account: &Account) -> Result<AccountNumber, Error> {
    match &account.control {
        Control::Program { controller, .. } => Ok(*controller),
        Control::Keys(_) | Control::Revoked { .. } => Err(wrong_state(format!(
            "account {} is not a live program account",
            account.number
        ))),
    }
}

fn controls(ctx: &QueryCtx, controlled: &Account) -> Result<AccountNumber, Error> {
    let env = ctx.env();
    let controller = controller_of(controlled)?;
    acts_for(env, &account(ctx, controller)?)?;
    Ok(controller)
}

pub(crate) fn transfer_control(
    ctx: &ExecCtx,
    number: AccountNumber,
    to: AccountNumber,
) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    let controller = controls(ctx, &account)?;
    let target = self::account(ctx, to)?;
    if !target.live() {
        return Err(wrong_state(format!("account {to} is not live")));
    }
    let circular = to == number || ancestry(ctx, to)?.contains(&number);
    if circular {
        return Err(wrong_state(format!(
            "account {to} is controlled through account {number}"
        )));
    }
    let Control::Program {
        controller: current,
        ..
    } = &mut account.control
    else {
        return Err(wrong_state(format!(
            "account {number} is not a program account"
        )));
    };
    *current = to;
    CONTROLLED.remove(ctx, &(controller, number));
    CONTROLLED.insert(ctx, &(to, number));
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

fn ancestry(ctx: &QueryCtx, mut number: AccountNumber) -> Result<Vec<AccountNumber>, Error> {
    let mut ancestors = Vec::new();
    loop {
        let Control::Program { controller, .. } = account(ctx, number)?.control else {
            return Ok(ancestors);
        };
        ancestors.push(controller);
        number = controller;
    }
}

pub(crate) fn revoke(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    let controller = controls(ctx, &account)?;
    account.control = Control::Revoked { controller };
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}
