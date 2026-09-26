//! The validators role: the consensus set the kernel seats each epoch.
//! [`run`] checks every rule below.

use abi::role::validators::{Genesis, Member, Query, Reply};
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{Error, MockHost, Module};

use crate::{ask, init, same_bytes};

/// What the suite needs from the module beyond the role: its own update
/// path, which the role leaves to it.
pub trait Fixture {
    type Module: Module<Query: BorshSerialize, Response: BorshDeserialize>;

    /// A host before founding; the suite runs the module's init.
    fn host(&self) -> MockHost {
        MockHost::default()
    }

    /// Seats `member` as a validator, by the module's own op.
    fn seat(&self, host: &MockHost, member: Member) -> Result<(), Error>;

    /// Unseats the validator `key`, by the module's own op.
    fn unseat(&self, host: &MockHost, key: &[u8]) -> Result<(), Error>;
}

/// Every validators rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_first_variants(fixture);
    genesis_seats_the_given_set(fixture);
    an_update_changes_the_answer(fixture);
}

fn module() -> String {
    MockHost::roles().validators
}

fn member(n: u8) -> Member {
    Member {
        key: vec![n; 32],
        address: format!("node-{n}"),
    }
}

/// A host founded with validators 1 and 2.
fn founded<F: Fixture>(fixture: &F) -> MockHost {
    let host = fixture.host();
    let genesis = Genesis {
        validators: vec![member(2), member(1)],
    };
    init::<F::Module>(&host, &module(), &genesis);
    host
}

/// Validators and Members answer, every key is 32 bytes, and every
/// validator is a member. The validators' keys, sorted.
fn answers<F: Fixture>(host: &MockHost) -> (Vec<Vec<u8>>, Vec<Member>) {
    let m = module();
    let mut validators = match ask::<F::Module, Reply>(host, &m, 1, &Query::Validators) {
        Ok(Reply::Validators(keys)) => keys,
        other => panic!("validators: Validators answers Validators, not {other:?}"),
    };
    let members = match ask::<F::Module, Reply>(host, &m, 1, &Query::Members) {
        Ok(Reply::Members(members)) => members,
        other => panic!("validators: Members answers Members, not {other:?}"),
    };
    for key in validators.iter().chain(members.iter().map(|m| &m.key)) {
        assert_eq!(key.len(), 32, "validators: a key is 32 bytes: {key:?}");
    }
    for key in &validators {
        assert!(
            members.iter().any(|m| &m.key == key),
            "validators: validator {key:?} is among the Members"
        );
    }
    validators.sort();
    (validators, members)
}

/// The role's queries and replies are the module's first variants, byte
/// for byte.
pub fn the_role_is_the_first_variants<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Module;
    let m = module();
    for query in [Query::Validators, Query::Members] {
        same_bytes::<<M<F> as Module>::Query>(&m, "Query", &query);
    }
    for reply in [
        Reply::Validators(vec![vec![1; 32]]),
        Reply::Members(vec![member(1)]),
    ] {
        same_bytes::<<M<F> as Module>::Response>(&m, "Response", &reply);
    }
}

/// Init with the role's genesis seats exactly its validators, each a
/// member at its address.
pub fn genesis_seats_the_given_set<F: Fixture>(fixture: &F) {
    let host = founded(fixture);
    let (validators, members) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(1).key, member(2).key],
        "validators: genesis seats exactly its validators"
    );
    for seated in [member(1), member(2)] {
        assert!(
            members.contains(&seated),
            "validators: genesis member {seated:?} is among the Members"
        );
    }
}

/// Seating and unseating through the module's update path change what
/// Validators and Members answer.
pub fn an_update_changes_the_answer<F: Fixture>(fixture: &F) {
    let host = founded(fixture);
    fixture
        .seat(&host, member(3))
        .unwrap_or_else(|e| panic!("validators: seating a validator: {e:?}"));
    let (validators, members) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(1).key, member(2).key, member(3).key],
        "validators: a seated validator is answered"
    );
    assert!(
        members.contains(&member(3)),
        "validators: a seated validator is a member"
    );
    fixture
        .unseat(&host, &member(1).key)
        .unwrap_or_else(|e| panic!("validators: unseating a validator: {e:?}"));
    let (validators, _) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(2).key, member(3).key],
        "validators: an unseated validator is no longer answered"
    );
}
