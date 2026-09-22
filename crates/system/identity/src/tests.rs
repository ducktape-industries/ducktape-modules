//! The founding suite's identity behaviours, natively over a `MemoryStore`.
use abi::{Cause, Env, Origin, reason};
use module_registry::Page;
use program::MemoryStore;

use super::*;

const TIME: u64 = 1_700_000_000_000;

fn signed(key: &[u8]) -> Env {
    env(Origin::External(key.to_vec()))
}

fn env(origin: Origin) -> Env {
    Env {
        network: b"net".to_vec(),
        height: 7,
        time: TIME,
        me: PROGRAM.into(),
        origin,
        cause: Cause::Direct,
    }
}

fn consent(authorizer: &[u8], account: AccountNumber, expires_at: u64) -> Consent {
    Consent {
        key: authorizer.to_vec(),
        account,
        expires_at,
        proof: b"signed".to_vec(),
    }
}

#[test]
fn accounts_are_created_keys_admitted_by_consent_and_removed_by_seniority() {
    let store = MemoryStore::new();
    let mut identity = Identity::open(store.clone()).unwrap();
    assert_eq!(
        identity
            .create(&signed(b"alice"), " Alice ".into(), Scheme::Ed25519)
            .unwrap(),
        1
    );
    identity.save();
    let ops = store.ops();
    assert_eq!(
        ops.iter().filter(|op| op.starts_with("Get")).count(),
        3,
        "{ops:?}"
    );
    assert_eq!(
        ops.iter().filter(|op| op.starts_with("Set")).count(),
        4,
        "{ops:?}"
    );
    let twice = identity.create(&signed(b"alice"), "Alice again".into(), Scheme::Ed25519);
    assert_eq!(twice.unwrap_err().reason, reason::ALREADY_EXISTS);
    let unsigned = identity.create(&env(Origin::System), "Nobody".into(), Scheme::Ed25519);
    assert_eq!(unsigned.unwrap_err().reason, reason::UNAUTHORIZED);

    let phone = b"phone";
    let later = Env {
        time: TIME + 1,
        ..signed(phone)
    };
    identity
        .add_key(
            &later,
            Scheme::Ed25519,
            Some("phone".into()),
            consent(b"alice", 1, TIME + 1),
        )
        .unwrap();
    let account = identity.get(1).unwrap().unwrap();
    assert_eq!(account.name, "Alice");
    assert_eq!(account.keys().len(), 2);
    assert_eq!(identity.of_key(phone.to_vec()).unwrap(), Some(1));
    let expired = identity.add_key(
        &signed(b"laptop"),
        Scheme::Ed25519,
        None,
        consent(b"alice", 1, TIME - 1),
    );
    assert_eq!(expired.unwrap_err().reason, reason::UNAUTHORIZED);
    store.accept.set(false);
    let forged = identity.add_key(
        &signed(b"laptop"),
        Scheme::Ed25519,
        None,
        consent(b"alice", 1, TIME + 1),
    );
    assert_eq!(forged.unwrap_err().reason, reason::UNAUTHORIZED);
    store.accept.set(true);

    let senior = identity.remove_key(&signed(phone), b"alice".to_vec());
    assert_eq!(senior.unwrap_err().reason, reason::UNAUTHORIZED);
    identity
        .remove_key(&signed(b"alice"), phone.to_vec())
        .unwrap();
    let last = identity.remove_key(&signed(b"alice"), b"alice".to_vec());
    assert_eq!(last.unwrap_err().reason, reason::WRONG_STATE);
    assert_eq!(identity.generation(phone.to_vec()).unwrap(), 1);
    identity
        .add_key(
            &signed(phone),
            Scheme::Ed25519,
            None,
            consent(b"alice", 1, TIME + 1),
        )
        .unwrap();
    assert_eq!(identity.generation(phone.to_vec()).unwrap(), 2);

    // The root record round-trips: a fresh open over the same store continues the numbering.
    identity.save();
    let mut reopened = Identity::open(store.clone()).unwrap();
    assert_eq!(
        reopened
            .create(&signed(b"bob"), "Bob".into(), Scheme::Ed25519)
            .unwrap(),
        2
    );
    assert_eq!(
        reopened
            .resolve(vec![
                Reference::Account(2),
                Reference::Key(phone.to_vec()),
                Reference::Account(9)
            ])
            .unwrap(),
        vec![Some(2), Some(1), None]
    );
}

#[test]
fn program_accounts_are_controlled_listed_by_page_and_revoked() {
    let mut identity = Identity::default();
    for seed in 1..=3u8 {
        identity
            .create(&signed(&[seed]), format!("User {seed}"), Scheme::Ed25519)
            .unwrap();
    }
    let probe = env(Origin::Program("probe".into()));
    for _ in 0..3 {
        identity
            .create_program(&probe, "Controlled".into(), 1)
            .unwrap();
    }
    let not_the_executor = identity.set_standing(&signed(&[1]), 4, Standing::Suspended);
    assert_eq!(not_the_executor.unwrap_err().reason, reason::UNAUTHORIZED);
    identity
        .set_standing(&probe, 4, Standing::Suspended)
        .unwrap();
    let circular = identity.transfer_control(&signed(&[1]), 5, 5);
    assert_eq!(circular.unwrap_err().reason, reason::WRONG_STATE);
    identity.transfer_control(&signed(&[1]), 5, 2).unwrap();

    for (by, expected) in [(1, vec![4, 6]), (2, vec![5]), (3, vec![])] {
        let mut after = None;
        let mut numbers = Vec::new();
        loop {
            let page = Page {
                after,
                limit: Some(1),
            };
            let reply = identity.controlled(&signed(&[1]), by, page).unwrap();
            assert_eq!(reply.height, 7);
            assert!(reply.items.len() <= 1);
            numbers.extend(reply.items.iter().map(|account| account.number));
            after = reply.next;
            if after.is_none() {
                break;
            }
        }
        assert_eq!(numbers, expected, "controlled by {by}");
    }
    let all = identity
        .list(
            &signed(&[1]),
            Page {
                after: None,
                limit: Some(100),
            },
        )
        .unwrap();
    assert_eq!(
        all.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(all.next, None);

    identity.revoke(&signed(&[1]), 4).unwrap();
    assert_eq!(
        identity.get(4).unwrap().unwrap().control,
        Control::Revoked { controller: 1 }
    );
    let revoked_controls_nothing = identity.create_program(&probe, "Orphan".into(), 4);
    assert_eq!(
        revoked_controls_nothing.unwrap_err().reason,
        reason::WRONG_STATE
    );
}
