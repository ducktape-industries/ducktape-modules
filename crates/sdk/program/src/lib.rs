//! The runtime a program is written over: a state struct of typed
//! collections and inline scalars, whose methods `#[program]` turns into the
//! wire contract (`Op`, `Query`, `Reply`) and the guest glue.
//!
//! The root struct's scalars are one borsh record under the key `state`,
//! loaded when a call starts and saved after an `&mut self` call. A `Map`,
//! `Set` or `Item` field holds only its prefix (its field name); its elements
//! are separate keys, `<name>/<encoded key>` (see [`KeyCodec`]).
mod key;
mod store;

use core::marker::PhantomData;

use abi::{Refusal, Scan};
use borsh::{BorshDeserialize, BorshSerialize};

pub use key::{KeyCodec, corrupt};
pub use program_derive::program;
#[cfg(all(feature = "program", target_arch = "wasm32"))]
pub use store::Host;
pub use store::{Handle, MemoryStore, Store};

/// Not part of the surface: what the generated code names.
#[doc(hidden)]
pub mod __private {
    pub use abi;
    pub use std::rc::Rc;
    pub const STATE: &[u8] = b"state";
}

pub struct Map<K, V> {
    host: Handle,
    prefix: Vec<u8>,
    _element: PhantomData<(K, V)>,
}

impl<K: KeyCodec, V: BorshSerialize + BorshDeserialize> Map<K, V> {
    pub fn new(host: Handle, name: &str) -> Self {
        Map {
            host,
            prefix: format!("{name}/").into_bytes(),
            _element: PhantomData,
        }
    }

    fn key(&self, key: &K) -> Vec<u8> {
        let mut bytes = self.prefix.clone();
        key.encode_key(&mut bytes);
        bytes
    }

    /// Every element's key starts with this; a `Page` scans from it.
    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    /// Every element whose key starts with `head` (a tuple key's first
    /// parts) starts with this.
    pub fn prefix_of(&self, head: &impl KeyCodec) -> Vec<u8> {
        let mut bytes = self.prefix.clone();
        head.encode_key(&mut bytes);
        bytes
    }

    pub fn get(&self, key: &K) -> Result<Option<V>, Refusal> {
        self.host
            .get(&self.key(key))
            .map(|bytes| abi::decode(&bytes))
            .transpose()
    }

    pub fn has(&self, key: &K) -> bool {
        self.host.get(&self.key(key)).is_some()
    }

    pub fn insert(&mut self, key: &K, value: &V) {
        self.host.set(self.key(key), abi::encode(value));
    }

    pub fn remove(&mut self, key: &K) {
        self.host.delete(&self.key(key));
    }

    /// The elements a scan admits, in scan order: the store key (a page
    /// cursor), the key decoded, the value.
    pub fn rows(&self, scan: Scan) -> Result<Vec<(Vec<u8>, K, V)>, Refusal> {
        self.host
            .scan(scan)
            .into_iter()
            .map(|entry| {
                let mut rest = entry
                    .key
                    .strip_prefix(self.prefix.as_slice())
                    .ok_or_else(|| corrupt("a scanned key is outside its collection"))?;
                let key = K::decode_key(&mut rest)?;
                if !rest.is_empty() {
                    return Err(corrupt("a scanned key has bytes past its element"));
                }
                Ok((entry.key, key, abi::decode(&entry.value)?))
            })
            .collect()
    }
}

pub struct Set<K>(Map<K, ()>);

impl<K: KeyCodec> Set<K> {
    pub fn new(host: Handle, name: &str) -> Self {
        Set(Map::new(host, name))
    }

    pub fn prefix(&self) -> &[u8] {
        self.0.prefix()
    }

    pub fn prefix_of(&self, head: &impl KeyCodec) -> Vec<u8> {
        self.0.prefix_of(head)
    }

    pub fn has(&self, key: &K) -> bool {
        self.0.has(key)
    }

    pub fn insert(&mut self, key: &K) {
        self.0.insert(key, &());
    }

    pub fn remove(&mut self, key: &K) {
        self.0.remove(key);
    }

    /// The members a scan admits: the store key (a page cursor) and the member.
    pub fn keys(&self, scan: Scan) -> Result<Vec<(Vec<u8>, K)>, Refusal> {
        Ok(self
            .0
            .rows(scan)?
            .into_iter()
            .map(|(cursor, key, ())| (cursor, key))
            .collect())
    }
}

/// One value under its own key, read only when asked for: a scalar too big
/// or too rarely used to ride the root record.
pub struct Item<T> {
    host: Handle,
    key: Vec<u8>,
    _value: PhantomData<T>,
}

impl<T: BorshSerialize + BorshDeserialize> Item<T> {
    pub fn new(host: Handle, name: &str) -> Self {
        Item {
            host,
            key: name.as_bytes().to_vec(),
            _value: PhantomData,
        }
    }

    pub fn get(&self) -> Result<Option<T>, Refusal> {
        self.host
            .get(&self.key)
            .map(|b| abi::decode(&b))
            .transpose()
    }

    pub fn set(&mut self, value: &T) {
        self.host.set(self.key.clone(), abi::encode(value));
    }

    pub fn clear(&mut self) {
        self.host.delete(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_live_under_their_prefix_and_decode_their_keys() {
        let store = MemoryStore::new();
        let host: Handle = store.clone();
        let mut names: Map<u64, String> = Map::new(host.clone(), "names");
        let mut edges: Set<(u64, u64)> = Set::new(host.clone(), "edges");
        let mut next: Item<u64> = Item::new(host, "next");
        names.insert(&2, &"two".into());
        names.insert(&10, &"ten".into());
        edges.insert(&(1, 2));
        edges.insert(&(1, 3));
        edges.insert(&(2, 1));
        next.set(&7);
        assert_eq!(names.get(&10).unwrap(), Some("ten".into()));
        assert!(names.has(&2) && !names.has(&3));
        let rows = names.rows(Scan::prefix(names.prefix())).unwrap();
        assert_eq!(rows[0].1, 2);
        assert_eq!(
            rows[1],
            (b"names/\0\0\0\0\0\0\0\x0a".to_vec(), 10, "ten".into())
        );
        let under_one = edges.keys(Scan::prefix(edges.prefix_of(&1u64))).unwrap();
        assert_eq!(
            under_one.iter().map(|(_, k)| *k).collect::<Vec<_>>(),
            [(1, 2), (1, 3)]
        );
        assert_eq!(next.get().unwrap(), Some(7));
        names.remove(&2);
        next.clear();
        assert!(!names.has(&2) && next.get().unwrap().is_none());
        assert!(
            store
                .ops()
                .iter()
                .all(|op| op.contains('/') || op.contains("next"))
        );
    }
}
