//! forge's host with chat's beside it: the sibling forge queries, and where
//! its emissions land, in the frame that emitted them, run as the kernel
//! runs a frame by `chain` ([`guest::MockChain`]). `accounts` is identity's
//! roster: each key the account it belongs to, the harness keys
//! ([`HOLDERS`](super::HOLDERS)) from the start, each module its account
//! ([`MODULES`]) and each agent its standing (`agents`). A frame's sender
//! is resolved against it, as the host asks identity.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::role::identity::{Category, Kind, Profile, Standing};
use guest::{Cause, Env, Error, Origin, Outcome, Principal};
use guest::{ExecCtx, MockChain, MockHost, Module, QueryCtx};

pub struct MemorySandbox {
    pub forge: MockHost,
    pub chat: MockHost,
    /// forge and chat, each acting as its [`MODULES`] account
    pub chain: MockChain,
    pub accounts: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>,
    /// the agents [`AGENT_MANAGER`] manages, each with its standing
    pub agents: Rc<RefCell<BTreeMap<u64, Standing>>>,
}

/// The person every agent of the roster is managed by.
pub const AGENT_MANAGER: u64 = 11;

/// The account identity registered for each module.
pub const MODULES: [(&str, u64); 2] = [("forge", 900), ("chat", 901)];

/// The module whose account `number` is.
pub fn module_of(number: u64) -> Option<&'static str> {
    MODULES
        .iter()
        .find(|(_, account)| *account == number)
        .map(|(module, _)| *module)
}

/// `module`'s env in a block at `height`, sent by `origin` acting as `sender`.
fn env(module: &str, origin: Origin, sender: Option<Principal>, height: u64, time: u64) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height,
        time,
        module: module.into(),
        origin,
        sender,
        roles: guest::MockHost::roles(),
        cause: Cause::Direct,
    }
}

impl Default for MemorySandbox {
    fn default() -> Self {
        let held = super::HOLDERS.map(|(key, account)| (key.to_vec(), account));
        let accounts = Rc::new(RefCell::new(BTreeMap::from(held)));
        let chat = MockHost::default();
        let forge = MockHost::default();
        let sibling = chat.clone();
        forge.borrow_mut().siblings.insert(
            "chat".into(),
            Box::new(move |request| {
                let reads = sibling.query(env("chat", Origin::Root, None, 0, 0));
                let reply = chat::Chat::query(
                    &reads,
                    abi::decode(request).map_err(guest::kernel::error_from)?,
                )?;
                Ok(abi::encode(&reply))
            }),
        );
        let agents = Rc::new(RefCell::new(BTreeMap::new()));
        let (roster, standing) = (accounts.clone(), agents.clone());
        forge.borrow_mut().siblings.insert(
            guest::MockHost::roles().identity,
            Box::new(move |request| {
                guest::identity_role(&profiles(&roster.borrow(), &standing.borrow()), request)
            }),
        );
        let mut chain = MockChain::default();
        let account = |module| {
            let (_, number) = MODULES.iter().find(|(name, _)| *name == module).unwrap();
            Some(Principal::Account(*number))
        };
        chain.seat(
            "forge",
            forge.clone(),
            account("forge"),
            guest::execute::<forge::Forge>,
        );
        chain.seat(
            "chat",
            chat.clone(),
            account("chat"),
            guest::execute::<chat::Chat>,
        );
        MemorySandbox {
            forge,
            chat,
            chain,
            accounts,
            agents,
        }
    }
}

/// The roster as the identity role profiles it: each key's account a
/// person's, each of [`MODULES`] its module's, each agent managed by
/// [`AGENT_MANAGER`].
fn profiles(roster: &BTreeMap<Vec<u8>, u64>, agents: &BTreeMap<u64, Standing>) -> Vec<Profile> {
    let people = roster.values().map(|number| (*number, Kind::Person));
    let modules = MODULES.map(|(module, number)| (number, Kind::Module(module.into())));
    let agents = agents.iter().map(|(number, standing)| {
        let kind = Kind::Managed {
            manager: AGENT_MANAGER,
            category: Category::Agent,
            standing: *standing,
        };
        (*number, kind)
    });
    people
        .chain(modules)
        .chain(agents)
        .map(|(number, kind)| Profile {
            number,
            name: format!("account {number}"),
            kind,
        })
        .collect()
}

impl MemorySandbox {
    /// Seats `key` in `account`, as identity would.
    pub fn hold(&self, key: &[u8], account: u64) {
        self.accounts.borrow_mut().insert(key.to_vec(), account);
    }

    /// forge's env in a block at `height`, sent by `origin`: its sender
    /// resolved as the host resolves it, a key through the roster.
    pub fn env_at(&self, origin: Origin, height: u64, time: u64) -> Env {
        self.env_of("forge", origin, height, time)
    }

    /// `module`'s env in a block at `height`, sent by `origin`.
    fn env_of(&self, module: &str, origin: Origin, height: u64, time: u64) -> Env {
        let sender = match &origin {
            Origin::Signed(key) => self.principal(key),
            Origin::Module(module) => MODULES
                .iter()
                .find(|(name, _)| name == module)
                .map(|(_, account)| Principal::Account(*account)),
            Origin::Root => Some(Principal::Root),
        };
        env(module, origin, sender, height, time)
    }

    /// A write to forge's host at `height`, signed by the system.
    pub fn exec(&self, height: u64) -> ExecCtx {
        self.forge
            .exec(self.env_at(Origin::Root, height, super::TIME))
    }

    /// A read of forge's host at `height`.
    pub fn reads(&self, height: u64) -> QueryCtx {
        self.forge
            .query(env("forge", Origin::Root, None, height, super::TIME))
    }

    /// Who `key` signs as: the account it holds, if any.
    pub fn principal(&self, key: &[u8]) -> Option<Principal> {
        self.accounts
            .borrow()
            .get(key)
            .copied()
            .map(Principal::Account)
    }

    /// Runs one chat message directly, signed by `origin` in its own block.
    pub fn chat_execute(
        &self,
        origin: Origin,
        height: u64,
        time: u64,
        msg: chat::Op,
    ) -> Result<(), Error> {
        let env = self.env_of("chat", origin, height, time);
        chat::Chat::execute(&self.chat.exec(env), msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Error> {
        chat::Chat::query(&self.chat.query(env("chat", Origin::Root, None, 0, 0)), q)
    }

    /// forge's op signed by `origin` in a block at `height`, and what it
    /// emitted, in one frame as the kernel runs it: forge's output, or the
    /// refusal that failed the frame (which left every host as it was).
    pub fn frame(
        &self,
        origin: Origin,
        height: u64,
        time: u64,
        op: &forge::Op,
    ) -> Result<Vec<u8>, Error> {
        let env = self.env_at(origin, height, time);
        match self.chain.execute(env, &abi::encode(op)) {
            Outcome::Applied { output } => Ok(output),
            Outcome::Rejected(error) => Err(error),
        }
    }

    pub fn blob_count(&self) -> usize {
        self.forge.borrow().blobs.len()
    }
}
