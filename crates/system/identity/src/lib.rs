//! The `identity` program: accounts, the keys and programs that control them,
//! and the consent by which a key joins an account. The types and the state
//! are always built; a view links them with `program` off. `#[program]`
//! derives `Op`, `Query` and `Reply` from `impl Identity`, and with the
//! `program` feature the wasm32 program over the host.
use abi::{BlobId, Env, Origin, ProgramId, Refusal, Scheme};
use borsh::{BorshDeserialize, BorshSerialize};
use module_registry::helpers::{already_exists, invalid, not_found, unauthorized, wrong_state};
use module_registry::{Page, PageReply};
use program::{Map, Set, program};

#[cfg(test)]
mod tests;

pub type AccountNumber = u64;

pub const PROGRAM: &str = "identity";
pub const CONSENT_NAMESPACE: &[u8] = b"ducktape:identity:consent";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Key {
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub label: Option<String>,
    pub added_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Standing {
    Active,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Control {
    Keys(Vec<Key>),
    Program {
        executor: ProgramId,
        controller: AccountNumber,
        standing: Standing,
    },
    Revoked {
        controller: AccountNumber,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    pub number: AccountNumber,
    pub name: String,
    pub control: Control,
    pub avatar: Option<BlobId>,
    pub bio: Option<String>,
    pub updated_at: u64,
}

impl Account {
    pub fn keys(&self) -> &[Key] {
        match &self.control {
            Control::Keys(keys) => keys,
            Control::Program { .. } | Control::Revoked { .. } => &[],
        }
    }

    pub fn holds(&self, key: &[u8]) -> bool {
        self.keys().iter().any(|held| held.key == key)
    }

    pub fn live(&self) -> bool {
        match &self.control {
            Control::Keys(_) => true,
            Control::Program { standing, .. } => *standing == Standing::Active,
            Control::Revoked { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Consent {
    pub key: Vec<u8>,
    pub account: AccountNumber,
    pub expires_at: u64,
    pub proof: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Admission {
    pub network: Vec<u8>,
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub generation: u64,
    pub account: AccountNumber,
    pub expires_at: u64,
}

impl Admission {
    pub fn preimage(&self) -> Vec<u8> {
        abi::encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reference {
    Account(AccountNumber),
    Key(Vec<u8>),
}

pub fn principal(number: AccountNumber) -> Vec<u8> {
    number.to_le_bytes().to_vec()
}

pub fn account_of_principal(bytes: &[u8]) -> Option<AccountNumber> {
    <[u8; 8]>::try_from(bytes).ok().map(u64::from_le_bytes)
}

/// The state: accounts by number, the account each key holds, how many times
/// a key has been admitted, which program accounts each account controls
/// (`(controller, controlled)`, so one prefix scan lists a controller's),
/// and the last number given out.
#[program]
pub struct Identity {
    accounts: Map<AccountNumber, Account>,
    of_key: Map<Vec<u8>, AccountNumber>,
    generations: Map<Vec<u8>, u64>,
    controlled: Set<(AccountNumber, AccountNumber)>,
    next: AccountNumber,
}

#[program]
impl Identity {
    pub fn create(
        &mut self,
        env: &Env,
        name: String,
        scheme: Scheme,
    ) -> Result<AccountNumber, Refusal> {
        let signer = module_registry::helpers::external(env)?;
        let number = self.next + 1;
        self.admit_key(&signer, number)?;
        self.next = number;
        self.accounts.insert(
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
        Ok(number)
    }

    pub fn add_key(
        &mut self,
        env: &Env,
        scheme: Scheme,
        label: Option<String>,
        consent: Consent,
    ) -> Result<(), Refusal> {
        let signer = module_registry::helpers::external(env)?;
        let mut account = self.account(consent.account)?;
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
            generation: self.generation(signer.clone())?,
            account: consent.account,
            expires_at: consent.expires_at,
        };
        let consented = self.host().verify(
            authorizer.scheme,
            &consent.key,
            CONSENT_NAMESPACE,
            &admission.preimage(),
            &consent.proof,
        )?;
        if !consented {
            return Err(unauthorized("the consent does not verify"));
        }
        self.admit_key(&signer, consent.account)?;
        keys.push(Key {
            scheme,
            key: signer,
            label,
            added_at: env.time,
        });
        keys.sort_by(|a, b| a.key.cmp(&b.key));
        account.updated_at = env.time;
        self.accounts.insert(&account.number, &account);
        Ok(())
    }

    pub fn remove_key(&mut self, env: &Env, key: Vec<u8>) -> Result<(), Refusal> {
        let signer = module_registry::helpers::external(env)?;
        let mut account = self.account_of_key(&signer)?;
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
        self.of_key.remove(&key);
        account.updated_at = env.time;
        self.accounts.insert(&account.number, &account);
        Ok(())
    }

    pub fn set_name(
        &mut self,
        env: &Env,
        account: AccountNumber,
        name: String,
    ) -> Result<(), Refusal> {
        let mut account = self.account(account)?;
        acts_for(env, &account)?;
        account.name = named(name)?;
        account.updated_at = env.time;
        self.accounts.insert(&account.number, &account);
        Ok(())
    }

    pub fn set_profile(
        &mut self,
        env: &Env,
        account: AccountNumber,
        avatar: Option<BlobId>,
        bio: Option<String>,
    ) -> Result<(), Refusal> {
        let mut account = self.account(account)?;
        acts_for(env, &account)?;
        account.avatar = avatar;
        account.bio = bio
            .map(|bio| bio.trim().to_owned())
            .filter(|bio| !bio.is_empty());
        account.updated_at = env.time;
        self.accounts.insert(&account.number, &account);
        Ok(())
    }

    pub fn create_program(
        &mut self,
        env: &Env,
        name: String,
        controller: AccountNumber,
    ) -> Result<AccountNumber, Refusal> {
        let executor = module_registry::helpers::program(env)?;
        let controlling = self.account(controller)?;
        if !controlling.live() {
            return Err(wrong_state(format!("account {controller} is not live")));
        }
        let number = self.next + 1;
        let name = named(name)?;
        self.next = number;
        self.accounts.insert(
            &number,
            &Account {
                number,
                name,
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
        self.controlled.insert(&(controller, number));
        Ok(number)
    }

    pub fn set_standing(
        &mut self,
        env: &Env,
        account: AccountNumber,
        standing: Standing,
    ) -> Result<(), Refusal> {
        let program = module_registry::helpers::program(env)?;
        let number = account;
        let mut account = self.account(number)?;
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
        self.accounts.insert(&number, &account);
        Ok(())
    }

    pub fn transfer_control(
        &mut self,
        env: &Env,
        account: AccountNumber,
        to: AccountNumber,
    ) -> Result<(), Refusal> {
        let number = account;
        let mut account = self.account(number)?;
        let controller = self.controls(env, &account)?;
        let target = self.account(to)?;
        if !target.live() {
            return Err(wrong_state(format!("account {to} is not live")));
        }
        let circular = to == number || self.ancestry(to)?.contains(&number);
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
        self.controlled.remove(&(controller, number));
        self.controlled.insert(&(to, number));
        account.updated_at = env.time;
        self.accounts.insert(&number, &account);
        Ok(())
    }

    pub fn revoke(&mut self, env: &Env, account: AccountNumber) -> Result<(), Refusal> {
        let mut account = self.account(account)?;
        let controller = self.controls(env, &account)?;
        account.control = Control::Revoked { controller };
        account.updated_at = env.time;
        self.accounts.insert(&account.number, &account);
        Ok(())
    }

    pub fn get(&self, number: AccountNumber) -> Result<Option<Account>, Refusal> {
        self.accounts.get(&number)
    }

    pub fn of_key(&self, key: Vec<u8>) -> Result<Option<AccountNumber>, Refusal> {
        self.of_key.get(&key)
    }

    pub fn generation(&self, key: Vec<u8>) -> Result<u64, Refusal> {
        Ok(self.generations.get(&key)?.unwrap_or(0))
    }

    pub fn resolve(
        &self,
        references: Vec<Reference>,
    ) -> Result<Vec<Option<AccountNumber>>, Refusal> {
        references
            .into_iter()
            .map(|reference| match reference {
                Reference::Account(number) => Ok(self.accounts.has(&number).then_some(number)),
                Reference::Key(key) => self.of_key.get(&key),
            })
            .collect()
    }

    pub fn list(&self, env: &Env, page: Page) -> Result<PageReply<Account>, Refusal> {
        let rows = self
            .accounts
            .rows(page.scan_ahead(self.accounts.prefix()))?;
        Ok(page.reply(
            env.height,
            rows.into_iter()
                .map(|(cursor, _, account)| (cursor, account)),
        ))
    }

    pub fn controlled(
        &self,
        env: &Env,
        by: AccountNumber,
        page: Page,
    ) -> Result<PageReply<Account>, Refusal> {
        let scan = page.scan_ahead(&self.controlled.prefix_of(&by));
        let mut accounts = Vec::new();
        for (cursor, (_, number)) in self.controlled.keys(scan)? {
            accounts.push((cursor, self.account(number)?));
        }
        Ok(page.reply(env.height, accounts))
    }

    fn account(&self, number: AccountNumber) -> Result<Account, Refusal> {
        self.accounts
            .get(&number)?
            .ok_or_else(|| not_found(format!("account {number}")))
    }

    fn account_of_key(&self, key: &[u8]) -> Result<Account, Refusal> {
        let number = self
            .of_key
            .get(&key.to_vec())?
            .ok_or_else(|| unauthorized("this key holds no account"))?;
        self.account(number)
    }

    fn admit_key(&mut self, key: &[u8], number: AccountNumber) -> Result<(), Refusal> {
        let key = key.to_vec();
        if self.of_key.has(&key) {
            return Err(already_exists("this key already belongs to an account"));
        }
        self.of_key.insert(&key, &number);
        let generation = self.generation(key.clone())? + 1;
        self.generations.insert(&key, &generation);
        Ok(())
    }

    /// The controller of a program account, once the frame acts for it.
    fn controls(&self, env: &Env, controlled: &Account) -> Result<AccountNumber, Refusal> {
        let Control::Program { controller, .. } = &controlled.control else {
            return Err(wrong_state(format!(
                "account {} is not a live program account",
                controlled.number
            )));
        };
        acts_for(env, &self.account(*controller)?)?;
        Ok(*controller)
    }

    fn ancestry(&self, mut number: AccountNumber) -> Result<Vec<AccountNumber>, Refusal> {
        let mut ancestors = Vec::new();
        loop {
            let Control::Program { controller, .. } = self.account(number)?.control else {
                return Ok(ancestors);
            };
            ancestors.push(controller);
            number = controller;
        }
    }
}

fn named(name: String) -> Result<String, Refusal> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a name is not empty"));
    }
    Ok(name)
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

#[cfg(feature = "guest")]
pub fn account_of(
    ctx: &impl guest::Reads,
    key: &[u8],
) -> Result<Option<AccountNumber>, abi::Refusal> {
    match ctx.ask::<Query, Reply>(PROGRAM, &Query::OfKey { key: key.to_vec() })? {
        Reply::OfKey(number) => Ok(number),
        other => Err(abi::Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            format!("identity answered OfKey with {other:?}"),
        )),
    }
}

#[cfg(feature = "guest")]
pub fn account(
    ctx: &impl guest::Reads,
    number: AccountNumber,
) -> Result<Option<Account>, abi::Refusal> {
    match ctx.ask::<Query, Reply>(PROGRAM, &Query::Get { number })? {
        Reply::Get(account) => Ok(account),
        other => Err(abi::Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            format!("identity answered Get with {other:?}"),
        )),
    }
}
