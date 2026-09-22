// The rules over any store: the signer resolved to its account, then the ops and queries.

use abi::{Env, Origin, Refusal, Scheme};
use module_registry::helpers;
use store::{
    Item, Map, Reads, Set, Writes, already_exists, invalid, not_found, unauthorized, wrong_state,
};

use crate::{
    Account, AccountNumber, Admission, CONSENT_NAMESPACE, Consent, Control, Key, Op, Query,
    Reference, Reply, Standing,
};

const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a/");
const OF_KEY: Map<Vec<u8>, AccountNumber> = Map::new("k/");
const GENERATION: Map<Vec<u8>, u64> = Map::new("g/");
/// `(controller, controlled)`: the program accounts an account controls.
const CONTROLLED: Set<(AccountNumber, AccountNumber)> = Set::new("c/");
const NEXT: Item<AccountNumber> = Item::new("next");

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Refusal> {
    match op {
        Op::Create { name, scheme } => create(store, env, name, scheme),
        Op::AddKey {
            scheme,
            label,
            consent,
        } => add_key(store, env, scheme, label, consent),
        Op::RemoveKey { key } => remove_key(store, env, &key),
        Op::SetName { account, name } => set_name(store, env, account, name),
        Op::SetProfile {
            account,
            avatar,
            bio,
        } => set_profile(store, env, account, avatar, bio),
        Op::CreateProgram { name, controller } => create_program(store, env, name, controller),
        Op::SetStanding { account, standing } => set_standing(store, env, account, standing),
        Op::TransferControl { account, to } => transfer_control(store, env, account, to),
        Op::Revoke { account } => revoke(store, env, account),
    }
}

pub fn query(store: &impl Reads, env: &Env, query: Query) -> Result<Reply, Refusal> {
    Ok(match query {
        Query::Get { number } => Reply::Account(ACCOUNTS.get(store, &number)?),
        Query::OfKey { key } => Reply::Number(OF_KEY.get(store, &key)?),
        Query::Generation { key } => Reply::Generation(generation(store, &key)?),
        Query::Resolve { references } => Reply::Resolved(
            references
                .iter()
                .map(|reference| resolve(store, reference))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Query::List { page } => Reply::Accounts(
            ACCOUNTS
                .range(store, &page, env.height)?
                .map(|(_, account)| account),
        ),
        Query::Controlled { by, page } => Reply::Accounts(
            CONTROLLED
                .range_of(store, &by, &page, env.height)?
                .try_map(|(_, number)| account(store, number))?,
        ),
    })
}

fn account(store: &impl Reads, number: AccountNumber) -> Result<Account, Refusal> {
    ACCOUNTS
        .get(store, &number)?
        .ok_or_else(|| not_found(format!("account {number}")))
}

fn resolve(store: &impl Reads, reference: &Reference) -> Result<Option<AccountNumber>, Refusal> {
    match reference {
        Reference::Account(number) => Ok(ACCOUNTS.has(store, number).then_some(*number)),
        Reference::Key(key) => OF_KEY.get(store, key),
    }
}

fn generation(store: &impl Reads, key: &Vec<u8>) -> Result<u64, Refusal> {
    Ok(GENERATION.get(store, key)?.unwrap_or(0))
}

fn next_number(store: &mut impl Writes) -> Result<AccountNumber, Refusal> {
    let number = NEXT.get(store)?.unwrap_or(1);
    NEXT.put(store, &(number + 1));
    Ok(number)
}

fn admit_key(store: &mut impl Writes, key: &Vec<u8>, number: AccountNumber) -> Result<(), Refusal> {
    if OF_KEY.has(store, key) {
        return Err(already_exists("this key already belongs to an account"));
    }
    OF_KEY.put(store, key, &number);
    GENERATION.put(store, key, &(generation(store, key)? + 1));
    Ok(())
}

fn named(name: String) -> Result<String, Refusal> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
}

fn create(store: &mut impl Writes, env: &Env, name: String, scheme: Scheme) -> Result<(), Refusal> {
    let signer = helpers::external(env)?;
    let number = next_number(store)?;
    admit_key(store, &signer, number)?;
    ACCOUNTS.put(
        store,
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
    store.output(abi::encode(&number));
    Ok(())
}

fn add_key(
    store: &mut impl Writes,
    env: &Env,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Refusal> {
    let signer = helpers::external(env)?;
    let mut account = account(store, consent.account)?;
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
        generation: generation(store, &signer)?,
        account: consent.account,
        expires_at: consent.expires_at,
    };
    let consented = store.verify(
        authorizer.scheme,
        consent.key.clone(),
        CONSENT_NAMESPACE,
        admission.preimage(),
        consent.proof,
    )?;
    if !consented {
        return Err(unauthorized("the consent does not verify"));
    }
    admit_key(store, &signer, consent.account)?;
    keys.push(Key {
        scheme,
        key: signer,
        label,
        added_at: env.time,
    });
    keys.sort_by(|a, b| a.key.cmp(&b.key));
    account.updated_at = env.time;
    ACCOUNTS.put(store, &account.number, &account);
    Ok(())
}

fn remove_key(store: &mut impl Writes, env: &Env, key: &Vec<u8>) -> Result<(), Refusal> {
    let signer = helpers::external(env)?;
    let mut account = account_of_key(store, &signer)?;
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
    OF_KEY.remove(store, key);
    account.updated_at = env.time;
    ACCOUNTS.put(store, &account.number, &account);
    Ok(())
}

fn account_of_key(store: &impl Reads, key: &Vec<u8>) -> Result<Account, Refusal> {
    let number = OF_KEY
        .get(store, key)?
        .ok_or_else(|| unauthorized("this key holds no account"))?;
    account(store, number)
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
    store: &mut impl Writes,
    env: &Env,
    number: AccountNumber,
    name: String,
) -> Result<(), Refusal> {
    let mut account = account(store, number)?;
    acts_for(env, &account)?;
    account.name = named(name)?;
    account.updated_at = env.time;
    ACCOUNTS.put(store, &number, &account);
    Ok(())
}

fn set_profile(
    store: &mut impl Writes,
    env: &Env,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Refusal> {
    let mut account = account(store, number)?;
    acts_for(env, &account)?;
    account.avatar = avatar;
    account.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    account.updated_at = env.time;
    ACCOUNTS.put(store, &number, &account);
    Ok(())
}

fn create_program(
    store: &mut impl Writes,
    env: &Env,
    name: String,
    controller: AccountNumber,
) -> Result<(), Refusal> {
    let executor = helpers::program(env)?;
    let controlling = account(store, controller)?;
    if !controlling.live() {
        return Err(wrong_state(format!("account {controller} is not live")));
    }
    let number = next_number(store)?;
    ACCOUNTS.put(
        store,
        &number,
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
    CONTROLLED.insert(store, &(controller, number));
    store.output(abi::encode(&number));
    Ok(())
}

fn set_standing(
    store: &mut impl Writes,
    env: &Env,
    number: AccountNumber,
    standing: Standing,
) -> Result<(), Refusal> {
    let program = helpers::program(env)?;
    let mut account = account(store, number)?;
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
    ACCOUNTS.put(store, &number, &account);
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

fn controls(store: &impl Reads, env: &Env, controlled: &Account) -> Result<AccountNumber, Refusal> {
    let controller = controller_of(controlled)?;
    acts_for(env, &account(store, controller)?)?;
    Ok(controller)
}

fn transfer_control(
    store: &mut impl Writes,
    env: &Env,
    number: AccountNumber,
    to: AccountNumber,
) -> Result<(), Refusal> {
    let mut account = account(store, number)?;
    let controller = controls(store, env, &account)?;
    let target = self::account(store, to)?;
    if !target.live() {
        return Err(wrong_state(format!("account {to} is not live")));
    }
    let circular = to == number || ancestry(store, to)?.contains(&number);
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
    CONTROLLED.remove(store, &(controller, number));
    CONTROLLED.insert(store, &(to, number));
    account.updated_at = env.time;
    ACCOUNTS.put(store, &number, &account);
    Ok(())
}

fn ancestry(store: &impl Reads, mut number: AccountNumber) -> Result<Vec<AccountNumber>, Refusal> {
    let mut ancestors = Vec::new();
    loop {
        let Control::Program { controller, .. } = account(store, number)?.control else {
            return Ok(ancestors);
        };
        ancestors.push(controller);
        number = controller;
    }
}

fn revoke(store: &mut impl Writes, env: &Env, number: AccountNumber) -> Result<(), Refusal> {
    let mut account = account(store, number)?;
    let controller = controls(store, env, &account)?;
    account.control = Control::Revoked { controller };
    account.updated_at = env.time;
    ACCOUNTS.put(store, &number, &account);
    Ok(())
}
