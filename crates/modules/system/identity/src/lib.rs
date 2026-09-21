use abi::{Env, Origin, Refusal, Scheme};
use guest::{Execute, Program, Query as QueryCtx, Reads};
use modules::AccountNumber;
use modules::identity::{
    Account, Admission, CONSENT_NAMESPACE, Consent, Control, Key, Op, Query, Reference, Reply,
    Standing,
};
use modules::program::{
    already_exists, bytes_key, invalid, not_found, u64_key, unauthorized, wrong_state,
};

const ACCOUNT: &str = "a/";
const OF_KEY: &str = "k/";
const GENERATION: &str = "g/";
const CONTROLLED: &str = "c/";
const NEXT: &[u8] = b"next";

struct Identity;

fn account_key(number: AccountNumber) -> Vec<u8> {
    u64_key(ACCOUNT, number)
}

fn of_key(key: &[u8]) -> Vec<u8> {
    bytes_key(OF_KEY, key)
}

fn generation_key(key: &[u8]) -> Vec<u8> {
    bytes_key(GENERATION, key)
}

fn controlled_prefix(by: AccountNumber) -> Vec<u8> {
    let mut key = u64_key(CONTROLLED, by);
    key.push(b'/');
    key
}

fn controlled_key(by: AccountNumber, number: AccountNumber) -> Vec<u8> {
    let mut key = controlled_prefix(by);
    key.extend_from_slice(&number.to_be_bytes());
    key
}

impl Program for Identity {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        match abi::decode(payload)? {
            Op::Create { name, scheme } => create(ctx, env, name, scheme),
            Op::AddKey {
                scheme,
                label,
                consent,
            } => add_key(ctx, env, scheme, label, consent),
            Op::RemoveKey { key } => remove_key(ctx, env, &key),
            Op::SetName { account, name } => set_name(ctx, env, account, name),
            Op::SetProfile {
                account,
                avatar,
                bio,
            } => set_profile(ctx, env, account, avatar, bio),
            Op::CreateProgram { name, controller } => create_program(ctx, env, name, controller),
            Op::SetStanding { account, standing } => set_standing(ctx, env, account, standing),
            Op::TransferControl { account, to } => transfer_control(ctx, env, account, to),
            Op::Revoke { account } => revoke(ctx, env, account),
        }
    }

    fn query(ctx: &mut QueryCtx, _env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = match abi::decode(request)? {
            Query::Get { number } => Reply::Account(ctx.record(account_key(number))?),
            Query::OfKey { key } => Reply::Number(ctx.record(of_key(&key))?),
            Query::Generation { key } => Reply::Generation(generation(ctx, &key)?),
            Query::Resolve { references } => Reply::Resolved(
                references
                    .iter()
                    .map(|reference| resolve(ctx, reference))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Query::List { page } => Reply::Accounts(
                ctx.records::<Account>(page.scan(ACCOUNT.as_bytes()))?
                    .into_iter()
                    .map(|(_, account)| account)
                    .collect(),
            ),
            Query::Controlled { by, page } => {
                let keys = ctx.scan(page.scan(&controlled_prefix(by)));
                let mut accounts = Vec::new();
                for entry in keys {
                    let number = u64::from_be_bytes(
                        entry.key[entry.key.len() - 8..]
                            .try_into()
                            .map_err(|_| invalid("a controlled key names no account"))?,
                    );
                    accounts.push(account(ctx, number)?);
                }
                Reply::Accounts(accounts)
            }
        };
        ctx.reply(&reply);
        Ok(())
    }
}

fn account(ctx: &impl Reads, number: AccountNumber) -> Result<Account, Refusal> {
    ctx.record(account_key(number))?
        .ok_or_else(|| not_found(format!("account {number}")))
}

fn resolve(ctx: &impl Reads, reference: &Reference) -> Result<Option<AccountNumber>, Refusal> {
    match reference {
        Reference::Account(number) => {
            Ok(ctx.get(account_key(*number)).is_some().then_some(*number))
        }
        Reference::Key(key) => ctx.record(of_key(key)),
    }
}

fn generation(ctx: &impl Reads, key: &[u8]) -> Result<u64, Refusal> {
    Ok(ctx.record(generation_key(key))?.unwrap_or(0))
}

fn next_number(ctx: &mut Execute) -> Result<AccountNumber, Refusal> {
    let number: AccountNumber = ctx.record(NEXT)?.unwrap_or(1);
    ctx.put(NEXT, &(number + 1));
    Ok(number)
}

fn store(ctx: &mut Execute, account: &Account) {
    ctx.put(account_key(account.number), account);
}

fn admit_key(ctx: &mut Execute, key: &[u8], number: AccountNumber) -> Result<(), Refusal> {
    let held = ctx.get(of_key(key)).is_some();
    if held {
        return Err(already_exists("this key already belongs to an account"));
    }
    ctx.put(of_key(key), &number);
    ctx.put(generation_key(key), &(generation(ctx, key)? + 1));
    Ok(())
}

fn named(name: String) -> Result<String, Refusal> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
}

fn create(ctx: &mut Execute, env: &Env, name: String, scheme: Scheme) -> Result<(), Refusal> {
    let signer = modules::program::external(env)?;
    let number = next_number(ctx)?;
    admit_key(ctx, &signer, number)?;
    store(
        ctx,
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
    ctx.output(abi::encode(&number));
    Ok(())
}

fn add_key(
    ctx: &mut Execute,
    env: &Env,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Refusal> {
    let signer = modules::program::external(env)?;
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
        network: env.network.clone(),
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
    store(ctx, &account);
    Ok(())
}

fn remove_key(ctx: &mut Execute, env: &Env, key: &[u8]) -> Result<(), Refusal> {
    let signer = modules::program::external(env)?;
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
        .find(|held| held.key == key)
        .ok_or_else(|| not_found("that key is not on this account"))?;
    let last = keys.len() == 1;
    if last {
        return Err(wrong_state("an account keeps its last key"));
    }
    let senior = removed.added_at < remover_added_at;
    if senior {
        return Err(unauthorized("a key removes only itself or a junior key"));
    }
    keys.retain(|held| held.key != key);
    ctx.delete(of_key(key));
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

fn account_of_key(ctx: &impl Reads, key: &[u8]) -> Result<Account, Refusal> {
    let number: AccountNumber = ctx
        .record(of_key(key))?
        .ok_or_else(|| unauthorized("this key holds no account"))?;
    account(ctx, number)
}

fn acts_for(env: &Env, account: &Account) -> Result<(), Refusal> {
    let acts = match (&env.origin, &account.control) {
        (Origin::External(key), Control::Keys(_)) => account.holds(key),
        (Origin::Program(program), Control::Program { executor, .. }) => program == executor,
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

fn set_name(
    ctx: &mut Execute,
    env: &Env,
    number: AccountNumber,
    name: String,
) -> Result<(), Refusal> {
    let mut account = account(ctx, number)?;
    acts_for(env, &account)?;
    account.name = named(name)?;
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

fn set_profile(
    ctx: &mut Execute,
    env: &Env,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Refusal> {
    let mut account = account(ctx, number)?;
    acts_for(env, &account)?;
    account.avatar = avatar;
    account.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

fn create_program(
    ctx: &mut Execute,
    env: &Env,
    name: String,
    controller: AccountNumber,
) -> Result<(), Refusal> {
    let executor = modules::program::program(env)?;
    let controlling = account(ctx, controller)?;
    if !controlling.live() {
        return Err(wrong_state(format!("account {controller} is not live")));
    }
    let number = next_number(ctx)?;
    store(
        ctx,
        &Account {
            number,
            name: named(name)?,
            control: Control::Program {
                executor,
                controller,
                standing: Standing::Active,
            },
            avatar: None,
            bio: None,
            updated_at: env.time,
        },
    );
    ctx.set(controlled_key(controller, number), Vec::new());
    ctx.output(abi::encode(&number));
    Ok(())
}

fn set_standing(
    ctx: &mut Execute,
    env: &Env,
    number: AccountNumber,
    standing: Standing,
) -> Result<(), Refusal> {
    let program = modules::program::program(env)?;
    let mut account = account(ctx, number)?;
    let Control::Program {
        executor,
        standing: current,
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
    *current = standing;
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

fn controller_of(account: &Account) -> Result<AccountNumber, Refusal> {
    match &account.control {
        Control::Program { controller, .. } => Ok(*controller),
        Control::Keys(_) | Control::Revoked { .. } => Err(wrong_state(format!(
            "account {} is not a live program account",
            account.number
        ))),
    }
}

fn controls(ctx: &impl Reads, env: &Env, controlled: &Account) -> Result<AccountNumber, Refusal> {
    let controller = controller_of(controlled)?;
    acts_for(env, &account(ctx, controller)?)?;
    Ok(controller)
}

fn transfer_control(
    ctx: &mut Execute,
    env: &Env,
    number: AccountNumber,
    to: AccountNumber,
) -> Result<(), Refusal> {
    let mut account = account(ctx, number)?;
    let controller = controls(ctx, env, &account)?;
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
    ctx.delete(controlled_key(controller, number));
    ctx.set(controlled_key(to, number), Vec::new());
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

fn ancestry(ctx: &impl Reads, mut number: AccountNumber) -> Result<Vec<AccountNumber>, Refusal> {
    let mut ancestors = Vec::new();
    loop {
        let Control::Program { controller, .. } = account(ctx, number)?.control else {
            return Ok(ancestors);
        };
        ancestors.push(controller);
        number = controller;
    }
}

fn revoke(ctx: &mut Execute, env: &Env, number: AccountNumber) -> Result<(), Refusal> {
    let mut account = account(ctx, number)?;
    let controller = controls(ctx, env, &account)?;
    account.control = Control::Revoked { controller };
    account.updated_at = env.time;
    store(ctx, &account);
    Ok(())
}

guest::program!(Identity);
