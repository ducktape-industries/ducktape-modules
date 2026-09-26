//! forge's host with chat's beside it: the sibling forge queries, and where
//! its emissions land when a block delivers them. `accounts` is identity's
//! roster: each key the account it belongs to, the harness keys
//! ([`HOLDERS`](super::HOLDERS)) from the start, and each module its account
//! ([`MODULES`]). A frame's sender is resolved against it, as the host asks
//! identity.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use guest::{Cause, Env, Error, Origin, Principal, code};
use guest::{ExecCtx, MockHost, Module, QueryCtx};

pub struct MemorySandbox {
    pub forge: MockHost,
    pub chat: MockHost,
    pub accounts: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>,
}

/// The account identity registered for each module.
pub const MODULES: [(&str, u64); 2] = [("forge", 900), ("chat", 901)];

/// The module whose account `number` is.
pub fn module_of(number: u64) -> Option<&'static str> {
    MODULES
        .iter()
        .find(|(_, account)| *account == number)
        .map(|(module, _)| *module)
}

/// The env of a block at `height`, sent by `origin` acting as `sender`.
fn env(origin: Origin, sender: Option<Principal>, height: u64, time: u64) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height,
        time,
        module: "forge".into(),
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
                let reads = sibling.query(env(Origin::Root, None, 0, 0));
                let reply = chat::Chat::query(
                    &reads,
                    abi::decode(request).map_err(guest::kernel::error_from)?,
                )?;
                Ok(abi::encode(&reply))
            }),
        );
        MemorySandbox {
            forge,
            chat,
            accounts,
        }
    }
}

impl MemorySandbox {
    /// Seats `key` in `account`, as identity would.
    pub fn hold(&self, key: &[u8], account: u64) {
        self.accounts.borrow_mut().insert(key.to_vec(), account);
    }

    /// The env of a block at `height`, sent by `origin`: its sender resolved
    /// as the host resolves it, a key through the roster.
    pub fn env_at(&self, origin: Origin, height: u64, time: u64) -> Env {
        let sender = match &origin {
            Origin::Signed(key) => self.principal(key),
            Origin::Module(module) => MODULES
                .iter()
                .find(|(name, _)| name == module)
                .map(|(_, account)| Principal::Account(*account)),
            Origin::Root => Some(Principal::Root),
        };
        env(origin, sender, height, time)
    }

    /// A write to forge's host at `height`, signed by the system.
    pub fn exec(&self, height: u64) -> ExecCtx {
        self.forge
            .exec(self.env_at(Origin::Root, height, super::TIME))
    }

    /// A read of forge's host at `height`.
    pub fn reads(&self, height: u64) -> QueryCtx {
        self.forge
            .query(env(Origin::Root, None, height, super::TIME))
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
        chat::Chat::execute(&self.chat.exec(self.env_at(origin, height, time)), msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Error> {
        chat::Chat::query(&self.chat.query(env(Origin::Root, None, 0, 0)), q)
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&mut self, height: u64, time: u64) -> Vec<Result<(), Error>> {
        self.forge
            .take_emissions()
            .into_iter()
            .map(|m| {
                if m.target != "chat" {
                    return Err(Error::new(code::UNKNOWN_MODULE, m.target));
                }
                let forge = Origin::Module("forge".into());
                self.chat_execute(
                    forge,
                    height,
                    time,
                    abi::decode(&m.payload).map_err(guest::kernel::error_from)?,
                )
            })
            .collect()
    }

    pub fn blob_count(&self) -> usize {
        self.forge.borrow().blobs.len()
    }
}
