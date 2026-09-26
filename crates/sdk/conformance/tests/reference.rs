// The suite run on the reference modules: each fixture below is what a
// third-party module writes to prove it fills a role.

use abi::role::registry::{Entry, View};
use abi::role::validators::Member;
use conformance::env;
use guest::{AccountNumber, BlobId, Error, MockHost, Module, Origin, Principal, Scheme};

struct Identity;

impl Identity {
    fn run(host: &MockHost, key: &[u8], as_: Option<AccountNumber>, op: identity::Op) -> u64 {
        let env = env(
            "identity",
            1,
            Origin::Signed(key.to_vec()),
            as_.map(Principal::Account),
        );
        identity::Identity::execute(&host.exec(env), op).unwrap();
        let output = host.take_output();
        if output.is_empty() {
            0
        } else {
            abi::decode(&output).unwrap()
        }
    }

    /// `key` joins `account`, consenting (every consent verifies here).
    fn add_key(key: &[u8], account: AccountNumber) -> identity::Op {
        identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: None,
            consent: identity::Consent {
                key: key.to_vec(),
                account,
                expires_at: u64::MAX,
                proof: vec![],
            },
        }
    }

    fn get(host: &MockHost, number: AccountNumber) -> identity::Account {
        let env = env("identity", 1, Origin::Root, None);
        match identity::Identity::query(&host.query(env), identity::Query::Get { number }) {
            Ok(identity::Reply::Account(Some(account))) => account,
            other => panic!("Get({number}) answers the account, not {other:?}"),
        }
    }

    /// `op`, signed by `agent`'s manager.
    fn as_manager(host: &MockHost, agent: AccountNumber, op: identity::Op) {
        let identity::Control::Managed { manager, .. } = Self::get(host, agent).control else {
            panic!("{agent} is managed");
        };
        let key = Self::get(host, manager).keys()[0].key.clone();
        Self::run(host, &key, Some(manager), op);
    }

    fn person(host: &MockHost, key: &[u8]) -> AccountNumber {
        let op = identity::Op::Create {
            name: "person".into(),
            scheme: Scheme::Ed25519,
        };
        Self::run(host, key, None, op)
    }
}

impl conformance::identity::Fixture for Identity {
    type Module = identity::Identity;

    fn host(&self) -> MockHost {
        let host = MockHost::default();
        // every consent verifies: the suite checks the role, not signatures
        host.borrow_mut().verifier = Some(Box::new(|_, _, _, _, _| true));
        host
    }

    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber {
        Self::person(host, key)
    }

    /// A suspended agent holding `key`.
    fn stopped(&self, host: &MockHost, key: &[u8]) -> Option<AccountNumber> {
        let agent = self.managed(host, key)?;
        Self::as_manager(host, agent, identity::Op::Suspend { account: agent });
        Some(agent)
    }

    /// A person keeps their last key: a spare joins, then `key` goes.
    fn drop_key(&self, host: &MockHost, account: AccountNumber, key: &[u8]) -> bool {
        let spare = [key, b"'s spare"].concat();
        Self::run(host, &spare, None, Self::add_key(key, account));
        let remove = identity::Op::RemoveKey {
            account,
            key: key.to_vec(),
        };
        Self::run(host, key, Some(account), remove);
        true
    }

    /// An active agent holding `key`.
    fn managed(&self, host: &MockHost, key: &[u8]) -> Option<AccountNumber> {
        let manager_key = [key, b"'s manager"].concat();
        let manager = Self::person(host, &manager_key);
        let create = identity::Op::CreateAgent {
            name: "agent".into(),
        };
        let agent = Self::run(host, &manager_key, Some(manager), create);
        Self::as_manager(host, agent, Self::add_key(key, agent));
        Some(agent)
    }

    fn revoke(&self, host: &MockHost, account: AccountNumber) {
        Self::as_manager(host, account, identity::Op::Revoke { account });
    }
}

#[test]
fn identity_fills_the_identity_role() {
    conformance::identity::run(&Identity);
}

struct Valset;

impl Valset {
    fn run(host: &MockHost, op: valset::Op) -> Result<(), Error> {
        let env = env("valset", 1, Origin::Signed(vec![9]), None);
        valset::Valset::execute(&host.exec(env), op)
    }
}

impl conformance::validators::Fixture for Valset {
    type Module = valset::Valset;

    fn seat(&self, host: &MockHost, member: Member) -> Result<(), Error> {
        let op = valset::Op::Set(valset::Membership {
            key: member.key,
            address: member.address,
            role: valset::Role::Validator,
        });
        Self::run(host, op)
    }

    fn unseat(&self, host: &MockHost, key: &[u8]) -> Result<(), Error> {
        Self::run(host, valset::Op::Remove { key: key.to_vec() })
    }
}

#[test]
fn valset_fills_the_validators_role() {
    conformance::validators::run(&Valset);
}

struct Registry;

impl Registry {
    fn run(host: &MockHost, height: u64, op: module_registry::Op) -> Result<(), Error> {
        let env = env("module-registry", height, Origin::Signed(vec![9]), None);
        module_registry::Modules::execute(&host.exec(env), op)
    }

    fn schedule(
        host: &MockHost,
        height: u64,
        at: u64,
        change: module_registry::Change,
    ) -> Result<(), Error> {
        let scheduled = module_registry::Scheduled { height: at, change };
        Self::run(host, height, module_registry::Op::Schedule(scheduled))
    }
}

impl conformance::registry::Fixture for Registry {
    type Module = module_registry::Modules;

    fn publish(&self, host: &MockHost, height: u64, body: &[u8]) -> Result<BlobId, Error> {
        let body = body.to_vec();
        Self::run(host, height, module_registry::Op::Publish { body })?;
        Ok(abi::decode(&host.take_output()).unwrap())
    }

    fn set(&self, host: &MockHost, height: u64, at: u64, entry: Entry) -> Result<(), Error> {
        Self::schedule(host, height, at, module_registry::Change::Set(entry))
    }

    fn remove(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error> {
        let change = module_registry::Change::Remove(program.into());
        Self::schedule(host, height, at, change)
    }

    fn views(&self, host: &MockHost, height: u64) -> Vec<View> {
        let env = env("module-registry", height, Origin::Root, None);
        let query = module_registry::Query::Views(height);
        match module_registry::Modules::query(&host.query(env), query).unwrap() {
            module_registry::Reply::Views(views) => views,
            other => panic!("Views answers Views, not {other:?}"),
        }
    }
}

#[test]
fn module_registry_fills_the_registry_role() {
    conformance::registry::run(&Registry);
}
