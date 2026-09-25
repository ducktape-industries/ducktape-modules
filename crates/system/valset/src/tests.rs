// The module natively over `guest::MockHost`: what the founding suite checks on the host, without the host.

use abi::{Cause, Env, Origin, reason};
use guest::{MockHost, Module};
use module_registry::AUTHORITY;
use store::Page;

use crate::{Genesis, Member, Membership, Op, Query, Reply, Standing, Valset};

fn key(n: u8) -> Vec<u8> {
    vec![n; 32]
}

fn env(origin: Origin) -> Env {
    Env {
        network: b"net".to_vec(),
        height: 3,
        time: 0,
        me: crate::PROGRAM.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn founded() -> MockHost {
    let store = MockHost::default();
    let genesis = abi::encode(&Genesis {
        validators: vec![
            Member {
                key: key(2),
                address: "b".into(),
            },
            Member {
                key: key(1),
                address: "a".into(),
            },
        ],
    });
    Valset::init(&store.exec(env(Origin::System)), &genesis).unwrap();
    store
}

fn govern(store: &MockHost, op: Op) -> Result<(), abi::Refusal> {
    Valset::execute(&store.exec(env(Origin::Program(AUTHORITY.into()))), op)
}

fn ask(store: &MockHost, query: Query) -> Reply {
    Valset::query(&store.query(env(Origin::System)), query).unwrap()
}

fn membership(n: u8, standing: Standing) -> Membership {
    Membership {
        key: key(n),
        address: format!("node-{n}"),
        standing,
    }
}

#[test]
fn founding_seats_the_validators_in_key_order() {
    let store = founded();
    assert_eq!(
        ask(&store, Query::Validators),
        Reply::Validators(vec![key(1), key(2)])
    );
    let Reply::Members(members) = ask(&store, Query::Members) else {
        panic!()
    };
    assert_eq!(members[0].address, "a");
}

#[test]
fn only_the_authority_writes_and_a_key_is_32_bytes() {
    let store = founded();
    let stranger = Valset::execute(
        &store.exec(env(Origin::External(key(9)))),
        Op::Set(membership(3, Standing::Resident)),
    );
    assert_eq!(stranger.unwrap_err().reason, reason::UNAUTHORIZED);
    let short = govern(
        &store,
        Op::Set(Membership {
            key: vec![1, 2],
            address: "x".into(),
            standing: Standing::Resident,
        }),
    );
    assert_eq!(short.unwrap_err().reason, reason::INVALID_INPUT);
    govern(&store, Op::Set(membership(3, Standing::Resident))).unwrap();
    assert_eq!(
        ask(&store, Query::Membership { key: key(3) }),
        Reply::Membership(Some(membership(3, Standing::Resident)))
    );
    assert_eq!(
        ask(&store, Query::Validators),
        Reply::Validators(vec![key(1), key(2)])
    );
}

#[test]
fn the_last_validator_stays_seated() {
    let store = founded();
    govern(&store, Op::Remove { key: key(1) }).unwrap();
    let demote = govern(&store, Op::Set(membership(2, Standing::Resident)));
    assert_eq!(demote.unwrap_err().reason, reason::WRONG_STATE);
    let remove = govern(&store, Op::Remove { key: key(2) });
    assert_eq!(remove.unwrap_err().reason, reason::WRONG_STATE);
    govern(&store, Op::Set(membership(5, Standing::Validator))).unwrap();
    govern(&store, Op::Remove { key: key(2) }).unwrap();
    assert_eq!(
        ask(&store, Query::Validators),
        Reply::Validators(vec![key(5)])
    );
}

#[test]
fn memberships_page_in_key_order_at_the_answering_height() {
    let store = founded();
    govern(&store, Op::Set(membership(3, Standing::Resident))).unwrap();
    let Reply::Memberships(first) = ask(
        &store,
        Query::Memberships {
            page: Page::first(2),
        },
    ) else {
        panic!()
    };
    assert_eq!(first.height, 3);
    assert_eq!(
        first.items.iter().map(|m| m.key[0]).collect::<Vec<_>>(),
        [1, 2]
    );
    let Reply::Memberships(rest) = ask(
        &store,
        Query::Memberships {
            page: Page {
                after: first.next,
                limit: Some(2),
            },
        },
    ) else {
        panic!()
    };
    assert_eq!(rest.items, [membership(3, Standing::Resident)]);
    assert_eq!(rest.next, None);
}

#[test]
fn the_host_contract_is_a_prefix_of_the_program_contract() {
    assert_eq!(
        abi::encode(&abi::valset::Query::Validators),
        abi::encode(&super::Query::Validators)
    );
    assert_eq!(
        abi::encode(&abi::valset::Query::Members),
        abi::encode(&super::Query::Members)
    );
    let member = abi::valset::Member {
        key: vec![1],
        address: "a".into(),
    };
    assert_eq!(
        abi::encode(&abi::valset::Reply::Validators(vec![vec![1]])),
        abi::encode(&super::Reply::Validators(vec![vec![1]]))
    );
    assert_eq!(
        abi::encode(&abi::valset::Reply::Members(vec![member.clone()])),
        abi::encode(&super::Reply::Members(vec![member]))
    );
}
