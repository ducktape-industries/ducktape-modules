// The module natively over `guest::MockHost`: what the founding suite checks on the host, without the host.

use guest::{Cause, Env, Origin, Scheme, code};
use guest::{MockHost, Module};
use store::PageRequest;

use crate::{
    Account, Admission, CONSENT_NAMESPACE, Consent, Control, Identity, Op, Query, Reply, Status,
};

const ALICE: &[u8] = b"alice-key";
const SECOND: &[u8] = b"alice-second-key";
const EXECUTOR: &str = "agents";

fn env(origin: Origin, time: u64) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height: 7,
        time,
        module: crate::MODULE.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn signed(key: &[u8]) -> Env {
    env(Origin::Signed(key.to_vec()), 100)
}

fn by_program() -> Env {
    env(Origin::Module(EXECUTOR.into()), 100)
}

/// A consent proof, natively: the signature is the preimage itself, and the
/// verifier checks it names the consenting key.
fn memory() -> MockHost {
    let host = MockHost::default();
    host.borrow_mut().verifier = Some(Box::new(|_, key, namespace, message, signature| {
        namespace == CONSENT_NAMESPACE && message == signature && !key.is_empty()
    }));
    host
}

fn run(store: &MockHost, env: &Env, op: Op) -> Result<u64, guest::Error> {
    Identity::execute(&store.exec(env.clone()), op)?;
    let output = store.take_output();
    Ok(if output.is_empty() {
        0
    } else {
        abi::decode(&output).unwrap()
    })
}

fn get(store: &MockHost, number: u64) -> Account {
    match Identity::query(&store.query(signed(ALICE)), Query::Get { number }).unwrap() {
        Reply::Account(account) => account.expect("the account exists"),
        other => panic!("{other:?}"),
    }
}

fn create(store: &MockHost, key: &[u8], name: &str) -> u64 {
    run(
        store,
        &signed(key),
        Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        },
    )
    .unwrap()
}

#[test]
fn accounts_are_numbered_from_one_and_a_key_holds_one_account() {
    let store = memory();
    assert_eq!(create(&store, ALICE, "  Alice "), 1);
    assert_eq!(get(&store, 1).name, "Alice");
    assert_eq!(create(&store, b"bob", "Bob"), 2);
    let again = run(
        &store,
        &signed(ALICE),
        Op::Create {
            name: "Twice".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(again.unwrap_err().code, code::ALREADY_EXISTS);
    assert_eq!(
        Identity::query(
            &store.query(signed(ALICE)),
            Query::OfKey {
                key: ALICE.to_vec()
            }
        )
        .unwrap(),
        Reply::Number(Some(1))
    );
    let unsigned = Identity::execute(
        &store.exec(by_program()),
        Op::Create {
            name: "x".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(unsigned.unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn a_key_joins_by_consent_and_leaves_only_junior_to_its_remover() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let admission = Admission {
        network: b"net".to_vec(),
        scheme: Scheme::Ed25519,
        key: SECOND.to_vec(),
        generation: 0,
        account: 1,
        expires_at: 200,
    };
    let consent = |proof: Vec<u8>, expires_at: u64| Consent {
        key: ALICE.to_vec(),
        account: 1,
        expires_at,
        proof,
    };
    let add = |proof: Vec<u8>, expires_at: u64| Op::AddKey {
        scheme: Scheme::Ed25519,
        label: Some("laptop".into()),
        consent: consent(proof, expires_at),
    };
    let forged = run(&store, &signed(SECOND), add(b"nope".to_vec(), 200));
    assert_eq!(forged.unwrap_err().code, code::UNAUTHORIZED);
    let expired = run(
        &store,
        &env(Origin::Signed(SECOND.to_vec()), 300),
        add(admission.preimage(), 200),
    );
    assert_eq!(expired.unwrap_err().code, code::UNAUTHORIZED);
    run(
        &store,
        &env(Origin::Signed(SECOND.to_vec()), 150),
        add(admission.preimage(), 200),
    )
    .unwrap();
    assert_eq!(get(&store, 1).keys().len(), 2);
    assert_eq!(
        Identity::query(
            &store.query(signed(ALICE)),
            Query::Generation {
                key: SECOND.to_vec()
            }
        )
        .unwrap(),
        Reply::Generation(1)
    );
    let senior = run(
        &store,
        &env(Origin::Signed(SECOND.to_vec()), 150),
        Op::RemoveKey {
            key: ALICE.to_vec(),
        },
    );
    assert_eq!(senior.unwrap_err().code, code::UNAUTHORIZED);
    run(
        &store,
        &env(Origin::Signed(ALICE.to_vec()), 150),
        Op::RemoveKey {
            key: SECOND.to_vec(),
        },
    )
    .unwrap();
    assert!(!get(&store, 1).holds(SECOND));
    let last = run(
        &store,
        &signed(ALICE),
        Op::RemoveKey {
            key: ALICE.to_vec(),
        },
    );
    assert_eq!(last.unwrap_err().code, code::WRONG_STATE);
}

#[test]
fn a_program_account_is_controlled_transferred_and_revoked_by_its_controller() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, b"bob", "Bob");
    let agent = run(
        &store,
        &by_program(),
        Op::CreateProgram {
            name: "Agent".into(),
            controller: 1,
        },
    )
    .unwrap();
    assert_eq!(agent, 3);
    let stranger = run(
        &store,
        &signed(b"bob"),
        Op::SetName {
            account: 3,
            name: "Mine".into(),
        },
    );
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);
    run(
        &store,
        &by_program(),
        Op::SetStatus {
            account: 3,
            status: Status::Suspended,
        },
    )
    .unwrap();
    assert!(!get(&store, 3).live());
    let circular = run(
        &store,
        &signed(ALICE),
        Op::TransferControl { account: 3, to: 3 },
    );
    assert_eq!(circular.unwrap_err().code, code::WRONG_STATE);
    run(
        &store,
        &signed(ALICE),
        Op::TransferControl { account: 3, to: 2 },
    )
    .unwrap();
    assert!(matches!(
        get(&store, 3).control,
        Control::Program { controller: 2, .. }
    ));
    let former = run(&store, &signed(ALICE), Op::Revoke { account: 3 });
    assert_eq!(former.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(b"bob"), Op::Revoke { account: 3 }).unwrap();
    assert_eq!(get(&store, 3).control, Control::Revoked { controller: 2 });
}

#[test]
fn lists_page_in_number_order_and_controlled_lists_one_controller() {
    let store = memory();
    for n in 0..11u8 {
        create(&store, &[n], &format!("a{n}"));
    }
    for _ in 0..3 {
        run(
            &store,
            &by_program(),
            Op::CreateProgram {
                name: "agent".into(),
                controller: 2,
            },
        )
        .unwrap();
    }
    run(
        &store,
        &by_program(),
        Op::CreateProgram {
            name: "other".into(),
            controller: 11,
        },
    )
    .unwrap();
    let list = |query| match Identity::query(&store.query(signed(ALICE)), query).unwrap() {
        Reply::Accounts(page) => page,
        other => panic!("{other:?}"),
    };
    let first = list(Query::List {
        page: PageRequest::first(10),
    });
    assert_eq!(first.height, 7);
    assert_eq!(first.items.len(), 10);
    let rest = list(Query::List {
        page: PageRequest {
            after: first.next,
            limit: Some(10),
        },
    });
    assert_eq!(
        rest.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [11, 12, 13, 14, 15],
        "numeric order across the ten boundary"
    );
    let controlled = list(Query::Controlled {
        by: 2,
        page: PageRequest::first(2),
    });
    assert_eq!(
        controlled
            .items
            .iter()
            .map(|a| a.number)
            .collect::<Vec<_>>(),
        [12, 13]
    );
    let more = list(Query::Controlled {
        by: 2,
        page: PageRequest {
            after: controlled.next,
            limit: Some(2),
        },
    });
    assert_eq!(
        (
            more.items.iter().map(|a| a.number).collect::<Vec<_>>(),
            more.next
        ),
        (vec![14], None),
        "the page stays under one controller"
    );
    assert_eq!(
        Identity::query(
            &store.query(signed(ALICE)),
            Query::Resolve {
                references: vec![
                    crate::Reference::Account(15),
                    crate::Reference::Account(99),
                    crate::Reference::Key(vec![3]),
                ],
            }
        )
        .unwrap(),
        Reply::Resolved(vec![Some(15), None, Some(4)])
    );
}
