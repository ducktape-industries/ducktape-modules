//! What a view keeps on this device between runs: its own keys, on the
//! network it runs on (`store.get` / `store.set`), borsh-encoded. The host
//! keeps them apart per view and per network; no other view reads them.
use std::future::Future;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::host::{malformed, Host, Refusal};
use crate::methods::{self, StoreGet, StoreSet};

/// The value kept under `key`, or `None` where nothing is.
pub fn get<T: BorshDeserialize>(
    host: &Host,
    key: &str,
) -> impl Future<Output = Result<Option<T>, Refusal>> + 'static {
    let asked = host.ask::<StoreGet>(key.to_owned());
    async move {
        asked
            .await?
            .map(|bytes| methods::decode(&bytes).map_err(malformed))
            .transpose()
    }
}

/// Keep `value` under `key`; `None` drops it. Nobody waits for the answer.
pub fn set<T: BorshSerialize>(host: &Host, key: &str, value: Option<&T>) {
    host.notify::<StoreSet>((key.to_owned(), value.map(methods::encode)));
}
