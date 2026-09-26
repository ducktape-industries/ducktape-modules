use super::*;

fn consent(
    authorizer: &ed25519::PrivateKey,
    account: AccountNumber,
    new_key: &[u8],
    generation: u64,
    expires_at: u64,
) -> identity::Consent {
    let admission = identity::Admission {
        network: NETWORK.to_vec(),
        scheme: Scheme::Ed25519,
        key: new_key.to_vec(),
        generation,
        account,
        expires_at,
    };
    identity::Consent {
        key: authorizer.public_key().as_ref().to_vec(),
        account,
        expires_at,
        proof: testkit::ed25519_proof(
            authorizer,
            identity::CONSENT_NAMESPACE,
            &admission.preimage(),
        ),
    }
}

/// The founding programs, in admission order: identity numbers their
/// accounts first, so a person's account comes after them.
const FOUNDED: [&str; 5] = [
    module_registry::MODULE,
    valset::MODULE,
    identity::MODULE,
    AUTHORITY,
    "probe",
];

/// The first person's account: the founding programs hold 1..=5.
const ALICE: AccountNumber = 6;

impl Net {
    async fn account(&self, number: AccountNumber) -> identity::Account {
        let identity::Reply::Account(Some(account)) = self
            .ask(identity::MODULE, &identity::Query::Get { number })
            .await
        else {
            panic!("account {number} exists")
        };
        account
    }

    async fn of_module(&self, module: &str) -> Option<AccountNumber> {
        let asked = identity::Query::OfModule {
            module: module.into(),
        };
        let identity::Reply::Number(number) = self.ask(identity::MODULE, &asked).await else {
            panic!()
        };
        number
    }

    async fn create(&mut self, seed: u64, name: &str) -> AccountNumber {
        let op = identity::Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        };
        let output = self.apply(&public(seed), identity::MODULE, &op).await;
        abi::decode(&output).unwrap()
    }
}

#[test]
fn identity_founds_accounts_and_admits_keys_by_consent() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        assert_eq!(net.create(1, " Alice ").await, ALICE);
        let twice = net
            .refuse(
                &public(1),
                identity::MODULE,
                &identity::Op::Create {
                    name: "Alice again".into(),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        assert_eq!(twice, reason::ALREADY_EXISTS);
        let phone = public(11);
        let expires_at = TIME + 600_000;
        net.apply(
            &phone,
            identity::MODULE,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone".into()),
                consent: consent(&key(1), ALICE, &phone, 0, expires_at),
            },
        )
        .await;
        let account = net.account(ALICE).await;
        assert_eq!(account.name, "Alice");
        assert_eq!(account.keys.len(), 2);
        let identity::Reply::Number(of_phone) = net
            .ask(
                identity::MODULE,
                &identity::Query::OfKey { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(of_phone, Some(ALICE));
        let replayed = net
            .refuse(
                &public(12),
                identity::MODULE,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), ALICE, &phone, 0, expires_at),
                },
            )
            .await;
        assert_eq!(replayed, reason::UNAUTHORIZED);
        let expired = net
            .refuse(
                &public(12),
                identity::MODULE,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), ALICE, &public(12), 0, TIME - 1),
                },
            )
            .await;
        assert_eq!(expired, reason::UNAUTHORIZED);
        let remove = |key: Vec<u8>| identity::Op::RemoveKey {
            account: ALICE,
            key,
        };
        let senior = net
            .refuse(&phone, identity::MODULE, &remove(public(1)))
            .await;
        assert_eq!(senior, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::MODULE, &remove(phone.clone()))
            .await;
        let last = net
            .refuse(&public(1), identity::MODULE, &remove(public(1)))
            .await;
        assert_eq!(last, reason::WRONG_STATE);
        let identity::Reply::Generation(generation) = net
            .ask(
                identity::MODULE,
                &identity::Query::Generation { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(generation, 1);
        net.apply(
            &phone,
            identity::MODULE,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone again".into()),
                consent: consent(&key(1), ALICE, &phone, 1, expires_at),
            },
        )
        .await;
        net.apply(
            &public(1),
            identity::MODULE,
            &identity::Op::SetName {
                account: ALICE,
                name: "Alice B".into(),
            },
        )
        .await;
        let identity::Reply::Resolved(resolved) = net
            .ask(
                identity::MODULE,
                &identity::Query::Resolve {
                    references: vec![
                        identity::Reference::Account(ALICE),
                        identity::Reference::Key(phone.clone()),
                        identity::Reference::Account(99),
                    ],
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(resolved, vec![Some(ALICE), Some(ALICE), None]);
    });
}

#[test]
fn every_module_has_its_account_from_its_admission() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        for (at, module) in FOUNDED.into_iter().enumerate() {
            let number = at as AccountNumber + 1;
            assert_eq!(net.of_module(module).await, Some(number), "{module}");
            let account = net.account(number).await;
            assert_eq!(account.module.as_deref(), Some(module));
            assert!(account.keys.is_empty() && account.manager.is_none());
        }

        // a module installed later has its account once it runs
        let output = net
            .apply(
                &public(7),
                module_registry::MODULE,
                &module_registry::Op::Publish {
                    body: PROBE.to_vec(),
                },
            )
            .await;
        let code: BlobId = abi::decode(&output).unwrap();
        let lands_at = net.height + 3;
        let entry = module_registry::Entry {
            program: "late".into(),
            code,
            params: abi::encode(&Vec::<Step>::new()),
        };
        let scheduled = net
            .as_authority(
                module_registry::MODULE,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: lands_at,
                    change: module_registry::Change::Set(entry),
                }),
            )
            .await;
        output_of(&scheduled);
        assert_eq!(net.of_module("late").await, None);
        net.ticks(lands_at - net.height).await;
        let late = net.of_module("late").await.expect("late has an account");
        assert_eq!(net.account(late).await.name, "late");

        // the module alone names its account; its frames act as it
        let alice = net.create(1, "Alice").await;
        let probe = net.of_module("probe").await.unwrap();
        let rename = identity::Op::SetName {
            account: probe,
            name: "Probe".into(),
        };
        let by_person = net.refuse(&public(1), identity::MODULE, &rename).await;
        assert_eq!(by_person, reason::UNAUTHORIZED);
        output_of(&net.sent_by("probe", identity::MODULE, &rename).await);
        assert_eq!(net.account(probe).await.name, "Probe");
        let theirs = identity::Op::SetName {
            account: alice,
            name: "Probed".into(),
        };
        let other = net.sent_by("probe", identity::MODULE, &theirs).await;
        assert_eq!(refusal_of(&other), reason::UNAUTHORIZED);
    });
}

#[test]
fn an_agent_acts_until_its_manager_suspends_it() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let alice = net.create(1, "Alice").await;
        let bob = net.create(2, "Bob").await;
        let output = net
            .apply(
                &public(1),
                identity::MODULE,
                &identity::Op::CreateAgent {
                    name: "Scout".into(),
                },
            )
            .await;
        let agent: AccountNumber = abi::decode(&output).unwrap();
        let made = net.account(agent).await;
        assert_eq!(
            (made.manager, made.category),
            (Some(alice), Some(identity::Category::Agent))
        );

        // the manager signs; the agent's new key consents to joining
        let bot = key(21);
        let bot_key = public(21);
        let add = identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: Some("sandbox".into()),
            consent: consent(&bot, agent, &bot_key, 0, TIME + 600_000),
        };
        let stranger = net.refuse(&public(2), identity::MODULE, &add).await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::MODULE, &add).await;
        let nested = net
            .refuse(
                &bot_key,
                identity::MODULE,
                &identity::Op::CreateAgent { name: "x".into() },
            )
            .await;
        assert_eq!(nested, reason::UNAUTHORIZED);

        // the agent acts: it names itself
        let rename = |name: &str| identity::Op::SetName {
            account: agent,
            name: name.into(),
        };
        net.apply(&bot_key, identity::MODULE, &rename("Scout 2"))
            .await;

        // suspended, its every frame is refused before it runs
        let status = |status| identity::Op::SetStatus {
            account: agent,
            status,
        };
        let by_bob = net
            .refuse(
                &public(2),
                identity::MODULE,
                &status(identity::Status::Suspended),
            )
            .await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        net.apply(
            &public(1),
            identity::MODULE,
            &status(identity::Status::Suspended),
        )
        .await;
        let suspended = net
            .refuse(&bot_key, identity::MODULE, &rename("Scout 3"))
            .await;
        assert_eq!(suspended, reason::UNAUTHORIZED);
        net.apply(
            &public(1),
            identity::MODULE,
            &status(identity::Status::Active),
        )
        .await;
        net.apply(&bot_key, identity::MODULE, &rename("Scout 3"))
            .await;

        // handed to Bob, then revoked for good
        net.apply(
            &public(1),
            identity::MODULE,
            &identity::Op::TransferManager {
                account: agent,
                to: bob,
            },
        )
        .await;
        net.apply(
            &public(2),
            identity::MODULE,
            &status(identity::Status::Revoked),
        )
        .await;
        let revoked = net
            .refuse(&bot_key, identity::MODULE, &rename("Scout 4"))
            .await;
        assert_eq!(revoked, reason::UNAUTHORIZED);
        let revived = net
            .refuse(
                &public(2),
                identity::MODULE,
                &status(identity::Status::Active),
            )
            .await;
        assert_eq!(revived, reason::WRONG_STATE);
        assert_eq!(net.account(agent).await.name, "Scout 3");
    });
}

#[test]
fn account_lists_resume_with_the_answering_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        for seed in 1..=3 {
            net.apply(
                &public(seed),
                identity::MODULE,
                &identity::Op::Create {
                    name: format!("User {seed}"),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        }
        for _ in 0..3 {
            let op = identity::Op::CreateAgent {
                name: "Managed".into(),
            };
            net.apply(&public(1), identity::MODULE, &op).await;
        }
        for managed in [false, true] {
            let mut after = None;
            let mut numbers = Vec::new();
            loop {
                let page = PageRequest {
                    after,
                    limit: Some(2),
                };
                let query = if managed {
                    identity::Query::Managed { by: ALICE, page }
                } else {
                    identity::Query::List { page }
                };
                let identity::Reply::Accounts(reply) = net.ask(identity::MODULE, &query).await
                else {
                    panic!()
                };
                assert_eq!(reply.height, net.height);
                assert!(reply.items.len() <= 2);
                numbers.extend(reply.items.iter().map(|account| account.number));
                after = reply.next;
                if after.is_none() {
                    break;
                }
            }
            // the founding programs' accounts, three people, three agents
            assert_eq!(
                numbers,
                if managed {
                    vec![9, 10, 11]
                } else {
                    (1..=11).collect::<Vec<_>>()
                }
            );
        }
        // A cursor is opaque and bound to its listing: bytes that are not
        // one are refused, and a Managed cursor does not open a List.
        let garbage = net
            .refused(
                identity::MODULE,
                &identity::Query::List {
                    page: PageRequest {
                        after: Some(vec![255]),
                        limit: Some(0),
                    },
                },
            )
            .await;
        assert_eq!(garbage.reason, abi::reason::INVALID_INPUT);
        let identity::Reply::Accounts(managed) = net
            .ask(
                identity::MODULE,
                &identity::Query::Managed {
                    by: ALICE,
                    page: PageRequest::first(1),
                },
            )
            .await
        else {
            panic!()
        };
        let other_listing = net
            .refused(
                identity::MODULE,
                &identity::Query::List {
                    page: PageRequest {
                        after: managed.next,
                        limit: Some(1),
                    },
                },
            )
            .await;
        assert_eq!(other_listing.reason, abi::reason::STALE);
    });
}
