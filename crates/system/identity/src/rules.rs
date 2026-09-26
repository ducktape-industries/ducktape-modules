// The rules: who acts as an account, then the ops and queries.

use guest::{
    Error, ExecCtx, ModuleId, Origin, QueryCtx, Scheme, already_exists, invalid, not_found,
    unauthorized, wrong_state,
};
use store::{Item, Map, PageRequest, Set};

use crate::{
    Account, AccountNumber, Admission, CONSENT_NAMESPACE, Card, Category, Consent, Control, Key,
    Life, Reference, Reply,
};

pub(crate) const ACCOUNTS: Map<AccountNumber, Account> = Map::new("a/");
pub(crate) const OF_KEY: Map<Vec<u8>, AccountNumber> = Map::new("k/");
pub(crate) const OF_MODULE: Map<ModuleId, AccountNumber> = Map::new("m/");
const GENERATION: Map<Vec<u8>, u64> = Map::new("g/");
/// `(manager, managed)`: the agents an account manages, revoked ones too,
/// so a manager's list keeps what they answered for.
pub(crate) const MANAGED: Set<(AccountNumber, AccountNumber)> = Set::new("d/");
const NEXT: Item<AccountNumber> = Item::new("next");

pub(crate) fn account(ctx: &QueryCtx, number: AccountNumber) -> Result<Account, Error> {
    ACCOUNTS
        .get(ctx, &number)?
        .ok_or_else(|| not_found(format!("account {number}")))
}

/// The account a frame signed by `key` acts as: refused while it does not
/// act, so the host rejects the frame. A person and a module always act;
/// an agent while its manager keeps it active (a revoked one holds no key
/// to be asked about).
pub(crate) fn of_key(ctx: &QueryCtx, key: &Vec<u8>) -> Result<Option<AccountNumber>, Error> {
    let Some(number) = OF_KEY.get(ctx, key)? else {
        return Ok(None);
    };
    match account(ctx, number)?.control {
        Control::Person { .. }
        | Control::Module { .. }
        | Control::Managed {
            life: Life::Active { .. },
            ..
        } => Ok(Some(number)),
        Control::Managed { .. } => Err(unauthorized(format!("account {number} is suspended"))),
    }
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

/// Forgets every key in `keys`: the account no longer holds them. Their
/// generations stay, so no old consent admits one again.
fn drop_keys(ctx: &ExecCtx, keys: &[Key]) {
    for key in keys {
        OF_KEY.remove(ctx, &key.key);
    }
}

fn named(name: String) -> Result<String, Error> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
}

/// A fresh card: a name, nothing else yet.
fn card(ctx: &ExecCtx, name: String) -> Result<Card, Error> {
    Ok(Card {
        name: named(name)?,
        avatar: None,
        bio: None,
        updated_at: ctx.env().time,
    })
}

fn save(ctx: &ExecCtx, mut account: Account) {
    account.card.updated_at = ctx.env().time;
    ACCOUNTS.put(ctx, &account.number, &account);
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
    let card = card(ctx, module.clone())?;
    let number = next_number(ctx)?;
    let account = Account {
        number,
        card,
        control: Control::Module {
            module: module.clone(),
        },
    };
    ACCOUNTS.put(ctx, &number, &account);
    OF_MODULE.put(ctx, &module, &number);
    Ok(())
}

pub(crate) fn create(ctx: &ExecCtx, name: String, scheme: Scheme) -> Result<(), Error> {
    let env = ctx.env();
    let signer = env.signer()?;
    let card = card(ctx, name)?;
    if OF_KEY.has(ctx, &signer) {
        return Err(already_exists("this key already belongs to an account"));
    }
    let number = next_number(ctx)?;
    admit_key(ctx, &signer, number)?;
    let key = Key {
        scheme,
        key: signer,
        label: None,
        added_at: env.time,
    };
    let account = Account {
        number,
        card,
        control: Control::Person { keys: vec![key] },
    };
    ACCOUNTS.put(ctx, &number, &account);
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// The account number the frame acts as.
fn acting(ctx: &ExecCtx) -> Result<AccountNumber, Error> {
    ctx.sender()?
        .account()
        .ok_or_else(|| unauthorized("the system acts as no account"))
}

/// `number`'s account, which must be a person's: the one kind that
/// manages.
fn person(ctx: &ExecCtx, number: AccountNumber) -> Result<Account, Error> {
    let account = account(ctx, number)?;
    match account.control {
        Control::Person { .. } => Ok(account),
        Control::Managed { .. } | Control::Module { .. } => Err(unauthorized(format!(
            "account {number} is not a person's: an agent or a module manages no one"
        ))),
    }
}

pub(crate) fn create_agent(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    let manager = person(ctx, acting(ctx)?)?.number;
    let card = card(ctx, name)?;
    let number = next_number(ctx)?;
    let account = Account {
        number,
        card,
        control: Control::Managed {
            manager,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
        },
    };
    ACCOUNTS.put(ctx, &number, &account);
    MANAGED.insert(ctx, &(manager, number));
    ctx.set_return_data(abi::encode(&number));
    Ok(())
}

/// Refused unless the frame acts as `manager`, the manager of `number`.
fn managing(ctx: &ExecCtx, manager: AccountNumber, number: AccountNumber) -> Result<(), Error> {
    let acting = acting(ctx)?;
    if acting != manager {
        return Err(unauthorized(format!(
            "account {acting} does not manage account {number}"
        )));
    }
    Ok(())
}

/// The refusal every op on a revoked agent meets.
fn revoked(number: AccountNumber) -> Error {
    wrong_state(format!("account {number} is revoked for good"))
}

/// A person's new key signs the frame and a key already on their account
/// consents; an agent's manager signs the frame and the new key consents to
/// itself. Either way the consent proves a key agreed to the admission. A
/// module's account holds no keys; a revoked agent takes none.
pub(crate) fn add_key(
    ctx: &ExecCtx,
    scheme: Scheme,
    label: Option<String>,
    consent: Consent,
) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, consent.account)?;
    let (keys, key, consenting_scheme) = match &mut account.control {
        Control::Person { keys } => {
            let authorizer = keys
                .iter()
                .find(|key| key.key == consent.key)
                .ok_or_else(|| unauthorized("the consenting key is not on this account"))?;
            let consenting_scheme = authorizer.scheme;
            (keys, env.signer()?, consenting_scheme)
        }
        Control::Managed {
            manager,
            life: Life::Active { keys } | Life::Suspended { keys },
            ..
        } => {
            managing(ctx, *manager, consent.account)?;
            (keys, consent.key.clone(), scheme)
        }
        Control::Managed {
            manager,
            life: Life::Revoked,
            ..
        } => {
            managing(ctx, *manager, consent.account)?;
            return Err(revoked(consent.account));
        }
        Control::Module { .. } => {
            return Err(wrong_state("a module's account holds no keys"));
        }
    };
    if env.time > consent.expires_at {
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
    keys.push(Key {
        scheme,
        key,
        label,
        added_at: env.time,
    });
    keys.sort_by(|a, b| a.key.cmp(&b.key));
    save(ctx, account);
    Ok(())
}

/// A person removes their own keys, never a senior one nor their last; an
/// agent's manager removes any of its keys.
pub(crate) fn remove_key(ctx: &ExecCtx, number: AccountNumber, key: &Vec<u8>) -> Result<(), Error> {
    let env = ctx.env();
    let mut account = account(ctx, number)?;
    let keys = match &mut account.control {
        Control::Person { keys } => {
            let signer = env.signer()?;
            let held = |wanted: &[u8]| keys.iter().find(|held| held.key == wanted);
            let removed = held(key).ok_or_else(|| not_found("that key is not on this account"))?;
            let remover =
                held(&signer).ok_or_else(|| unauthorized("the signer is not on this account"))?;
            if keys.len() == 1 {
                return Err(wrong_state("an account keeps its last key"));
            }
            if removed.added_at < remover.added_at {
                return Err(unauthorized("a key removes only itself or a junior key"));
            }
            keys
        }
        Control::Managed {
            manager,
            life: Life::Active { keys } | Life::Suspended { keys },
            ..
        } => {
            managing(ctx, *manager, number)?;
            if !keys.iter().any(|held| &held.key == key) {
                return Err(not_found("that key is not on this account"));
            }
            keys
        }
        Control::Managed {
            manager,
            life: Life::Revoked,
            ..
        } => {
            managing(ctx, *manager, number)?;
            return Err(revoked(number));
        }
        Control::Module { .. } => {
            return Err(wrong_state("a module's account holds no keys"));
        }
    };
    keys.retain(|held| &held.key != key);
    OF_KEY.remove(ctx, key);
    save(ctx, account);
    Ok(())
}

/// The account whose card the frame may edit: a person's or a module's
/// own, an agent's by its manager alone.
fn editable(ctx: &ExecCtx, number: AccountNumber) -> Result<Account, Error> {
    let account = account(ctx, number)?;
    let acting = acting(ctx)?;
    let editor = match &account.control {
        Control::Person { .. } | Control::Module { .. } => number,
        Control::Managed {
            life: Life::Revoked,
            ..
        } => return Err(revoked(number)),
        Control::Managed { manager, .. } => *manager,
    };
    if acting != editor {
        return Err(unauthorized(format!(
            "account {acting} does not edit account {number}"
        )));
    }
    Ok(account)
}

pub(crate) fn set_name(ctx: &ExecCtx, number: AccountNumber, name: String) -> Result<(), Error> {
    let mut account = editable(ctx, number)?;
    account.card.name = named(name)?;
    save(ctx, account);
    Ok(())
}

pub(crate) fn set_profile(
    ctx: &ExecCtx,
    number: AccountNumber,
    avatar: Option<abi::BlobId>,
    bio: Option<String>,
) -> Result<(), Error> {
    let mut account = editable(ctx, number)?;
    account.card.avatar = avatar;
    account.card.bio = bio
        .map(|bio| bio.trim().to_owned())
        .filter(|bio| !bio.is_empty());
    save(ctx, account);
    Ok(())
}

/// The agent `number` as its manager, who signed the frame, changes it:
/// `change` takes its life and gives the next, or says why not.
fn relive(
    ctx: &ExecCtx,
    number: AccountNumber,
    change: impl FnOnce(Life) -> Result<Life, Error>,
) -> Result<(), Error> {
    let mut account = account(ctx, number)?;
    let Control::Managed { manager, life, .. } = &mut account.control else {
        return Err(wrong_state(format!("account {number} is no one's agent")));
    };
    managing(ctx, *manager, number)?;
    let was = std::mem::replace(life, Life::Revoked);
    *life = change(was)?;
    save(ctx, account);
    Ok(())
}

pub(crate) fn suspend(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Active { keys } => Ok(Life::Suspended { keys }),
        Life::Suspended { .. } => Err(wrong_state(format!(
            "account {number} is already suspended"
        ))),
        Life::Revoked => Err(revoked(number)),
    })
}

pub(crate) fn resume(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Suspended { keys } => Ok(Life::Active { keys }),
        Life::Active { .. } => Err(wrong_state(format!("account {number} is active"))),
        Life::Revoked => Err(revoked(number)),
    })
}

/// Final: the agent's keys are dropped and act as no one; the manager's
/// list keeps it.
pub(crate) fn revoke(ctx: &ExecCtx, number: AccountNumber) -> Result<(), Error> {
    relive(ctx, number, |life| match life {
        Life::Active { keys } | Life::Suspended { keys } => {
            drop_keys(ctx, &keys);
            Ok(Life::Revoked)
        }
        Life::Revoked => Err(revoked(number)),
    })
}
