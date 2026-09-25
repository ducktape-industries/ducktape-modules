// forge's store with chat beside it: the sibling forge queries, and where its emissions land when a block delivers them.

use std::cell::RefCell;
use std::rc::Rc;

use abi::{BlobId, HashKind, HostOp, HostReply, ItemRef, ProgramId, Refusal, reason};
use store::{Memory, Reads, Writes};

pub struct MemorySandbox {
    pub forge: Memory,
    pub chat: Rc<RefCell<Memory>>,
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
        MemorySandbox { forge, chat }
    }
}

impl MemorySandbox {
    /// Runs one chat message directly, as a key or module would in its own block.
    pub fn chat_execute(&self, frame: &chat::Frame, msg: chat::Op) -> Result<(), Refusal> {
        chat::execute(&mut *self.chat.borrow_mut(), frame, msg)
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Refusal> {
        chat::query(&*self.chat.borrow(), 0, q)
    }

    /// Delivers what forge emitted so far to chat, the way the kernel delivers
    /// the previous block's queue: as forge, at the delivering height.
    pub fn deliver(&mut self, height: u64, time: u64) -> Vec<Result<(), Refusal>> {
        let frame = chat::Frame {
            party: chat::Party::Module("forge".into()),
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
