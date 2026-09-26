// The module natively over `guest::MockHost`: what the founding suite checks on the host, without the host.

use guest::{Cause, Env, Origin, Principal, Scheme, code};
use guest::{MockHost, Module};
use store::PageRequest;

use crate::{
    Account, Admission, CONSENT_NAMESPACE, Category, Consent, Control, Identity, Kind, Life, Op,
    Query, Reply, Standing,
};

const ALICE: &[u8] = b"alice-key";
const SECOND: &[u8] = b"alice-second-key";
const BOB: &[u8] = b"bob";
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

/// A proof, natively: the signature is the namespace then the preimage
/// ([`proof`]), so a signature made under one namespace verifies under no
/// other; the verifier checks it names a key.
fn memory() -> MockHost {
    let host = MockHost::default();
    host.borrow_mut().verifier = Some(Box::new(|_, key, namespace, message, signature| {
        signature == proof(namespace, message) && !key.is_empty()
    }));
    host
}

/// What [`memory`]'s verifier takes as a signature under `namespace`.
fn proof(namespace: &[u8], preimage: &[u8]) -> Vec<u8> {
    [namespace, preimage].concat()
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

/// `op` as `env`, which must refuse and leave the store as it was.
#[track_caller]
fn refused(store: &MockHost, env: &Env, op: Op) -> guest::Error {
    store.refused(|| run(store, env, op))
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
            proof: proof(CONSENT_NAMESPACE, &admission.preimage()),
        },
    }
}

/// An agent's keys, whatever its life.
fn keys_of(account: &Account) -> Vec<Vec<u8>> {
    account.keys().iter().map(|key| key.key.clone()).collect()
}

#[test]
fn accounts_are_numbered_from_one_and_a_key_holds_one_account() {
    let store = memory();
    assert_eq!(create(&store, ALICE, "  Alice "), 1);
    let alice = get(&store, 1);
    assert_eq!(alice.card.name, "Alice");
    assert!(matches!(alice.control, Control::Person { .. }));
    assert_eq!(create(&store, BOB, "Bob"), 2);
    let again = refused(
        &store,
        &signed(&store, ALICE),
        Op::Create {
            name: "Twice".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(again.code, code::ALREADY_EXISTS);
    assert_eq!(
        query(
            &store,
            Query::OfKey {
                key: ALICE.to_vec()
            }
        ),
        Reply::Number(Some(1))
    );
    let unsigned = refused(
        &store,
        &root(),
        Op::Create {
            name: "x".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(unsigned.code, code::UNAUTHORIZED);
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
    let consented = proof(CONSENT_NAMESPACE, &admission.preimage());
    let second = |time| signed_at(&store, SECOND, time).unwrap();
    let forged = refused(&store, &second(100), add(b"nope".to_vec(), 200));
    assert_eq!(forged.code, code::UNAUTHORIZED);
    let expired = refused(&store, &second(300), add(consented.clone(), 200));
    assert_eq!(expired.code, code::UNAUTHORIZED);
    run(&store, &second(150), add(consented, 200)).unwrap();
    assert_eq!(get(&store, 1).keys().len(), 2);
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
    let senior = refused(&store, &second(150), remove(ALICE));
    assert_eq!(senior.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove(SECOND)).unwrap();
    assert!(!get(&store, 1).holds(SECOND));
    let last = refused(&store, &signed(&store, ALICE), remove(ALICE));
    assert_eq!(last.code, code::WRONG_STATE);
}

/// A person creates an agent and alone adds and removes its keys; the
/// agent itself neither creates agents nor touches its own keys.
#[test]
fn a_person_manages_an_agent_and_its_keys() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    assert_eq!(agent, 3);
    assert_eq!(
        get(&store, agent).control,
        Control::Managed {
            manager: 1,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
        }
    );

    // only the manager adds a key, and the key consents to joining
    let stranger = refused(&store, &signed(&store, BOB), agent_key(agent, BOT, 0));
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    // an agent creates no agent
    let nested = refused(
        &store,
        &signed(&store, BOT),
        Op::CreateAgent { name: "x".into() },
    );
    assert_eq!(nested.code, code::UNAUTHORIZED);

    // only the manager removes its key
    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    let by_itself = refused(&store, &signed(&store, BOT), remove.clone());
    assert_eq!(by_itself.code, code::UNAUTHORIZED);
    let by_stranger = refused(&store, &signed(&store, BOB), remove.clone());
    assert_eq!(by_stranger.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove).unwrap();
    assert_eq!(signed(&store, BOT).sender, None);
}

/// A card (name, avatar, bio) is a person's own, an agent's manager's alone
/// (never the agent's own), and a module's own alone.
#[test]
fn a_card_is_edited_by_its_person_its_manager_or_its_module() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
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
    let rename = |account: u64, name: &str| Op::SetName {
        account,
        name: name.into(),
    };
    run(&store, &signed(&store, ALICE), bio(1, " mine ")).unwrap();
    assert_eq!(get(&store, 1).card.bio.as_deref(), Some("mine"));
    run(&store, &signed(&store, ALICE), bio(agent, "managed")).unwrap();
    run(&store, &signed(&store, ALICE), rename(agent, "Scout II")).unwrap();
    assert_eq!(get(&store, agent).card.bio.as_deref(), Some("managed"));
    assert_eq!(get(&store, agent).card.name, "Scout II");
    run(&store, &forge, rename(forge_account, "Forge")).unwrap();
    run(&store, &forge, bio(forge_account, "the forge")).unwrap();
    assert_eq!(get(&store, forge_account).card.name, "Forge");
    for (by, account) in [
        (signed(&store, BOB), 1),
        (signed(&store, BOB), agent),
        (signed(&store, BOT), 1),
        // the agent does not edit its own card: its manager answers for it
        (signed(&store, BOT), agent),
        (forge.clone(), 1),
        (forge.clone(), agent),
        (signed(&store, ALICE), forge_account),
        (signed(&store, BOT), forge_account),
    ] {
        let why = format!("{:?} on {account}", by.origin);
        assert_eq!(
            refused(&store, &by, bio(account, "not yours")).code,
            code::UNAUTHORIZED,
            "{why}"
        );
        assert_eq!(
            refused(&store, &by, rename(account, "not yours")).code,
            code::UNAUTHORIZED,
            "{why}"
        );
    }
}

/// Suspended, an agent acts as no one and keeps its keys for its resume;
/// revoked, it is done for good: its keys are gone, its every op refused,
/// and its manager's list still names it.
#[test]
fn an_agent_is_suspended_resumed_or_revoked_by_its_manager_alone() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let suspend = || Op::Suspend { account: agent };
    let resume = || Op::Resume { account: agent };
    let revoke = || Op::Revoke { account: agent };
    for op in [suspend(), resume(), revoke()] {
        let why = format!("{op:?}");
        assert_eq!(
            refused(&store, &signed(&store, BOT), op.clone()).code,
            code::UNAUTHORIZED,
            "by itself: {why}"
        );
        assert_eq!(
            refused(&store, &signed(&store, BOB), op).code,
            code::UNAUTHORIZED,
            "by a stranger: {why}"
        );
    }
    let already = refused(&store, &signed(&store, ALICE), resume());
    assert_eq!(already.code, code::WRONG_STATE);

    run(&store, &signed(&store, ALICE), suspend()).unwrap();
    let refused_key = signed_at(&store, BOT, 100).unwrap_err();
    assert_eq!(refused_key.code, code::UNAUTHORIZED);
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec()]);
    assert_eq!(get(&store, agent).kind().note(), Some("suspended"));
    let again = refused(&store, &signed(&store, ALICE), suspend());
    assert_eq!(again.code, code::WRONG_STATE);
    // a suspended agent's manager still works its keys and its card
    run(
        &store,
        &signed(&store, ALICE),
        agent_key(agent, b"second-bot", 0),
    )
    .unwrap();
    run(
        &store,
        &signed(&store, ALICE),
        Op::SetName {
            account: agent,
            name: "Idle".into(),
        },
    )
    .unwrap();
    run(&store, &signed(&store, ALICE), resume()).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    run(&store, &signed(&store, ALICE), revoke()).unwrap();
    let account = get(&store, agent);
    assert_eq!(account.kind().note(), Some("revoked"));
    assert!(account.keys().is_empty(), "revoked, it keeps no key");
    assert_eq!(
        query(&store, Query::OfKey { key: BOT.to_vec() }),
        Reply::Number(None),
        "its key holds nothing"
    );
    let ops = [
        suspend(),
        resume(),
        revoke(),
        agent_key(agent, b"third-bot", 0),
        Op::RemoveKey {
            account: agent,
            key: BOT.to_vec(),
        },
        Op::SetName {
            account: agent,
            name: "Back".into(),
        },
        Op::SetProfile {
            account: agent,
            avatar: None,
            bio: Some("back".into()),
        },
    ];
    for op in ops {
        let why = format!("{op:?}");
        let refused = refused(&store, &signed(&store, ALICE), op);
        assert_eq!(refused.code, code::WRONG_STATE, "{why}");
    }
    // its key is free for another account, and the manager's list keeps it
    create(&store, BOT, "Reborn");
    let Reply::Accounts(managed) = query(
        &store,
        Query::Managed {
            by: 1,
            page: PageRequest::first(10),
        },
    ) else {
        panic!("Managed answers accounts");
    };
    assert_eq!(
        managed.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [agent]
    );

    // a person is no one's agent: no one suspends them
    let alice = refused(&store, &signed(&store, ALICE), Op::Suspend { account: 1 });
    assert_eq!(alice.code, code::WRONG_STATE);
    let bob = refused(&store, &signed(&store, BOB), Op::Suspend { account: 1 });
    assert_eq!(bob.code, code::WRONG_STATE);
}

#[test]
fn the_system_registers_a_module_which_alone_names_its_account() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let register = || Op::RegisterModule {
        module: "forge".into(),
    };
    let by_person = refused(&store, &signed(&store, ALICE), register());
    assert_eq!(by_person.code, code::UNAUTHORIZED);
    run(&store, &root(), register()).unwrap();
    run(&store, &root(), register()).unwrap();
    let forge = from_module(&store, "forge");
    let by_module = refused(
        &store,
        &forge,
        Op::RegisterModule {
            module: "other".into(),
        },
    );
    assert_eq!(by_module.code, code::UNAUTHORIZED);
    assert_eq!(forge.sender, Some(Principal::Account(2)));
    let account = get(&store, 2);
    assert_eq!(account.card.name, "forge");
    assert_eq!(
        account.control,
        Control::Module {
            module: "forge".into()
        }
    );
    assert!(account.keys().is_empty());

    // a module's account holds no keys and manages no one
    let agent = refused(&store, &forge, Op::CreateAgent { name: "x".into() });
    assert_eq!(agent.code, code::UNAUTHORIZED);
    let key = refused(&store, &signed(&store, ALICE), agent_key(2, BOT, 0));
    assert_eq!(key.code, code::WRONG_STATE);
    let key = refused(&store, &forge, agent_key(2, BOT, 0));
    assert_eq!(key.code, code::WRONG_STATE);
    let remove = refused(
        &store,
        &forge,
        Op::RemoveKey {
            account: 2,
            key: BOT.to_vec(),
        },
    );
    assert_eq!(remove.code, code::WRONG_STATE);
    let suspend = refused(&store, &signed(&store, ALICE), Op::Suspend { account: 2 });
    assert_eq!(suspend.code, code::WRONG_STATE);
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
        abi::encode(&role::Query::OfModule("chat".into())),
        abi::encode(&Query::OfModule {
            module: "chat".into()
        })
    );
    assert_eq!(
        abi::encode(&role::Query::Profile(4)),
        abi::encode(&Query::Profile { number: 4 })
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
        abi::encode(&role::Reply::Account(Some(3))),
        abi::encode(&Reply::Number(Some(3)))
    );
    let profile = crate::Profile {
        number: 1,
        name: "Alice".into(),
        kind: Kind::Managed {
            manager: 2,
            category: Category::Agent,
            standing: Standing::Suspended,
        },
    };
    assert_eq!(
        abi::encode(&role::Reply::Profile(Some(profile.clone()))),
        abi::encode(&Reply::Profile(Some(profile.clone())))
    );
    assert_eq!(
        abi::encode(&role::Reply::Profiles {
            profiles: vec![profile.clone()],
            next: Some(1)
        }),
        abi::encode(&Reply::Profiles {
            profiles: vec![profile],
            next: Some(1)
        })
    );
}

/// Profiles page in number order and say what each account is; one is
/// asked by number.
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
        .map(|p| (p.number, p.name.as_str(), p.kind.clone()))
        .collect();
    assert_eq!(
        what,
        [
            (1, "Alice", Kind::Person),
            (
                2,
                "Scout",
                Kind::Managed {
                    manager: 1,
                    category: Category::Agent,
                    standing: Standing::Active
                }
            )
        ]
    );
    assert_eq!(next, Some(2));
    let (rest, next) = page(next, 2);
    assert_eq!(
        rest.iter()
            .map(|p| (p.number, p.kind.clone()))
            .collect::<Vec<_>>(),
        [(3, Kind::Module("chat".into()))]
    );
    assert_eq!(next, None);
    let one = |number| match query(&store, Query::Profile { number }) {
        Reply::Profile(profile) => profile,
        other => panic!("{other:?}"),
    };
    assert_eq!(one(2).map(|p| p.name), Some("Scout".into()));
    assert_eq!(one(9), None);
}

/// An agent's new key consents over its generation and before it expires:
/// a consent past its time, or replayed once the key has left, is refused.
#[test]
fn an_agents_key_consent_expires_and_is_not_replayed() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    let late = signed_at(&store, ALICE, 300).unwrap();
    let expired = refused(&store, &late, agent_key(agent, BOT, 0));
    assert_eq!(expired.code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    run(&store, &signed(&store, ALICE), remove).unwrap();
    let replayed = refused(&store, &signed(&store, ALICE), agent_key(agent, BOT, 0));
    assert_eq!(replayed.code, code::UNAUTHORIZED);
    let fresh = agent_key(agent, BOT, 1);
    run(&store, &signed(&store, ALICE), fresh).unwrap();
    assert!(get(&store, agent).holds(BOT));
}
