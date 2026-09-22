//! Where a program's state lives: the host under wasm32, a `BTreeMap` in a
//! native test. A state struct holds one `Handle`, shared by its collections.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ops::Bound;
use std::rc::Rc;

use abi::{Entry, Refusal, Scan, Scheme};

pub trait Store {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn scan(&self, scan: Scan) -> Vec<Entry>;
    fn set(&self, key: Vec<u8>, value: Vec<u8>);
    fn delete(&self, key: &[u8]);
    fn verify(
        &self,
        scheme: Scheme,
        key: &[u8],
        namespace: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<bool, Refusal>;
}

pub type Handle = Rc<dyn Store>;

/// A store for native tests: entries in a map, every op logged, and one
/// switch for what `verify` answers.
#[derive(Default)]
pub struct MemoryStore {
    entries: RefCell<BTreeMap<Vec<u8>, Vec<u8>>>,
    /// What `verify` answers; a test flips it to refuse a proof.
    pub accept: Cell<bool>,
    log: RefCell<Vec<String>>,
}

impl MemoryStore {
    pub fn new() -> Rc<Self> {
        let store = Self::default();
        store.accept.set(true);
        Rc::new(store)
    }

    /// The ops since the last call, one line each: `Get accounts/…`.
    pub fn ops(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    fn log(&self, op: &str, key: &[u8]) {
        self.log
            .borrow_mut()
            .push(format!("{op} {}", printable(key)));
    }
}

fn printable(key: &[u8]) -> String {
    key.iter()
        .map(|&b| {
            if b.is_ascii_graphic() {
                (b as char).to_string()
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect()
}

impl Store for MemoryStore {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.log("Get", key);
        self.entries.borrow().get(key).cloned()
    }

    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.log("Scan", &scan.lo);
        let entries = self.entries.borrow();
        let hi = match &scan.hi {
            Some(hi) => Bound::Excluded(hi.clone()),
            None => Bound::Unbounded,
        };
        let range = entries.range((Bound::Included(scan.lo.clone()), hi));
        let entry = |(key, value): (&Vec<u8>, &Vec<u8>)| Entry {
            key: key.clone(),
            value: value.clone(),
        };
        let limit = scan.limit.map_or(usize::MAX, |l| l as usize);
        if scan.reverse {
            range.rev().map(entry).take(limit).collect()
        } else {
            range.map(entry).take(limit).collect()
        }
    }

    fn set(&self, key: Vec<u8>, value: Vec<u8>) {
        self.log("Set", &key);
        self.entries.borrow_mut().insert(key, value);
    }

    fn delete(&self, key: &[u8]) {
        self.log("Delete", key);
        self.entries.borrow_mut().remove(key);
    }

    fn verify(
        &self,
        _scheme: Scheme,
        _key: &[u8],
        _namespace: &[u8],
        _message: &[u8],
        _signature: &[u8],
    ) -> Result<bool, Refusal> {
        self.log("Verify", b"");
        Ok(self.accept.get())
    }
}

#[cfg(all(feature = "program", target_arch = "wasm32"))]
mod host {
    use abi::{Entry, HostOp, HostReply, Refusal, Scan, Scheme};
    use guest::Reads;

    use super::Store;

    /// Every `guest` context is a store, and so is `Host`.
    impl<R: Reads> Store for R {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            Reads::get(self, key)
        }
        fn scan(&self, scan: Scan) -> Vec<Entry> {
            Reads::scan(self, scan)
        }
        fn set(&self, key: Vec<u8>, value: Vec<u8>) {
            match self.host(&HostOp::Set { key, value }) {
                HostReply::Done => {}
                other => panic!("host answered {other:?} where done was expected"),
            }
        }
        fn delete(&self, key: &[u8]) {
            match self.host(&HostOp::Delete(key.to_vec())) {
                HostReply::Done => {}
                other => panic!("host answered {other:?} where done was expected"),
            }
        }
        fn verify(
            &self,
            scheme: Scheme,
            key: &[u8],
            namespace: &[u8],
            message: &[u8],
            signature: &[u8],
        ) -> Result<bool, Refusal> {
            Reads::verify(self, scheme, key, namespace, message, signature)
        }
    }

    #[link(wasm_import_module = "ducktape")]
    unsafe extern "C" {
        fn host_call(ptr: u32, len: u32) -> u32;
        fn host_take(ptr: u32);
    }

    /// The host of the running call. `guest` opens its door only through the
    /// context a call receives, which a state struct cannot hold; this names
    /// the same two imports, so a program's ABI does not change.
    pub struct Host;

    impl Host {
        fn call(&self, op: &HostOp) -> HostReply {
            let request = abi::encode(op);
            let len = unsafe { host_call(request.as_ptr() as u32, request.len() as u32) };
            let mut reply = vec![0u8; len as usize];
            unsafe { host_take(reply.as_mut_ptr() as u32) };
            abi::decode(&reply).unwrap_or_else(|fault| panic!("{fault}"))
        }
    }

    impl Reads for Host {
        fn host(&self, op: &HostOp) -> HostReply {
            self.call(op)
        }
    }
}

#[cfg(all(feature = "program", target_arch = "wasm32"))]
pub use host::Host;
