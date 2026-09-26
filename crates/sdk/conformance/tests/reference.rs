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
        let manager_key = [key, b"'s manager"].concat();
        let manager = Self::person(host, &manager_key);
        let as_manager = |op| Self::run(host, &manager_key, Some(manager), op);
        let agent = as_manager(identity::Op::CreateAgent {
            name: "agent".into(),
        });
        as_manager(identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: None,
            consent: identity::Consent {
                key: key.to_vec(),
                account: agent,
                expires_at: u64::MAX,
                proof: vec![],
            },
        });
        as_manager(identity::Op::Suspend { account: agent });
        Some(agent)
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
