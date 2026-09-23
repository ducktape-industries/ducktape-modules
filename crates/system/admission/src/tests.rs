// The rules natively over `store::Memory`: what the founding suite checks on the host, without the host.

use abi::{Cause, Env, Origin, reason};
use store::Memory;
use valset::{Membership, Standing};

use crate::Op;

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

/// A store whose valset answers `Membership` from `seated`.
fn with_valset(seated: Vec<Membership>) -> Memory {
    let mut store = Memory::default();
    store.siblings.insert(
        valset::PROGRAM.into(),
        Box::new(move |request| {
            let reply = match abi::decode::<valset::Query>(request)? {
                valset::Query::Membership { key } => {
                    valset::Reply::Membership(seated.iter().find(|m| m.key == key).cloned())
                }
                other => panic!("admission asked valset {other:?}"),
            };
            Ok(abi::encode(&reply))
        }),
    );
    store
}

fn enroll(store: &mut Memory, signer: Vec<u8>, address: &str) -> Result<valset::Op, abi::Refusal> {
    crate::execute(
        store,
        &env(Origin::External(signer)),
        Op::Enroll {
            address: address.into(),
        },
    )?;
    let sent = store.take_emissions();
    assert_eq!(sent.len(), 1, "one write to valset");
    assert_eq!(sent[0].target, valset::PROGRAM);
    abi::decode(&sent[0].payload)
}

#[test]
fn a_stranger_enrolls_as_a_resident_at_the_address_it_names() {
    let mut store = with_valset(Vec::new());
    let written = enroll(&mut store, key(3), "node-3:9000").unwrap();
    assert_eq!(
        written,
        valset::Op::Set(Membership {
            key: key(3),
            address: "node-3:9000".into(),
            standing: Standing::Resident,
        })
    );
}

#[test]
fn a_validator_keeps_its_standing_and_moves_its_address() {
    let mut store = with_valset(vec![Membership {
        key: key(1),
        address: "old".into(),
        standing: Standing::Validator,
    }]);
    let written = enroll(&mut store, key(1), "new").unwrap();
    assert_eq!(
        written,
        valset::Op::Set(Membership {
            key: key(1),
            address: "new".into(),
            standing: Standing::Validator,
        })
    );
}

#[test]
fn only_a_signed_frame_enrolls() {
    let mut store = with_valset(Vec::new());
    let refused = crate::execute(
        &mut store,
        &env(Origin::Program("probe".into())),
        Op::Enroll {
            address: "x".into(),
        },
    )
    .unwrap_err();
    assert_eq!(refused.reason, reason::UNAUTHORIZED);
    assert!(store.take_emissions().is_empty());
}
