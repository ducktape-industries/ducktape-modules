// The rules natively over `store::Memory`: what the founding suite checks on the host, without the host.

use abi::{Cause, Env, Origin, reason};
use module_registry::ADMISSION;
use store::{Memory, Page};

use crate::{Genesis, Member, Membership, Op, Query, Reply, Standing};

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

fn founded() -> Memory {
    let mut store = Memory::default();
    crate::init(
        &mut store,
        Genesis {
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
        },
    )
    .unwrap();
    store
}

fn govern(store: &mut Memory, op: Op) -> Result<(), abi::Refusal> {
    crate::execute(store, &env(Origin::Program(ADMISSION.into())), op)
}

fn ask(store: &Memory, query: Query) -> Reply {
    crate::query(store, &env(Origin::System), query).unwrap()
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
fn only_admission_writes_and_a_key_is_32_bytes() {
    let mut store = founded();
    let stranger = crate::execute(
        &mut store,
        &env(Origin::External(key(9))),
        Op::Set(membership(3, Standing::Resident)),
    );
    assert_eq!(stranger.unwrap_err().reason, reason::UNAUTHORIZED);
    let short = govern(
        &mut store,
        Op::Set(Membership {
            key: vec![1, 2],
            address: "x".into(),
            standing: Standing::Resident,
        }),
    );
    assert_eq!(short.unwrap_err().reason, reason::INVALID_INPUT);
    govern(&mut store, Op::Set(membership(3, Standing::Resident))).unwrap();
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
    let mut store = founded();
    govern(&mut store, Op::Remove { key: key(1) }).unwrap();
    let demote = govern(&mut store, Op::Set(membership(2, Standing::Resident)));
    assert_eq!(demote.unwrap_err().reason, reason::WRONG_STATE);
    let remove = govern(&mut store, Op::Remove { key: key(2) });
    assert_eq!(remove.unwrap_err().reason, reason::WRONG_STATE);
    govern(&mut store, Op::Set(membership(5, Standing::Validator))).unwrap();
    govern(&mut store, Op::Remove { key: key(2) }).unwrap();
    assert_eq!(
        ask(&store, Query::Validators),
        Reply::Validators(vec![key(5)])
    );
}

#[test]
fn memberships_page_in_key_order_at_the_answering_height() {
    let mut store = founded();
    govern(&mut store, Op::Set(membership(3, Standing::Resident))).unwrap();
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

#[test]
fn a_full_valset_refuses_a_new_member_and_still_moves_one() {
    let mut store = founded();
    for n in 0..crate::MAX_MEMBERS as u64 - 2 {
        let resident = Membership {
            key: n.to_be_bytes().repeat(4),
            address: format!("node-{n}"),
            standing: Standing::Resident,
        };
        govern(&mut store, Op::Set(resident)).unwrap();
    }
    let refused = govern(&mut store, Op::Set(membership(9, Standing::Resident))).unwrap_err();
    assert_eq!(refused.reason, abi::reason::CAPACITY);
    let moved = Membership {
        address: "moved".into(),
        ..membership(1, Standing::Validator)
    };
    govern(&mut store, Op::Set(moved)).unwrap();
}
