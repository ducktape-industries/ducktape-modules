// The rules: who acts as an account, then the ops and queries.

use guest::{
    Error, ExecCtx, ModuleId, Origin, QueryCtx, Scheme, already_exists, invalid, not_found,
    unauthorized, wrong_state,
};
use store::{Item, Map, PageRequest, Set};

use crate::{
    Account, AccountNumber, Admission, CONSENT_NAMESPACE, Category, Consent, Key, Reference, Reply,
    Status,
};

pub(crate) const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a/");
pub(crate) const OF_KEY: Map<Vec<u8>, AccountNumber> = Map::new("k/");
pub(crate) const OF_MODULE: Map<ModuleId, AccountNumber> = Map::new("m/");
const GENERATION: Map<Vec<u8>, u64> = Map::new("g/");
/// `(manager, managed)`: the agents an account manages.
pub(crate) const MANAGED: Set<(AccountNumber, AccountNumber)> = Set::new("d/");
const NEXT: Item<AccountNumber> = Item::new("next");

/// The refusal a key meets while its account does not act.
pub(crate) const SUSPENDED: &str = "this account is suspended";

pub(crate) fn account(ctx: &QueryCtx, number: AccountNumber) -> Result<Account, Error> {
    ACCOUNTS
        .get(ctx, &number)?
        .ok_or_else(|| not_found(format!("account {number}")))
}

/// An account acts while it is active and so is its manager.
pub(crate) fn live(ctx: &QueryCtx, account: &Account) -> Result<bool, Error> {
    if account.status != Status::Active {
        return Ok(false);
    }
    match account.manager {
        Some(manager) => Ok(self::account(ctx, manager)?.status == Status::Active),
        None => Ok(true),
    }
}

/// The account a frame signed by `key` acts as: refused while it is not
/// live, so the host rejects the frame.
pub(crate) fn of_key(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Option<AccountNumber>, Error> {
    let Some(number) = OF_KEY.get(ctx, key)? else {
        return Ok(None);
    };
    if !live(ctx, &account(ctx, number)?)? {
        return Err(unauthorized(SUSPENDED));
    }
    Ok(Some(number))
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

/// A fresh account: active, no keys, no profile.
fn fresh(ctx: &ExecCtx, number: AccountNumber, name: String) -> Account {
    Account {
        number,
        name,
        avatar: None,
        bio: None,
        updated_at: ctx.env().time,
        keys: Vec::new(),
        module: None,
        manager: None,
        status: Status::Active,
        category: None,
    }
}

/// The kernel, admitting `module`, gives it its account, named after it.
/// A module that has one keeps it.
pub(crate) fn register_module(ctx: &ExecCtx, module: ModuleId) -> Result<(), Error> {
    if ctx.env().origin != Origin::Root {
        return Err(unauthorized("only the system registers a module"));
    }
    if OF_MODULE.has(ctx, &module) {
        return Ok(());
    }
    let number = next_number(ctx)?;
    let account = Account {
        module: Some(module.clone()),
        ..fresh(ctx, number, named(module.clone())?)
    };
    ACCOUNTS.put(ctx, &number, &account);
    OF_MODULE.put(ctx, &module, &number);
    Ok(())
}

pub(crate) fn create(ctx: &ExecCtx, name: String, scheme: Scheme) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let number = next_number(ctx)?;
    admit_key(ctx, &signer, number)?;
    let key = Key {
        scheme,
        key: signer,
        label: None,
        added_at: env.time,
    };
    let account = Account {
        keys: vec![key],
        ..fresh(ctx, number, named(name)?)
    };
    ACCOUNTS.put(ctx, &number, &account);
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// The account the frame acts as, which must be a person's.
fn person(ctx: &ExecCtx) -> Result<Account, Error> {
    let number = acting(ctx)?;
    let account = account(ctx, number)?;
    if !account.is_person() {
        return Err(unauthorized(format!(
            "account {number} is not a person's: an agent or a module manages no one"
        )));
    }
    Ok(account)
}

/// The account number the frame acts as.
fn acting(ctx: &ExecCtx) -> Result<AccountNumber, Error> {
    ctx.sender()?
        .account()
        .ok_or_else(|| unauthorized("the system acts as no account"))
}

pub(crate) fn create_agent(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let manager = person(ctx)?.number;
    let number = next_number(ctx)?;
    let account = Account {
        manager: Some(manager),
        category: Some(Category::Agent),
        ..fresh(ctx, number, named(name)?)
    };
    ACCOUNTS.put(ctx, &number, &account);
    MANAGED.insert(ctx, &(manager, number));
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// Refused unless the frame acts as `account`'s manager.
fn managing(ctx: &ExecCtx, account: &Account) -> Result<(), Error> {
    let acting = acting(ctx)?;
    if account.manager != Some(acting) {
        return Err(unauthorized(format!(
            "account {acting} does not manage account {}",
            account.number
        )));
    }
    Ok(())
}

/// Refused unless the frame acts as `account` itself or as its manager.
fn acts_for(ctx: &ExecCtx, account: &Account) -> Result<(), Error> {
    let acting = acting(ctx)?;
    let acts = acting == account.number || account.manager == Some(acting);
    if !acts {
        return Err(unauthorized(format!(
            "account {acting} does not act for account {}",
            account.number
        )));
    }
    Ok(())
}

/// A person's new key signs the frame and a key already on her account
/// consents; an agent's manager signs the frame and the new key consents to
/// itself. Either way the consent proves a key agreed to the admission.
pub(crate) fn add_key(
    ctx: &ExecCtx,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let mut account = account(ctx, consent.account)?;
    if account.module.is_some() {
        return Err(wrong_state("a module's account holds no keys"));
    }
    let (key, consenting_scheme) = match account.manager {
        Some(_) => {
            managing(ctx, &account)?;
            (consent.key.clone(), scheme)
        }
        None => {
            let authorizer = account
                .keys
                .iter()
                .find(|key| key.key == consent.key)
                .ok_or_else(|| unauthorized("the consenting key is not on this account"))?;
            (signer, authorizer.scheme)
        }
    };
    let expired = env.time > consent.expires_at;
    if expired {
        return Err(unauthorized("the consent has expired"));
    }
    let admission = Admission {
        network: env.chain_id.clone(),
        scheme,
        key: key.clone(),
        generation: generation(ctx, &key)?,
        account: consent.account,
        expires_at: consent.expires_at,
    };
    let consented = ctx.verify(
        consenting_scheme,
        consent.key,
        CONSENT_NAMESPACE,
        admission.preimage(),
        consent.proof,
    )?;
    if !consented {
        return Err(unauthorized("the consent does not verify"));
    }
    admit_key(ctx, &key, consent.account)?;
    account.keys.push(Key {
        scheme,
        key,
        label,
        added_at: env.time,
    });
    account.keys.sort_by(|a, b| a.key.cmp(&b.key));
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &account.number, &account);
    Ok(())
}

/// A person removes her own keys, never a senior one nor her last; an
/// agent's manager removes any of its keys.
pub(crate) fn remove_key(ctx: &ExecCtx, number: AccountNumber, key: &Vec<u8>) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    let removed = account
        .keys
        .iter()
        .find(|held| &held.key == key)
        .ok_or_else(|| not_found("that key is not on this account"))?;
    if account.manager.is_some() {
        managing(ctx, &account)?;
    } else {
        let signer = env.signer()?;
        let remover_added_at = account
            .keys
            .iter()
            .find(|held| held.key == signer)
            .map(|held| held.added_at)
            .ok_or_else(|| unauthorized("the signer is not on this account"))?;
        if account.keys.len() == 1 {
            return Err(wrong_state("an account keeps its last key"));
        }
        if removed.added_at < remover_added_at {
            return Err(unauthorized("a key removes only itself or a junior key"));
        }
    }
    account.keys.retain(|held| &held.key != key);
    OF_KEY.remove(ctx, key);
    account.updated_at = env.time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn set_name(ctx: &ExecCtx, number: AccountNumber, name: String) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    acts_for(ctx, &account)?;
    account.name = named(name)?;
    account.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn set_profile(
    ctx: &ExecCtx,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    acts_for(ctx, &account)?;
    account.avatar = avatar;
    account.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    account.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn set_status(
    ctx: &ExecCtx,
    number: AccountNumber,
    status: Status,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    managing(ctx, &account)?;
    if account.status == Status::Revoked {
        return Err(wrong_state(format!("account {number} is revoked for good")));
    }
    account.status = status;
    account.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}

pub(crate) fn transfer_manager(
    ctx: &ExecCtx,
    number: AccountNumber,
    to: AccountNumber,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    managing(ctx, &account)?;
    let receiver = self::account(ctx, to)?;
    if !receiver.is_person() {
        return Err(wrong_state(format!("account {to} is not a person's")));
    }
    if !live(ctx, &receiver)? {
        return Err(wrong_state(format!("account {to} is not live")));
    }
    if let Some(from) = account.manager {
        MANAGED.remove(ctx, &(from, number));
    }
    MANAGED.insert(ctx, &(to, number));
    account.manager = Some(to);
    account.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &number, &account);
    Ok(())
}
