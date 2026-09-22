use super::*;

#[test]
fn a_published_program_is_scheduled_by_the_authority_and_seated_at_its_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let output = net
            .apply(
                &public(7),
                module_registry::PROGRAM,
                &module_registry::Op::Publish {
                    body: program("identity"),
                },
            )
            .await;
        let code: BlobId = abi::decode(&output).unwrap();
        let entry = module_registry::Entry {
            program: "identity2".into(),
            code,
            params: Vec::new(),
        };
        let stranger = net
            .refuse(
                &public(7),
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height + 3,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        let unpublished = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height + 3,
                    change: module_registry::Change::Set(module_registry::Entry {
                        program: "ghost".into(),
                        code: BlobId::Sha256([9; 32]),
                        params: Vec::new(),
                    }),
                }),
            )
            .await;
        assert_eq!(refusal_of(&unpublished), reason::NOT_FOUND);
        let past = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: net.height,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        assert_eq!(refusal_of(&past), reason::INVALID_INPUT);
        let lands_at = net.height + 6;
        let scheduled = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: lands_at,
                    change: module_registry::Change::Set(entry.clone()),
                }),
            )
            .await;
        output_of(&scheduled);
        let taken = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: lands_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        assert_eq!(refusal_of(&taken), reason::ALREADY_EXISTS);
        let module_registry::Reply::Scheduled(pending) = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::Scheduled {
                    page: Page::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(pending.height, net.height);
        assert_eq!(pending.next, None);
        let pending = pending.items;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].height, lands_at);
        assert_eq!(
            pending[0].change,
            module_registry::Change::Set(entry.clone())
        );
        let module_registry::Reply::Programs(later) = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::At(lands_at),
            )
            .await
        else {
            panic!()
        };
        assert!(later.iter().any(|entry| entry.program == "identity2"));
        while net.height + 1 < lands_at {
            let applied = net.tick().await;
            assert!(applied.admissions.is_empty());
        }
        let applied = net.tick().await;
        assert_eq!(applied.height, lands_at);
        assert_eq!(applied.admissions.len(), 1);
        assert_eq!(applied.admissions[0].program, "identity2");
        assert!(net.host.programs().unwrap().contains_key("identity2"));
        net.apply(
            &public(7),
            "identity2",
            &identity::Op::Create {
                name: "Seven".into(),
                scheme: Scheme::Ed25519,
            },
        )
        .await;
        let module_registry::Reply::Program {
            height,
            entry: Some(seated),
        } = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::Program("identity2".into()),
            )
            .await
        else {
            panic!()
        };
        assert_eq!(height, net.height);
        assert_eq!(seated, entry);
        let removal_at = net.height + 5;
        let removal = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: removal_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        output_of(&removal);
        let cancelled = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Cancel {
                    height: removal_at,
                    program: "identity2".into(),
                },
            )
            .await;
        output_of(&cancelled);
        net.ticks(3).await;
        assert!(net.host.programs().unwrap().contains_key("identity2"));
        let removal_at = net.height + 3;
        let removal = net
            .as_authority(
                module_registry::PROGRAM,
                &module_registry::Op::Schedule(module_registry::Scheduled {
                    height: removal_at,
                    change: module_registry::Change::Remove("identity2".into()),
                }),
            )
            .await;
        output_of(&removal);
        net.ticks(2).await;
        assert!(!net.host.programs().unwrap().contains_key("identity2"));
    });
}

#[test]
fn schedule_pages_and_missing_programs_report_the_answering_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        for program in ["z", "a", "m"] {
            let receipt = net
                .as_authority(
                    module_registry::PROGRAM,
                    &module_registry::Op::Schedule(module_registry::Scheduled {
                        height: 100,
                        change: module_registry::Change::Remove(program.into()),
                    }),
                )
                .await;
            output_of(&receipt);
        }
        let mut after = None;
        let mut programs = Vec::new();
        loop {
            let module_registry::Reply::Scheduled(reply) = net
                .ask(
                    module_registry::PROGRAM,
                    &module_registry::Query::Scheduled {
                        page: Page {
                            after,
                            limit: Some(2),
                        },
                    },
                )
                .await
            else {
                panic!()
            };
            assert_eq!(reply.height, net.height);
            assert!(reply.items.len() <= 2);
            for scheduled in reply.items {
                assert_eq!(scheduled.height, 100);
                programs.push(scheduled.change.program().to_owned());
            }
            after = reply.next;
            if after.is_none() {
                break;
            }
        }
        assert_eq!(programs, ["a", "m", "z"]);
        let reply: module_registry::Reply = net
            .ask(
                module_registry::PROGRAM,
                &module_registry::Query::Program("missing".into()),
            )
            .await;
        assert_eq!(
            reply,
            module_registry::Reply::Program {
                height: net.height,
                entry: None
            }
        );
    });
}
