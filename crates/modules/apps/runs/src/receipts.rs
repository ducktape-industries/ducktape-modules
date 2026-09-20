//! Point reads and writes for durable action receipts. Native snapshots carry
//! these exact records; guests place them individually in the host's KV store.
use sdk::Error;
#[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
use sdk::MerkleStore;
use sdk::refusal;
use std::collections::BTreeMap;

pub(super) type Records = BTreeMap<String, Vec<u8>>;

pub(super) enum View {
    Live,
    Committed,
}

enum Backing {
    Memory(Records),
    #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
    Host(Box<dyn MerkleStore>),
}

pub(super) struct Receipts {
    backing: Backing,
    /// this block's staged writes; `None` is a staged delete, which `get`
    /// answers as absent and `commit` applies as a removal.
    pending: BTreeMap<String, Option<Vec<u8>>>,
}

impl Default for Receipts {
    fn default() -> Self {
        Self {
            backing: Backing::Memory(Records::new()),
            pending: BTreeMap::new(),
        }
    }
}

impl Receipts {
    #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
    pub fn hosted(store: Box<dyn MerkleStore>) -> Self {
        Self {
            backing: Backing::Host(store),
            pending: BTreeMap::new(),
        }
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        if let Some(staged) = self.pending.get(key) {
            return Ok(staged.clone());
        }
        match &self.backing {
            Backing::Memory(records) => Ok(records.get(key).cloned()),
            #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
            Backing::Host(store) => store.get(&sdk::store_key(key.as_bytes())).await,
        }
    }

    pub async fn read(&self, key: &str, view: View) -> Result<Option<Vec<u8>>, Error> {
        match view {
            View::Live => self.get(key).await,
            View::Committed => self.committed(key).await,
        }
    }

    pub async fn committed(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match &self.backing {
            Backing::Memory(records) => Ok(records.get(key).cloned()),
            #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
            Backing::Host(store) => store.get_committed(&sdk::store_key(key.as_bytes())).await,
        }
    }

    pub fn stage(&mut self, key: String, value: Vec<u8>) -> Result<(), Error> {
        if value.len() > sdk::MAX_STORE_VALUE_BYTES {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: "action receipt record exceeds the store value bound".into(),
            });
        }
        self.pending.insert(key, Some(value));
        Ok(())
    }

    /// Stage a removal. The store's write batch already carries deletes, so a
    /// record that falls out of an index leaves nothing behind to decode.
    pub fn remove(&mut self, key: String) {
        self.pending.insert(key, None);
    }

    pub async fn commit(&mut self) -> Result<(), Error> {
        match &mut self.backing {
            Backing::Memory(records) => {
                for (key, staged) in std::mem::take(&mut self.pending) {
                    match staged {
                        Some(value) => records.insert(key, value),
                        None => records.remove(&key),
                    };
                }
            }
            #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
            Backing::Host(store) => {
                let writes = self
                    .pending
                    .iter()
                    .map(|(key, value)| (sdk::store_key(key.as_bytes()), value.clone()))
                    .collect();
                store.commit_batch(writes).await?;
                self.pending.clear();
            }
        }
        Ok(())
    }

    pub fn abort(&mut self) {
        self.pending.clear();
    }

    pub fn snapshot(&self) -> Records {
        match &self.backing {
            Backing::Memory(records) => records.clone(),
            #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
            Backing::Host(_) => Records::new(),
        }
    }

    pub fn install(&mut self, records: Records) -> Result<(), Error> {
        match &mut self.backing {
            Backing::Memory(committed) => *committed = records,
            #[cfg(any(test, all(feature = "guest", target_arch = "wasm32")))]
            Backing::Host(_) => {
                if !records.is_empty() {
                    return Err(Error::Module {
                        reason: refusal::UNSUPPORTED.into(),
                        sentence: "a host-backed receipt store restores from host state, not from snapshot records".into(),
                    });
                }
            }
        }
        self.pending.clear();
        Ok(())
    }

    #[cfg(test)]
    pub fn staged(&self) -> &BTreeMap<String, Option<Vec<u8>>> {
        &self.pending
    }
}
