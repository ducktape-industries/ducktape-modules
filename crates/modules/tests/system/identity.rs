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

#[test]
fn identity_founds_accounts_admits_keys_by_consent_and_provisions_programs() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let output = net
            .apply(
                &public(1),
                identity::PROGRAM,
                &identity::Op::Create {
                    name: " Alice ".into(),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        assert_eq!(abi::decode::<AccountNumber>(&output).unwrap(), 1);
        let twice = net
            .refuse(
                &public(1),
                identity::PROGRAM,
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
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone".into()),
                consent: consent(&key(1), 1, &phone, 0, expires_at),
            },
        )
        .await;
        let identity::Reply::Account(Some(account)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 1 })
            .await
        else {
            panic!()
        };
        assert_eq!(account.name, "Alice");
        assert_eq!(account.keys().len(), 2);
        let identity::Reply::Number(of_phone) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::OfKey { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(of_phone, Some(1));
        let replayed = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), 1, &phone, 0, expires_at),
                },
            )
            .await;
        assert_eq!(replayed, reason::UNAUTHORIZED);
        let expired = net
            .refuse(
                &public(12),
                identity::PROGRAM,
                &identity::Op::AddKey {
                    scheme: Scheme::Ed25519,
                    label: None,
                    consent: consent(&key(1), 1, &public(12), 0, TIME - 1),
                },
            )
            .await;
        assert_eq!(expired, reason::UNAUTHORIZED);
        let senior = net
            .refuse(
                &phone,
                identity::PROGRAM,
                &identity::Op::RemoveKey { key: public(1) },
            )
            .await;
        assert_eq!(senior, reason::UNAUTHORIZED);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::RemoveKey { key: phone.clone() },
        )
        .await;
        let last = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::RemoveKey { key: public(1) },
            )
            .await;
        assert_eq!(last, reason::WRONG_STATE);
        let identity::Reply::Generation(generation) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Generation { key: phone.clone() },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(generation, 1);
        net.apply(
            &phone,
            identity::PROGRAM,
            &identity::Op::AddKey {
                scheme: Scheme::Ed25519,
                label: Some("phone again".into()),
                consent: consent(&key(1), 1, &phone, 1, expires_at),
            },
        )
        .await;
        let created = net
            .sent_by(
                "probe",
                identity::PROGRAM,
                &identity::Op::CreateProgram {
                    name: "Chief".into(),
                    controller: 1,
                },
            )
            .await;
        assert_eq!(
            abi::decode::<AccountNumber>(output_of(&created)).unwrap(),
            2
        );
        let identity::Reply::Account(Some(chief)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 2 })
            .await
        else {
            panic!()
        };
        assert_eq!(
            chief.control,
            identity::Control::Program {
                executor: "probe".into(),
                controller: 1,
                standing: identity::Standing::Active,
            }
        );
        let identity::Reply::Accounts(controlled) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Controlled {
                    by: 1,
                    page: Page::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(controlled.height, net.height);
        assert_eq!(controlled.next, None);
        let controlled = controlled.items;
        assert_eq!(controlled.len(), 1);
        assert_eq!(controlled[0].number, 2);
        let not_the_executor = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::SetStanding {
                    account: 2,
                    standing: identity::Standing::Suspended,
                },
            )
            .await;
        assert_eq!(not_the_executor, reason::UNAUTHORIZED);
        let suspended = net
            .sent_by(
                "probe",
                identity::PROGRAM,
                &identity::Op::SetStanding {
                    account: 2,
                    standing: identity::Standing::Suspended,
                },
            )
            .await;
        output_of(&suspended);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::SetName {
                account: 1,
                name: "Alice B".into(),
            },
        )
        .await;
        let circular = net
            .refuse(
                &public(1),
                identity::PROGRAM,
                &identity::Op::TransferControl { account: 2, to: 2 },
            )
            .await;
        assert_eq!(circular, reason::WRONG_STATE);
        let identity::Reply::Resolved(resolved) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::Resolve {
                    references: vec![
                        identity::Reference::Account(2),
                        identity::Reference::Key(phone.clone()),
                        identity::Reference::Account(9),
                    ],
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(resolved, vec![Some(2), Some(1), None]);
        net.apply(
            &public(1),
            identity::PROGRAM,
            &identity::Op::Revoke { account: 2 },
        )
        .await;
        let identity::Reply::Account(Some(revoked)) = net
            .ask(identity::PROGRAM, &identity::Query::Get { number: 2 })
            .await
        else {
            panic!()
        };
        assert_eq!(
            revoked.control,
            identity::Control::Revoked { controller: 1 }
        );
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
                identity::PROGRAM,
                &identity::Op::Create {
                    name: format!("User {seed}"),
                    scheme: Scheme::Ed25519,
                },
            )
            .await;
        }
        for _ in 0..3 {
            let receipt = net
                .sent_by(
                    "probe",
                    identity::PROGRAM,
                    &identity::Op::CreateProgram {
                        name: "Controlled".into(),
                        controller: 1,
                    },
                )
                .await;
            output_of(&receipt);
        }
        for controlled in [false, true] {
            let mut after = None;
            let mut numbers = Vec::new();
            loop {
                let page = Page {
                    after,
                    limit: Some(2),
                };
                let query = if controlled {
                    identity::Query::Controlled { by: 1, page }
                } else {
                    identity::Query::List { page }
                };
                let identity::Reply::Accounts(reply) = net.ask(identity::PROGRAM, &query).await
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
            assert_eq!(
                numbers,
                if controlled {
                    vec![4, 5, 6]
                } else {
                    vec![1, 2, 3, 4, 5, 6]
                }
            );
        }
        let identity::Reply::Accounts(reply) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::List {
                    page: Page {
                        after: Some(vec![255]),
                        limit: Some(0),
                    },
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(reply.height, net.height);
        assert!(reply.items.is_empty());
        assert_eq!(reply.next, None);
    });
}
