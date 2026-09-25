//! forge's store with chat and identity beside it: the modules forge
//! queries, and where its sent land when a block delivers them.
//! `accounts` is identity's roster: each key the account it belongs to,
//! the harness keys ([`HOLDERS`](super::HOLDERS)) from the start.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::{HostOp, HostReply};
use store::testing::MockHost;
use store::{Env, Error, code};
use store::{Reads, Writes};

pub struct MemorySandbox {
    pub forge: MockHost,
    pub chat: Rc<RefCell<MockHost>>,
    pub accounts: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>,
}

impl Default for MemorySandbox {
    fn default() -> Self {
        let chat = Rc::new(RefCell::new(MockHost::default()));
        let sibling = chat.clone();
        let mut forge = MockHost::default();
        forge.modules.insert(
            "chat".into(),
            Box::new(move |request| {
                let reply = chat::query(
                    &*sibling.borrow(),
                    &store::testing::env(store::Origin::Root),
                    store::decode(request)?,
                )?;
                Ok(store::encode(&reply))
            }),
        );
        let held = super::HOLDERS.map(|(key, account)| (key.to_vec(), account));
        let accounts = Rc::new(RefCell::new(BTreeMap::from(held)));
        let roster = accounts.clone();
        forge.modules.insert(
            identity::MODULE.into(),
            Box::new(move |request| {
                let identity::Query::OfKey { key } = store::decode(request)? else {
                    return Err(Error::new(code::UNSUPPORTED, "the sandbox answers OfKey"));
                };
                let held = roster.borrow().get(&key).copied();
                Ok(store::encode(&identity::Reply::Number(held)))
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

    /// Who `key` signs as: the principal forge's module resolves.
    pub fn principal(&self, key: &[u8]) -> Result<forge::Principal, Error> {
        identity::principal_of(&self.forge, &store::Origin::Signed(key.to_vec()))
    }

    /// Runs one chat message directly, as a key or module would in its own block.
    pub fn chat_execute(
        &self,
        env: &Env,
        sender: forge::Principal,
        msg: chat::Op,
    ) -> Result<(), Error> {
        chat::execute(&mut *self.chat.borrow_mut(), env, sender, msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Error> {
        chat::query(
            &*self.chat.borrow(),
            &store::testing::env(store::Origin::Root),
            q,
        )
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&mut self, height: u64, time: u64) -> Vec<Result<(), Error>> {
        let env = Env {
            height,
            time,
            ..store::testing::env(store::Origin::Module("forge".into()))
        };
        self.forge
            .take_sent()
            .into_iter()
            .map(|m| {
                if m.target != "chat" {
                    return Err(Error::new(code::UNKNOWN_PROGRAM, m.target));
                }
                self.chat_execute(
                    &env,
                    forge::Principal::Module("forge".into()),
                    store::decode(&m.payload)?,
                )
            })
            .collect()
    }

    pub fn blob_count(&self) -> usize {
        self.forge.blobs.len()
    }
}

impl Reads for MemorySandbox {
    fn host(&self, op: &HostOp) -> HostReply {
        self.forge.host(op)
    }
}

impl Writes for MemorySandbox {
    fn host_mut(&mut self, op: HostOp) -> HostReply {
        self.forge.host_mut(op)
    }
}
