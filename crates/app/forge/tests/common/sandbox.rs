//! forge's host with chat's beside it, and identity's roster: the siblings
//! forge queries, and where its emissions land when a block delivers them.
//! `accounts` is identity's roster: each key the account it belongs to,
//! the harness keys ([`HOLDERS`](super::HOLDERS)) from the start. Chat asks
//! the same roster.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use guest::{Cause, Env, Error, Origin, code};
use guest::{ExecCtx, MockHost, Module, QueryCtx, Sibling};

pub struct MemorySandbox {
    pub forge: MockHost,
    pub chat: MockHost,
    pub accounts: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>,
}

/// Identity over `roster`: `OfKey` only.
fn identity(roster: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>) -> Sibling {
    Box::new(move |request| {
        let identity::Query::OfKey { key } =
            abi::decode(request).map_err(guest::kernel::error_from)?
        else {
            return Err(Error::new(code::UNSUPPORTED, "the sandbox answers OfKey"));
        };
        let held = roster.borrow().get(&key).copied();
        Ok(abi::encode(&identity::Reply::Number(held)))
    })
}

/// The env of a block at `height`, signed by `origin`.
pub fn env_at(origin: Origin, height: u64, time: u64) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height,
        time,
        module: "forge".into(),
        origin,
        cause: Cause::Direct,
    }
}

impl Default for MemorySandbox {
    fn default() -> Self {
        let held = super::HOLDERS.map(|(key, account)| (key.to_vec(), account));
        let accounts = Rc::new(RefCell::new(BTreeMap::from(held)));
        let chat = MockHost::default();
        chat.borrow_mut()
            .siblings
            .insert(identity::MODULE.into(), identity(accounts.clone()));
        let forge = MockHost::default();
        let sibling = chat.clone();
        forge.borrow_mut().siblings.insert(
            "chat".into(),
            Box::new(move |request| {
                let reads = sibling.query(env_at(Origin::Root, 0, 0));
                let reply = chat::Chat::query(
                    &reads,
                    abi::decode(request).map_err(guest::kernel::error_from)?,
                )?;
                Ok(abi::encode(&reply))
            }),
        );
        forge
            .borrow_mut()
            .siblings
            .insert(identity::MODULE.into(), identity(accounts.clone()));
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

    /// A write to forge's host at `height`, signed by the system.
    pub fn exec(&self, height: u64) -> ExecCtx {
        self.forge.exec(env_at(Origin::Root, height, super::TIME))
    }

    /// A read of forge's host at `height`.
    pub fn reads(&self, height: u64) -> QueryCtx {
        self.forge.query(env_at(Origin::Root, height, super::TIME))
    }

    /// Who `key` signs as: the principal forge's module resolves.
    pub fn principal(&self, key: &[u8]) -> Result<forge::Principal, Error> {
        identity::principal_of(&self.reads(0), &Origin::Signed(key.to_vec()))
    }

    /// Runs one chat message directly, signed by `origin` in its own block.
    pub fn chat_execute(
        &self,
        origin: Origin,
        height: u64,
        time: u64,
        msg: chat::Op,
    ) -> Result<(), Error> {
        chat::Chat::execute(&self.chat.exec(env_at(origin, height, time)), msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Error> {
        chat::Chat::query(&self.chat.query(env_at(Origin::Root, 0, 0)), q)
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&mut self, height: u64, time: u64) -> Vec<Result<(), Error>> {
        self.forge
            .take_emissions()
            .into_iter()
            .map(|m| {
                if m.target != "chat" {
                    return Err(Error::new(code::UNKNOWN_PROGRAM, m.target));
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
