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

/// `to`'s acceptance, by `key` (one of theirs), of `agent` at its
/// `transfers`th handover, signed under `namespace` (the handover's, unless
/// a test signs under the wrong one).
fn acceptance(
    key: &ed25519::PrivateKey,
    agent: AccountNumber,
    to: AccountNumber,
    transfers: u64,
    expires_at: u64,
    namespace: &[u8],
) -> identity::Acceptance {
    let handover = identity::Handover {
        network: NETWORK.to_vec(),
        account: agent,
        to,
        transfers,
        expires_at,
    };
    identity::Acceptance {
        key: key.public_key().as_ref().to_vec(),
        expires_at,
        proof: testkit::ed25519_proof(key, namespace, &handover.preimage()),
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
        assert_eq!(account.card.name, "Alice");
        assert_eq!(account.keys().len(), 2);
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
            assert_eq!(
                account.control,
                identity::Control::Module {
                    module: module.into()
                }
            );
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
        assert_eq!(net.account(late).await.card.name, "late");

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
        assert_eq!(net.account(probe).await.card.name, "Probe");
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
        assert_eq!(
            net.account(agent).await.control,
            identity::Control::Managed {
                manager: alice,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
                transfers: 0,
            }
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

        // the agent acts, as a member of chat would see; its card is its
        // manager's alone
        let rename = |name: &str| identity::Op::SetName {
            account: agent,
            name: name.into(),
        };
        let own_name = net.refuse(&bot_key, identity::MODULE, &rename("Me")).await;
        assert_eq!(own_name, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::MODULE, &rename("Scout 2"))
            .await;
        let identity::Reply::Profile(Some(profile)) = net
            .ask(
                identity::MODULE,
                &identity::Query::Profile { number: agent },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(profile.name, "Scout 2");

        // suspended, its every frame is refused before it runs
        let suspend = identity::Op::Suspend { account: agent };
        let by_bob = net.refuse(&public(2), identity::MODULE, &suspend).await;
        assert_eq!(by_bob, reason::UNAUTHORIZED);
        net.apply(&public(1), identity::MODULE, &suspend).await;
        let suspended = net
            .refuse(
                &bot_key,
                identity::MODULE,
                &identity::Op::SetProfile {
                    account: agent,
                    avatar: None,
                    bio: None,
                },
            )
            .await;
        assert_eq!(suspended, reason::UNAUTHORIZED);
        assert_eq!(net.account(agent).await.keys().len(), 1, "it keeps its key");
        net.apply(
            &public(1),
            identity::MODULE,
            &identity::Op::Resume { account: agent },
        )
        .await;
        let identity::Reply::Number(holds) = net
            .ask(
                identity::MODULE,
                &identity::Query::OfKey {
                    key: bot_key.clone(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(holds, Some(agent), "resumed, its key acts again");

        // handed to Bob, who accepted: its keys go with Alice
        let expires_at = TIME + 600_000;
        let handover = identity::HANDOVER_NAMESPACE;
        let unaccepted = identity::Op::TransferManager {
            account: agent,
            to: bob,
            acceptance: acceptance(&key(1), agent, bob, 0, expires_at, handover),
        };
        let not_bobs = net.refuse(&public(1), identity::MODULE, &unaccepted).await;
        assert_eq!(not_bobs, reason::UNAUTHORIZED);
        let as_consent = identity::Op::TransferManager {
            account: agent,
            to: bob,
            acceptance: acceptance(
                &key(2),
                agent,
                bob,
                0,
                expires_at,
                identity::CONSENT_NAMESPACE,
            ),
        };
        let wrong_namespace = net.refuse(&public(1), identity::MODULE, &as_consent).await;
        assert_eq!(wrong_namespace, reason::UNAUTHORIZED);
        let transfer = identity::Op::TransferManager {
            account: agent,
            to: bob,
            acceptance: acceptance(&key(2), agent, bob, 0, expires_at, handover),
        };
        net.apply(&public(1), identity::MODULE, &transfer).await;
        assert_eq!(
            net.account(agent).await.control,
            identity::Control::Managed {
                manager: bob,
                category: identity::Category::Agent,
                life: identity::Life::Active { keys: Vec::new() },
                transfers: 1,
            }
        );
        let former = net.refuse(&public(1), identity::MODULE, &suspend).await;
        assert_eq!(former, reason::UNAUTHORIZED);
        let identity::Reply::Number(holds) = net
            .ask(
                identity::MODULE,
                &identity::Query::OfKey {
                    key: bot_key.clone(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(holds, None, "the key Alice gave it is gone");
        let replayed = net.refuse(&public(2), identity::MODULE, &transfer).await;
        assert_eq!(replayed, reason::UNAUTHORIZED, "one consent, one handover");

        // then revoked for good
        net.apply(
            &public(2),
            identity::MODULE,
            &identity::Op::Revoke { account: agent },
        )
        .await;
        let revived = net
            .refuse(
                &public(2),
                identity::MODULE,
                &identity::Op::Resume { account: agent },
            )
            .await;
        assert_eq!(revived, reason::WRONG_STATE);
        let account = net.account(agent).await;
        assert_eq!(account.card.name, "Scout 2");
        assert_eq!(
            account.kind(),
            identity::Kind::Managed {
                manager: bob,
                category: identity::Category::Agent,
                standing: identity::Standing::Revoked,
            }
        );
    });
}

/// The identity role modules answer is the one ducktape's kernel speaks:
/// `abi::role::identity` here is a copy of ducktape's, and this compares
/// the two sources. (The workspace patches ducktape's `abi` to this copy,
/// so nothing links both: the source is what can drift.) The checkout is
/// the one cargo resolved `host` from.
#[test]
fn the_identity_role_is_ducktapes_byte_for_byte() {
    let metadata = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(metadata.status.success(), "{metadata:?}");
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    // `--no-deps` lists the workspace alone; the git source of `host` is
    // in this package's dependency list, as cargo resolved it
    let host = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "module-registry")
        .and_then(|package| package["dependencies"].as_array())
        .unwrap()
        .iter()
        .find(|dependency| dependency["name"] == "host")
        .expect("the founding suite links ducktape's host");
    // `git+https://…/ducktape?rev=<rev>`
    let source = host["source"].as_str().unwrap();
    let (_, rev) = source.split_once("rev=").expect("host is pinned to a rev");
    let ducktape = cargo_git_checkout(rev);
    let theirs = std::fs::read_to_string(ducktape.join("crates/kernel/abi/src/lib.rs")).unwrap();
    let ours = include_str!("../../../../sdk/abi/src/lib.rs");
    let block = |text: &str| {
        let start = text
            .find("    pub mod identity {")
            .expect("abi::role::identity");
        let end = text[start..]
            .find(
                "
    }
",
            )
            .unwrap()
            + start;
        text[start..end].to_owned()
    };
    assert_eq!(
        block(&theirs),
        block(ours),
        "abi::role::identity drifted from ducktape's"
    );
}

/// Where cargo checked ducktape out at `rev`: `$CARGO_HOME/git/checkouts/
/// ducktape-<hash>/<short rev>`.
fn cargo_git_checkout(rev: &str) -> std::path::PathBuf {
    let home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".cargo")))
        .expect("CARGO_HOME or HOME");
    let checkouts = home.join("git/checkouts");
    let short = &rev[..7];
    std::fs::read_dir(&checkouts)
        .unwrap_or_else(|error| panic!("{}: {error}", checkouts.display()))
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("ducktape-"))
        .map(|entry| entry.path().join(short))
        .find(|path| path.join("crates/kernel/abi/src/lib.rs").is_file())
        .unwrap_or_else(|| {
            panic!(
                "no checkout of ducktape @ {short} under {}",
                checkouts.display()
            )
        })
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
