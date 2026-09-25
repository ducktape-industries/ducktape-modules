//! forge's store with chat and identity beside it: the siblings forge
//! queries, and where its emissions land when a block delivers them.
//! `accounts` is identity's roster: each key the account it belongs to,
//! the harness keys ([`HOLDERS`](super::HOLDERS)) from the start.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::{BlobId, HashKind, HostOp, HostReply, ItemRef, ProgramId, Refusal, reason};
use store::{Memory, Reads, Writes};

pub struct MemorySandbox {
    pub forge: Memory,
    pub chat: Rc<RefCell<Memory>>,
    pub accounts: Rc<RefCell<BTreeMap<Vec<u8>, u64>>>,
}

impl Default for MemorySandbox {
    fn default() -> Self {
        let chat = Rc::new(RefCell::new(Memory::default()));
        let sibling = chat.clone();
        let mut forge = Memory::default();
        forge.siblings.insert(
            "chat".into(),
            Box::new(move |request| {
                let reply = chat::query(&*sibling.borrow(), 0, abi::decode(request)?)?;
                Ok(abi::encode(&reply))
            }),
        );
        let held = super::HOLDERS.map(|(key, account)| (key.to_vec(), account));
        let accounts = Rc::new(RefCell::new(BTreeMap::from(held)));
        let roster = accounts.clone();
        forge.siblings.insert(
            identity::PROGRAM.into(),
            Box::new(move |request| {
                let identity::Query::OfKey { key } = abi::decode(request)? else {
                    return Err(Refusal::new(
                        reason::UNSUPPORTED,
                        "the sandbox answers OfKey",
                    ));
                };
                let held = roster.borrow().get(&key).copied();
                Ok(abi::encode(&identity::Reply::Number(held)))
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

    /// Who `key` signs as: the principal forge's program resolves.
    pub fn principal(&self, key: &[u8]) -> Result<forge::Principal, Refusal> {
        identity::principal_of(&self.forge, &abi::Origin::External(key.to_vec()))
    }

    /// Runs one chat message directly, as a key or module would in its own block.
    pub fn chat_execute(&self, frame: &forge::Frame, msg: chat::Op) -> Result<(), Refusal> {
        chat::execute(&mut *self.chat.borrow_mut(), frame, msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Refusal> {
        chat::query(&*self.chat.borrow(), 0, q)
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&mut self, height: u64, time: u64) -> Vec<Result<(), Refusal>> {
        let frame = forge::Frame {
            principal: forge::Principal::Module("forge".into()),
            height,
            time,
        };
        self.forge
            .take_emissions()
            .into_iter()
            .map(|m| {
                if m.target != "chat" {
                    return Err(Refusal::new(reason::UNKNOWN_PROGRAM, m.target));
                }
                self.chat_execute(&frame, abi::decode(&m.payload)?)
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
    fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.forge.set(key, value)
    }
    fn delete(&mut self, key: impl Into<Vec<u8>>) {
        self.forge.delete(key)
    }
    fn blob_put(
        &mut self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Refusal> {
        self.forge.blob_put(hash, kind, body)
    }
    fn emit(&mut self, target: impl Into<ProgramId>, payload: impl Into<Vec<u8>>) -> ItemRef {
        self.forge.emit(target, payload)
    }
    fn event(&mut self, payload: impl Into<Vec<u8>>) {
        self.forge.event(payload)
    }
    fn output(&mut self, bytes: impl Into<Vec<u8>>) {
        self.forge.output(bytes)
    }
}
