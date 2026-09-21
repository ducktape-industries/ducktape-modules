//! The `identity` program: numbered accounts, each held by a set of keys and
//! carrying a name. The frame's origin key is the actor for every op; a
//! program origin is refused.
//!
//! - `Create` founds an account for the origin key.
//! - `AddKey` admits a key into the origin's account.
//! - `RemoveKey` drops a key from the origin's account, never the last one.
//! - `SetName` renames the origin's account; a name is held by one account.
//!
//! Keys on disk: `acct/<number>` → `Account`, `key/<pubkey>` → number,
//! `name/<name>` → number, `next` → the next number.
use abi::{Refusal, Scan, reason};
use borsh::{BorshDeserialize, BorshSerialize};

pub type AccountNumber = u64;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    pub number: AccountNumber,
    pub keys: Vec<Vec<u8>>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Msg {
    Create { name: String },
    AddKey { key: Vec<u8> },
    RemoveKey { key: Vec<u8> },
    SetName { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    OfKey(Vec<u8>),
    Account(AccountNumber),
    ByName(String),
    List {
        after: Option<AccountNumber>,
        limit: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Account(Option<Account>),
    Accounts(Vec<Account>),
}

pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_KEYS: usize = 16;

/// What the program reads and writes, so the rules run natively in tests.
pub trait Store {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>);
    fn delete(&mut self, key: &[u8]);
    fn scan(&self, scan: Scan) -> Vec<abi::Entry>;
}

pub fn account_key(number: AccountNumber) -> Vec<u8> {
    format!("acct/{number:020}").into_bytes()
}
fn key_key(key: &[u8]) -> Vec<u8> {
    [b"key/", key].concat()
}
fn name_key(name: &str) -> Vec<u8> {
    format!("name/{name}").into_bytes()
}

fn refuse(reason: &str, sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason, sentence)
}

fn load(store: &dyn Store, number: AccountNumber) -> Result<Account, Refusal> {
    let bytes = store
        .get(&account_key(number))
        .ok_or_else(|| refuse(reason::NOT_FOUND, format!("no account {number}")))?;
    abi::decode(&bytes)
}

fn number_at(store: &dyn Store, key: &[u8]) -> Option<AccountNumber> {
    store.get(key).and_then(|b| abi::decode(&b).ok())
}

fn of_key(store: &dyn Store, key: &[u8]) -> Result<Account, Refusal> {
    let number = number_at(store, &key_key(key))
        .ok_or_else(|| refuse(reason::NOT_FOUND, "this key holds no account"))?;
    load(store, number)
}

fn checked_name(store: &dyn Store, name: &str) -> Result<(), Refusal> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.contains('/') {
        return Err(refuse(
            reason::INVALID_INPUT,
            format!("a name is 1..={MAX_NAME_BYTES} bytes without '/'"),
        ));
    }
    if number_at(store, &name_key(name)).is_some() {
        return Err(refuse(reason::INVALID_INPUT, format!("{name} is taken")));
    }
    Ok(())
}

fn save(store: &mut dyn Store, account: &Account) {
    store.set(account_key(account.number), abi::encode(account));
}

/// Applies `msg` as the account the origin `actor` key holds.
pub fn execute(store: &mut dyn Store, actor: &[u8], msg: Msg) -> Result<(), Refusal> {
    match msg {
        Msg::Create { name } => {
            if number_at(store, &key_key(actor)).is_some() {
                return Err(refuse(
                    reason::INVALID_INPUT,
                    "this key already holds an account",
                ));
            }
            checked_name(store, &name)?;
            let number = store
                .get(b"next")
                .map(|b| abi::decode::<AccountNumber>(&b))
                .transpose()?
                .unwrap_or(1);
            let account = Account {
                number,
                keys: vec![actor.to_vec()],
                name: name.clone(),
            };
            save(store, &account);
            store.set(key_key(actor), abi::encode(&number));
            store.set(name_key(&name), abi::encode(&number));
            store.set(b"next".to_vec(), abi::encode(&(number + 1)));
        }
        Msg::AddKey { key } => {
            let mut account = of_key(store, actor)?;
            if number_at(store, &key_key(&key)).is_some() {
                return Err(refuse(
                    reason::INVALID_INPUT,
                    "that key already holds an account",
                ));
            }
            if account.keys.len() >= MAX_KEYS {
                return Err(refuse(
                    reason::INVALID_INPUT,
                    format!("an account holds at most {MAX_KEYS} keys"),
                ));
            }
            store.set(key_key(&key), abi::encode(&account.number));
            account.keys.push(key);
            save(store, &account);
        }
        Msg::RemoveKey { key } => {
            let mut account = of_key(store, actor)?;
            if !account.keys.contains(&key) {
                return Err(refuse(reason::NOT_FOUND, "that key is not on this account"));
            }
            if account.keys.len() == 1 {
                return Err(refuse(
                    reason::INVALID_INPUT,
                    "an account keeps its last key",
                ));
            }
            account.keys.retain(|k| *k != key);
            store.delete(&key_key(&key));
            save(store, &account);
        }
        Msg::SetName { name } => {
            let mut account = of_key(store, actor)?;
            checked_name(store, &name)?;
            store.delete(&name_key(&account.name));
            store.set(name_key(&name), abi::encode(&account.number));
            account.name = name;
            save(store, &account);
        }
    }
    Ok(())
}

pub fn query(store: &dyn Store, query: Query) -> Result<Reply, Refusal> {
    Ok(match query {
        Query::OfKey(key) => Reply::Account(of_key(store, &key).ok()),
        Query::Account(number) => Reply::Account(load(store, number).ok()),
        Query::ByName(name) => {
            Reply::Account(number_at(store, &name_key(&name)).and_then(|n| load(store, n).ok()))
        }
        Query::List { after, limit } => {
            let mut scan = Scan::prefix(b"acct/").limit(limit.min(256));
            if let Some(after) = after {
                scan = scan.after(account_key(after));
            }
            Reply::Accounts(
                store
                    .scan(scan)
                    .into_iter()
                    .map(|entry| abi::decode(&entry.value))
                    .collect::<Result<_, _>>()?,
            )
        }
    })
}

#[cfg(target_arch = "wasm32")]
mod program {
    use abi::{Origin, Refusal, Scan, reason};
    use guest::Program;

    struct Host;
    impl super::Store for Host {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            guest::get(key)
        }
        fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
            guest::set(key, value)
        }
        fn delete(&mut self, key: &[u8]) {
            guest::delete(key.to_vec())
        }
        fn scan(&self, scan: Scan) -> Vec<abi::Entry> {
            guest::scan(scan)
        }
    }

    struct Identity;
    impl Program for Identity {
        fn execute(payload: &[u8]) -> Result<(), Refusal> {
            let Origin::External(actor) = guest::env().origin else {
                return Err(Refusal::new(
                    reason::UNSUPPORTED,
                    "only a key acts on identity",
                ));
            };
            super::execute(&mut Host, &actor, abi::decode(payload)?)
        }
        fn query(request: &[u8]) -> Result<(), Refusal> {
            let reply = super::query(&Host, abi::decode(request)?)?;
            guest::respond(abi::encode(&reply));
            Ok(())
        }
    }

    guest::program!(Identity);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Mem(BTreeMap<Vec<u8>, Vec<u8>>);
    impl Store for Mem {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.0.get(key).cloned()
        }
        fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
            self.0.insert(key, value);
        }
        fn delete(&mut self, key: &[u8]) {
            self.0.remove(key);
        }
        fn scan(&self, scan: Scan) -> Vec<abi::Entry> {
            let mut hits: Vec<abi::Entry> = self
                .0
                .iter()
                .filter(|(k, _)| scan.admits(k))
                .map(|(k, v)| abi::Entry {
                    key: k.clone(),
                    value: v.clone(),
                })
                .collect();
            if scan.reverse {
                hits.reverse();
            }
            if let Some(limit) = scan.limit {
                hits.truncate(limit as usize);
            }
            hits
        }
    }

    #[test]
    fn accounts_are_numbered_keys_move_and_the_last_key_stays() {
        let mut store = Mem::default();
        execute(&mut store, b"ada", Msg::Create { name: "ada".into() }).unwrap();
        execute(&mut store, b"bob", Msg::Create { name: "bob".into() }).unwrap();
        assert_eq!(
            execute(&mut store, b"eve", Msg::Create { name: "ada".into() })
                .unwrap_err()
                .reason,
            reason::INVALID_INPUT
        );
        execute(
            &mut store,
            b"ada",
            Msg::AddKey {
                key: b"ada2".to_vec(),
            },
        )
        .unwrap();
        assert_eq!(
            execute(
                &mut store,
                b"ada",
                Msg::AddKey {
                    key: b"bob".to_vec()
                }
            )
            .unwrap_err()
            .reason,
            reason::INVALID_INPUT
        );
        let Reply::Account(Some(ada)) = query(&store, Query::OfKey(b"ada2".to_vec())).unwrap()
        else {
            panic!("ada2 is ada's key")
        };
        assert_eq!((ada.number, ada.keys.len()), (1, 2));
        execute(
            &mut store,
            b"ada2",
            Msg::RemoveKey {
                key: b"ada".to_vec(),
            },
        )
        .unwrap();
        assert_eq!(
            execute(
                &mut store,
                b"ada2",
                Msg::RemoveKey {
                    key: b"ada2".to_vec()
                }
            )
            .unwrap_err()
            .sentence,
            "an account keeps its last key"
        );
        execute(
            &mut store,
            b"ada2",
            Msg::SetName {
                name: "lovelace".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            query(&store, Query::ByName("ada".into())).unwrap(),
            Reply::Account(None)
        ));
        let Reply::Accounts(all) = query(
            &store,
            Query::List {
                after: None,
                limit: 10,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            all.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["lovelace", "bob"]
        );
        let Reply::Accounts(rest) = query(
            &store,
            Query::List {
                after: Some(1),
                limit: 10,
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(rest.len(), 1);
        assert!(execute(&mut store, b"nobody", Msg::SetName { name: "x".into() }).is_err());
    }
}
