// The module natively over `guest::MockHost`: what the founding suite checks on the host, without the host.

use guest::{Cause, Env, Origin, Principal, Scheme, code};
use guest::{MockHost, Module};
use store::PageRequest;

use crate::rules::ACCOUNTS;
use crate::{
    Account, Admission, CONSENT_NAMESPACE, Category, Consent, Identity, Op, Query, Reply, Status,
};

const ALICE: &[u8] = b"alice-key";
const SECOND: &[u8] = b"alice-second-key";
const BOT: &[u8] = b"bot-key";

fn env(origin: Origin, sender: Option<Principal>, time: u64) -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height: 7,
        time,
        module: crate::MODULE.into(),
        origin,
        sender,
        roles: guest::MockHost::roles(),
        cause: Cause::Direct,
    }
}

/// A frame signed by `key` at `time`, acting as the account identity says
/// it holds, as the host resolves it: a refusal rejects the frame.
fn signed_at(store: &MockHost, key: &[u8], time: u64) -> Result<Env, guest::Error> {
    let origin = Origin::Signed(key.to_vec());
    let asked = Query::OfKey { key: key.to_vec() };
    let sender = match Identity::query(&store.query(env(origin.clone(), None, time)), asked)? {
        Reply::Number(number) => number.map(Principal::Account),
        other => panic!("{other:?}"),
    };
    Ok(env(origin, sender, time))
}

fn signed(store: &MockHost, key: &[u8]) -> Env {
    signed_at(store, key, 100).unwrap()
}

/// A message from `module`, acting as its account.
fn from_module(store: &MockHost, module: &str) -> Env {
    let asked = Query::OfModule {
        module: module.into(),
    };
    let Reply::Number(number) = query(store, asked) else {
        panic!("OfModule answers a number");
    };
    env(
        Origin::Module(module.into()),
        number.map(Principal::Account),
        100,
    )
}

fn root() -> Env {
    env(Origin::Root, Some(Principal::Root), 100)
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

fn query(store: &MockHost, query: Query) -> Reply {
    Identity::query(&store.query(env(Origin::Root, None, 100)), query).unwrap()
}

fn get(store: &MockHost, number: u64) -> Account {
    match query(store, Query::Get { number }) {
        Reply::Account(account) => account.expect("the account exists"),
        other => panic!("{other:?}"),
    }
}

fn create(store: &MockHost, key: &[u8], name: &str) -> u64 {
    run(
        store,
        &signed(store, key),
        Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        },
    )
    .unwrap()
}

fn create_agent(store: &MockHost, manager: &[u8], name: &str) -> u64 {
    let op = Op::CreateAgent { name: name.into() };
    run(store, &signed(store, manager), op).unwrap()
}

/// The manager signs, `key` consents to joining `agent`.
fn add_agent_key(
    store: &MockHost,
    manager: &[u8],
    agent: u64,
    key: &[u8],
) -> Result<u64, guest::Error> {
    run(store, &signed(store, manager), agent_key(agent, key, 0))
}

/// `key`'s consent to joining `agent`, proved over its `generation`,
/// expiring at 200.
fn agent_key(agent: u64, key: &[u8], generation: u64) -> Op {
    let admission = Admission {
        network: b"net".to_vec(),
        scheme: Scheme::Ed25519,
        key: key.to_vec(),
        generation,
        account: agent,
        expires_at: 200,
    };
    Op::AddKey {
        scheme: Scheme::Ed25519,
        label: Some("sandbox".into()),
        consent: Consent {
            key: key.to_vec(),
            account: agent,
            expires_at: 200,
            proof: admission.preimage(),
        },
    }
}

#[test]
fn accounts_are_numbered_from_one_and_a_key_holds_one_account() {
    let store = memory();
    assert_eq!(create(&store, ALICE, "  Alice "), 1);
    assert_eq!(get(&store, 1).name, "Alice");
    assert!(get(&store, 1).is_person());
    assert_eq!(create(&store, b"bob", "Bob"), 2);
    let again = run(
        &store,
        &signed(&store, ALICE),
        Op::Create {
            name: "Twice".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(again.unwrap_err().code, code::ALREADY_EXISTS);
    assert_eq!(
        query(
            &store,
            Query::OfKey {
                key: ALICE.to_vec()
            }
        ),
        Reply::Number(Some(1))
    );
    let unsigned = run(
        &store,
        &root(),
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
    let second = |time| signed_at(&store, SECOND, time).unwrap();
    let forged = run(&store, &second(100), add(b"nope".to_vec(), 200));
    assert_eq!(forged.unwrap_err().code, code::UNAUTHORIZED);
    let expired = run(&store, &second(300), add(admission.preimage(), 200));
    assert_eq!(expired.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &second(150), add(admission.preimage(), 200)).unwrap();
    assert_eq!(get(&store, 1).keys.len(), 2);
    assert_eq!(
        query(
            &store,
            Query::Generation {
                key: SECOND.to_vec()
            }
        ),
        Reply::Generation(1)
    );
    let remove = |key: &[u8]| Op::RemoveKey {
        account: 1,
        key: key.to_vec(),
    };
    let senior = run(&store, &second(150), remove(ALICE));
    assert_eq!(senior.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove(SECOND)).unwrap();
    assert!(!get(&store, 1).holds(SECOND));
    let last = run(&store, &signed(&store, ALICE), remove(ALICE));
    assert_eq!(last.unwrap_err().code, code::WRONG_STATE);
}

#[test]
fn a_person_manages_an_agent_and_its_keys() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, b"bob", "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    assert_eq!(agent, 3);
    let made = get(&store, agent);
    assert_eq!(
        (made.manager, made.category, made.keys.len()),
        (Some(1), Some(Category::Agent), 0)
    );

    // only the manager adds a key, and the key consents to joining
    let stranger = add_agent_key(&store, b"bob", agent, BOT);
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    // an agent creates no agent, and names itself or is named by its manager
    let nested = run(
        &store,
        &signed(&store, BOT),
        Op::CreateAgent { name: "x".into() },
    );
    assert_eq!(nested.unwrap_err().code, code::UNAUTHORIZED);
    let rename = |name: &str| Op::SetName {
        account: agent,
        name: name.into(),
    };
    let by_stranger = run(&store, &signed(&store, b"bob"), rename("Mine"));
    assert_eq!(by_stranger.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), rename("Scout II")).unwrap();
    run(&store, &signed(&store, BOT), rename("Scout III")).unwrap();
    assert_eq!(get(&store, agent).name, "Scout III");

    // only the manager removes its key
    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    let by_itself = run(&store, &signed(&store, BOT), remove.clone());
    assert_eq!(by_itself.unwrap_err().code, code::UNAUTHORIZED);
    let by_stranger = run(&store, &signed(&store, b"bob"), remove.clone());
    assert_eq!(by_stranger.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove).unwrap();
    assert_eq!(signed(&store, BOT).sender, None);
}

#[test]
fn a_suspended_or_revoked_agent_acts_as_no_one() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, b"bob", "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let status = |status| Op::SetStatus {
        account: agent,
        status,
    };
    let by_itself = run(&store, &signed(&store, BOT), status(Status::Suspended));
    assert_eq!(by_itself.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), status(Status::Suspended)).unwrap();
    let refused = signed_at(&store, BOT, 100).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), status(Status::Active)).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    // a manager that is not live stops its agents with it
    let mut alice = get(&store, 1);
    alice.status = Status::Suspended;
    ACCOUNTS.put(&store.exec(root()), &1, &alice);
    assert!(signed_at(&store, BOT, 100).is_err());
    alice.status = Status::Active;
    ACCOUNTS.put(&store.exec(root()), &1, &alice);

    // the manager hands it to a person; the old one no longer manages it
    let to = |to| Op::TransferManager { account: agent, to };
    let to_agent = run(&store, &signed(&store, ALICE), to(agent));
    assert_eq!(to_agent.unwrap_err().code, code::WRONG_STATE);
    run(&store, &signed(&store, ALICE), to(2)).unwrap();
    assert_eq!(get(&store, agent).manager, Some(2));
    let former = run(&store, &signed(&store, ALICE), status(Status::Revoked));
    assert_eq!(former.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &signed(&store, b"bob"), status(Status::Revoked)).unwrap();
    assert!(signed_at(&store, BOT, 100).is_err());
    let revived = run(&store, &signed(&store, b"bob"), status(Status::Active));
    assert_eq!(revived.unwrap_err().code, code::WRONG_STATE);

    // a person has no manager, so no one sets her status
    let alice_status = Op::SetStatus {
        account: 1,
        status: Status::Suspended,
    };
    let own = run(&store, &signed(&store, ALICE), alice_status);
    assert_eq!(own.unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn the_system_registers_a_module_which_alone_names_its_account() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let register = || Op::RegisterModule {
        module: "forge".into(),
    };
    let by_person = run(&store, &signed(&store, ALICE), register());
    assert_eq!(by_person.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &root(), register()).unwrap();
    run(&store, &root(), register()).unwrap();
    let forge = from_module(&store, "forge");
    let by_module = run(
        &store,
        &forge,
        Op::RegisterModule {
            module: "other".into(),
        },
    );
    assert_eq!(by_module.unwrap_err().code, code::UNAUTHORIZED);
    assert_eq!(forge.sender, Some(Principal::Account(2)));
    let account = get(&store, 2);
    assert_eq!(
        (account.name.as_str(), account.module.as_deref()),
        ("forge", Some("forge"))
    );
    assert!(account.keys.is_empty() && account.manager.is_none());

    let rename = || Op::SetName {
        account: 2,
        name: "Forge".into(),
    };
    let by_person = run(&store, &signed(&store, ALICE), rename());
    assert_eq!(by_person.unwrap_err().code, code::UNAUTHORIZED);
    run(&store, &forge, rename()).unwrap();
    assert_eq!(get(&store, 2).name, "Forge");
    // a module's account holds no keys and manages no one
    let agent = run(&store, &forge, Op::CreateAgent { name: "x".into() });
    assert_eq!(agent.unwrap_err().code, code::UNAUTHORIZED);
    let key = add_agent_key(&store, ALICE, 2, BOT);
    assert_eq!(key.unwrap_err().code, code::WRONG_STATE);
}

#[test]
fn lists_page_in_number_order_and_managed_lists_one_manager() {
    let store = memory();
    for n in 0..11u8 {
        create(&store, &[n], &format!("a{n}"));
    }
    for _ in 0..3 {
        create_agent(&store, &[1], "agent");
    }
    create_agent(&store, &[10], "other");
    let list = |asked| match query(&store, asked) {
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
    let managed = list(Query::Managed {
        by: 2,
        page: PageRequest::first(2),
    });
    assert_eq!(
        managed.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [12, 13]
    );
    let more = list(Query::Managed {
        by: 2,
        page: PageRequest {
            after: managed.next,
            limit: Some(2),
        },
    });
    assert_eq!(
        (
            more.items.iter().map(|a| a.number).collect::<Vec<_>>(),
            more.next
        ),
        (vec![14], None),
        "the page stays under one manager"
    );
    assert_eq!(
        query(
            &store,
            Query::Resolve {
                references: vec![
                    crate::Reference::Account(15),
                    crate::Reference::Account(99),
                    crate::Reference::Key(vec![3]),
                ],
            }
        ),
        Reply::Resolved(vec![Some(15), None, Some(4)])
    );
}

/// Identity answers its role: the role's op, queries and replies are its
/// first ones, byte for byte.
#[test]
fn the_identity_role_is_its_first_variants() {
    use abi::role::identity as role;
    let key = ALICE.to_vec();
    assert_eq!(
        abi::encode(&role::Op::RegisterModule {
            module: "chat".into()
        }),
        abi::encode(&Op::RegisterModule {
            module: "chat".into()
        })
    );
    assert_eq!(
        abi::encode(&role::Query::Account(key.clone())),
        abi::encode(&Query::OfKey { key })
    );
    assert_eq!(
        abi::encode(&role::Query::Profiles {
            after: Some(2),
            limit: 5
        }),
        abi::encode(&Query::Profiles {
            after: Some(2),
            limit: 5
        })
    );
    assert_eq!(
        abi::encode(&role::Query::OfModule("chat".into())),
        abi::encode(&Query::OfModule {
            module: "chat".into()
        })
    );
    assert_eq!(
        abi::encode(&role::Reply::Account(Some(3))),
        abi::encode(&Reply::Number(Some(3)))
    );
    let profiles = vec![crate::Profile {
        number: 1,
        name: "Alice".into(),
        category: None,
        manager: None,
        module: None,
        status: Status::Active,
    }];
    assert_eq!(
        abi::encode(&role::Reply::Profiles {
            profiles: profiles.clone(),
            next: Some(1)
        }),
        abi::encode(&Reply::Profiles {
            profiles,
            next: Some(1)
        })
    );
}

/// Profiles page in number order and say what each account is.
#[test]
fn profiles_page_in_number_order_and_say_what_each_is() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create_agent(&store, ALICE, "Scout");
    run(
        &store,
        &root(),
        Op::RegisterModule {
            module: "chat".into(),
        },
    )
    .unwrap();
    let page = |after, limit| match query(&store, Query::Profiles { after, limit }) {
        Reply::Profiles { profiles, next } => (profiles, next),
        other => panic!("{other:?}"),
    };
    let (first, next) = page(None, 2);
    let what: Vec<_> = first
        .iter()
        .map(|p| (p.number, p.name.as_str(), p.category, p.manager))
        .collect();
    assert_eq!(
        what,
        [
            (1, "Alice", None, None),
            (2, "Scout", Some(Category::Agent), Some(1))
        ]
    );
    assert_eq!(next, Some(2));
    let (rest, next) = page(next, 2);
    assert_eq!(
        rest.iter()
            .map(|p| (p.number, p.module.as_deref()))
            .collect::<Vec<_>>(),
        [(3, Some("chat"))]
    );
    assert_eq!(next, None);
}

/// An account's profile is set by the account itself or its manager; a
/// stranger sets none, nor anyone a module's.
#[test]
fn a_profile_is_set_by_its_account_or_its_manager() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, b"bob", "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    run(
        &store,
        &root(),
        Op::RegisterModule {
            module: "forge".into(),
        },
    )
    .unwrap();
    let forge = from_module(&store, "forge");
    let Some(Principal::Account(forge_account)) = forge.sender else {
        panic!("forge acts as its account");
    };
    let bio = |account: u64, bio: &str| Op::SetProfile {
        account,
        avatar: None,
        bio: Some(bio.into()),
    };
    run(&store, &signed(&store, ALICE), bio(1, " mine ")).unwrap();
    assert_eq!(get(&store, 1).bio.as_deref(), Some("mine"));
    run(&store, &signed(&store, BOT), bio(agent, "itself")).unwrap();
    run(&store, &signed(&store, ALICE), bio(agent, "managed")).unwrap();
    assert_eq!(get(&store, agent).bio.as_deref(), Some("managed"));
    for (by, account) in [
        (signed(&store, b"bob"), 1),
        (signed(&store, b"bob"), agent),
        (signed(&store, BOT), 1),
        (forge.clone(), 1),
        (signed(&store, ALICE), forge_account),
        (signed(&store, BOT), forge_account),
    ] {
        let refused = run(&store, &by, bio(account, "not yours"));
        assert_eq!(refused.unwrap_err().code, code::UNAUTHORIZED, "{account}");
    }
    run(&store, &forge, bio(forge_account, "the forge")).unwrap();
    assert_eq!(get(&store, forge_account).bio.as_deref(), Some("the forge"));
}

/// An agent's new key consents over its generation and before it expires:
/// a consent past its time, or replayed once the key has left, is refused.
#[test]
fn an_agents_key_consent_expires_and_is_not_replayed() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    let late = signed_at(&store, ALICE, 300).unwrap();
    let expired = run(&store, &late, agent_key(agent, BOT, 0));
    assert_eq!(expired.unwrap_err().code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    run(&store, &signed(&store, ALICE), remove).unwrap();
    let replayed = add_agent_key(&store, ALICE, agent, BOT);
    assert_eq!(replayed.unwrap_err().code, code::UNAUTHORIZED);
    let fresh = agent_key(agent, BOT, 1);
    run(&store, &signed(&store, ALICE), fresh).unwrap();
    assert!(get(&store, agent).holds(BOT));
}
