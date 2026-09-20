use super::*;

#[test]
fn public_message_ids_are_bounded_and_disjoint_for_internal_run_keys() {
    let run_ids = [
        run_id_for("general", 1, "builder"),
        page_run_id_for("thread", 1, "builder"),
        job_run_id_for("job", "builder", 1),
        "attributed/1/builder".into(),
        "attributed/1/builder/post/s0".into(),
        "nested\u{1f}key/post/0".into(),
    ];
    let mut ids = BTreeSet::new();
    for run in run_ids {
        for id in [
            reply_message_id(&run),
            post_message_id(&run, "0"),
            post_message_id(&run, "s0"),
        ] {
            assert!(!id.contains(RUN_KEY_SEPARATOR));
            assert!(
                ids.insert(id),
                "distinct runs and action slots must not alias"
            );
        }
        assert_eq!(reply_message_id(&run).len(), "agent/".len() + 64);
    }
}

#[test]
fn model_run_session_and_request_state_round_trip() {
    let (mut module, registry, run_id) = awaiting_run();
    let mut open = CaptureCtx::new()
        .with_origin(user(8))
        .with_registry(&registry)
        .with_lease_holder(&run_id, &[8; 32]);
    exec(
        &mut module,
        &mut open,
        &admin(&RunsMsg::OpenAgentSession {
            attempt: 0,
            run_id: run_id.clone(),
            session_key: vec![7; 32],
        }),
    )
    .unwrap();
    commit(&mut module);
    let mut act = CaptureCtx::new()
        .with_origin(user(7))
        .with_registry(&registry)
        .with_lease_holder(&run_id, &[8; 32]);
    exec(
        &mut module,
        &mut act,
        &admin(&RunsMsg::AgentAction {
            run_id,
            request_id: "persisted".into(),
            action: create_task("persisted", "a task"),
        }),
    )
    .unwrap();
    commit(&mut module);
    let bytes = module.snapshot();
    let expected = module.root();
    let mut restored = super::module();
    restored.install(&bytes, expected).unwrap();
    assert_eq!(restored.snapshot(), bytes);
    assert_eq!(restored.root(), expected);
    assert_eq!(
        block_on(restored.action_deliveries()).unwrap(),
        block_on(module.action_deliveries()).unwrap()
    );
    let before = restored.snapshot();
    assert!(
        restored
            .install(&bytes[..bytes.len() - 1], expected)
            .is_err()
    );
    assert_eq!(restored.snapshot(), before);
}

#[test]
fn registration_queries_the_deployed_model_program_without_changing_state() {
    let module = RunsModule::new(
        "runs",
        "chat",
        "saga",
        "attribution",
        "dispatch",
        "agent",
        None,
        None,
    );
    let before = module.snapshot();
    for id in ["builder", "another-agent"] {
        let query =
            serde_json::to_vec(&serde_json::json!({"model_program":{"agent_id":id}})).unwrap();
        let reply = block_on(module.query(&query)).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&reply).unwrap(),
            serde_json::json!({"model_program":crate::model_program(id)})
        );
    }
    assert!(block_on(module.query(br#"{"model_program":{"agent_id":"Invalid ID"}}"#)).is_err());
    assert_eq!(module.snapshot(), before);
}

fn frozen_v0_root() -> StateRoot {
    let text = include_str!("../../tests/fixtures/state_v0.root").trim();
    let mut root = [0; 32];
    for (byte, pair) in root.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    StateRoot(root)
}

fn frozen_v0_bytes() -> Vec<u8> {
    include_bytes!("../../tests/fixtures/state_v0.bin").to_vec()
}

fn state_reply(module: &RunsModule, query: RunsQuery) -> RunsReply {
    runs_decode_reply(&block_on(module.query(&encode_query(&query))).unwrap()).unwrap()
}

#[test]
fn frozen_v0_models_carry_over_atomically_and_preserve_queries() {
    let bytes = frozen_v0_bytes();
    let root = frozen_v0_root();
    let mut m = super::module();
    m.install(&bytes, root).unwrap();
    let queries = [
        RunsQuery::PendingRuns,
        RunsQuery::AgentSessions,
        RunsQuery::Model {
            query: ModelQuery::Agents,
        },
    ];
    let before_receipts = m.receipts.snapshot();
    let before: Vec<_> = queries
        .iter()
        .cloned()
        .map(|query| state_reply(&m, query))
        .collect();
    let caller_run_id = match &before[0] {
        RunsReply::PendingRuns(runs) => runs[0].run_id.clone(),
        other => panic!("unexpected pending reply: {other:?}"),
    };
    let delegations_query = RunsQuery::Delegations { caller_run_id };
    let delegation_before = state_reply(&m, delegations_query.clone());
    assert_eq!(m.receipts.snapshot(), before_receipts);
    assert!(m.receipts.staged().is_empty());

    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    let enable_jobs = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::EnableJobWorker { enabled: true }),
    };
    block_on(m.execute(&mut ctx, &enable_jobs)).unwrap();
    assert_eq!(
        m.snapshot(),
        bytes,
        "uncommitted migration must be retryable"
    );
    block_on(m.abort_block()).unwrap();
    assert_eq!(m.snapshot(), bytes);
    assert!(
        block_on(m.receipts.committed("model/index"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        queries
            .iter()
            .cloned()
            .map(|query| state_reply(&m, query))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(
        state_reply(&m, delegations_query.clone()),
        delegation_before
    );

    block_on(m.execute(&mut ctx, &enable_jobs)).unwrap();
    block_on(m.commit_block()).unwrap();
    assert_ne!(m.snapshot(), bytes);
    assert!(m.legacy_models.is_none());
    assert!(
        block_on(m.receipts.committed("model/index"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        queries
            .iter()
            .cloned()
            .map(|query| state_reply(&m, query))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(state_reply(&m, delegations_query), delegation_before);
    assert!(matches!(
        state_reply(
            &m,
            RunsQuery::Model {
                query: ModelQuery::Agent {
                    agent_id: "builder".into(),
                },
            }
        ),
        RunsReply::Model(ModelReply::Agent(Some(_)))
    ));
}

#[test]
fn post_a_pending_and_session_state_carry_over_atomically() {
    let (mut source, registry, run_id) = awaiting_run();
    let mut open = CaptureCtx::new()
        .with_origin(user(8))
        .with_registry(&registry)
        .with_lease_holder(&run_id, &[8; 32]);
    block_on(source.open_agent_session(&mut open, run_id.clone(), 0, vec![7; 32])).unwrap();
    commit(&mut source);
    let pending: BTreeMap<_, _> = block_on(source.pending_list())
        .unwrap()
        .into_iter()
        .collect();
    let sessions: BTreeMap<_, _> = block_on(source.session_list())
        .unwrap()
        .into_iter()
        .map(|session| (session.run_id.clone(), session))
        .collect();
    let bytes = crate::state::encode_post_a_committed(
        &source.receipts.snapshot(),
        source.next_action_item,
        &pending,
        &sessions,
        &source.delegations,
    );
    let root = crate::state::post_a_root(
        &source.receipts.snapshot(),
        source.next_action_item,
        &pending,
        &sessions,
        &source.delegations,
    );
    let mut module = super::module();
    module.install(&bytes, root).unwrap();
    let before_receipts = module.receipts.snapshot();
    let before = [
        state_reply(&module, RunsQuery::PendingRuns),
        state_reply(&module, RunsQuery::AgentSessions),
        state_reply(
            &module,
            RunsQuery::Model {
                query: ModelQuery::Agents,
            },
        ),
    ];
    assert_eq!(module.receipts.snapshot(), before_receipts);
    assert!(module.receipts.staged().is_empty());

    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    let update = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::EnableJobWorker { enabled: true }),
    };
    block_on(module.execute(&mut ctx, &update)).unwrap();
    assert_eq!(module.snapshot(), bytes);
    block_on(module.abort_block()).unwrap();
    assert_eq!(module.snapshot(), bytes);
    block_on(module.execute(&mut ctx, &update)).unwrap();
    block_on(module.commit_block()).unwrap();
    assert_eq!(
        [
            state_reply(&module, RunsQuery::PendingRuns),
            state_reply(&module, RunsQuery::AgentSessions),
            state_reply(
                &module,
                RunsQuery::Model {
                    query: ModelQuery::Agents,
                },
            ),
        ],
        before
    );
    assert!(module.legacy_state_version.is_none());
    assert!(
        block_on(
            module
                .receipts
                .committed(&crate::state::pending_key(&dispatch_id_for(&run_id)))
        )
        .unwrap()
        .is_some()
    );
    assert!(
        block_on(
            module
                .receipts
                .committed(&crate::state::session_key(&run_id))
        )
        .unwrap()
        .is_some()
    );
}

struct LegacyDelegationFixture {
    root_run_id: String,
    next_action_item: u64,
    post_a: Vec<u8>,
    post_a_root: StateRoot,
    post_b_records: crate::receipts::Records,
    delegations: BTreeMap<String, DelegationState>,
    post_b: Vec<u8>,
    post_b_root: StateRoot,
}

fn legacy_delegation_fixture() -> LegacyDelegationFixture {
    let (mut source, registry, root_run_id) = awaiting_run();
    let mut open = CaptureCtx::new()
        .with_origin(user(8))
        .with_registry(&registry)
        .with_lease_holder(&root_run_id, &[8; 32]);
    block_on(source.open_agent_session(&mut open, root_run_id.clone(), 0, vec![7; 32])).unwrap();
    commit(&mut source);
    let root_entry = block_on(source.pending_entry(&dispatch_id_for(&root_run_id)))
        .unwrap()
        .unwrap();
    let mut pending = BTreeMap::from([(dispatch_id_for(&root_run_id), root_entry.clone())]);
    let pending_id = delegation_id_for(&root_run_id, "legacy-pending");
    let pending_child_run = delegated_run_id_for(&pending_id, "worker");
    let mut child_entry = root_entry.clone();
    child_entry.run_id = pending_child_run.clone();
    child_entry.agent_id = "worker".into();
    child_entry.delegation_id = Some(pending_id.clone());
    pending.insert(dispatch_id_for(&pending_child_run), child_entry);
    let sessions: BTreeMap<_, _> = block_on(source.session_list())
        .unwrap()
        .into_iter()
        .map(|session| (session.run_id.clone(), session))
        .collect();

    let request = DelegationRequest {
        agent_id: "worker".into(),
        instruction: "work".into(),
        skills: Vec::new(),
    };
    let terminal_id = delegation_id_for(&root_run_id, "legacy-terminal");
    let terminal = DelegationState {
        view: DelegationView {
            delegation_id: terminal_id.clone(),
            request_id: "legacy-terminal".into(),
            caller_run_id: root_run_id.clone(),
            root_run_id: root_run_id.clone(),
            callee_run_id: delegated_run_id_for(&terminal_id, "worker"),
            callee_agent_id: "worker".into(),
            status: DelegationStatus::Delivered,
            result: Some(DelegationResult {
                reply_blocks: vec![ReplyBlock {
                    kind: "paragraph".into(),
                    text: "preserved result".into(),
                    lang: None,
                }],
                output_ref: None,
                error: None,
            }),
            created_at: 1,
            completed_at: Some(2),
        },
        request: request.clone(),
    };
    let pending_delegation = DelegationState {
        view: DelegationView {
            delegation_id: pending_id.clone(),
            request_id: "legacy-pending".into(),
            caller_run_id: root_run_id.clone(),
            root_run_id: root_run_id.clone(),
            callee_run_id: pending_child_run,
            callee_agent_id: "worker".into(),
            status: DelegationStatus::Pending,
            result: None,
            created_at: 1,
            completed_at: None,
        },
        request,
    };
    let delegations = BTreeMap::from([(pending_id, pending_delegation), (terminal_id, terminal)]);
    let mut base = source.receipts.snapshot();
    base.retain(|key, _| !key.starts_with("run/") && !key.starts_with("session/"));
    let post_a = crate::state::encode_post_a_committed(
        &base,
        source.next_action_item,
        &pending,
        &sessions,
        &delegations,
    );
    let post_a_root = crate::state::post_a_root(
        &base,
        source.next_action_item,
        &pending,
        &sessions,
        &delegations,
    );
    let mut post_b_records = base;
    let ids: Vec<_> = pending.keys().cloned().collect();
    post_b_records.insert(
        crate::state::RUN_META_KEY.into(),
        crate::state::encode_pending_meta(ids.first().map(String::as_str), ids.len() as u64),
    );
    for (index, dispatch_id) in ids.iter().enumerate() {
        let prev = (index > 0).then(|| ids[index - 1].as_str());
        let next = ids.get(index + 1).map(String::as_str);
        post_b_records.insert(
            crate::state::pending_key(dispatch_id),
            crate::state::encode_pending_record(&pending[dispatch_id], prev, next),
        );
    }
    for session in sessions.values() {
        post_b_records.insert(
            crate::state::session_key(&session.run_id),
            crate::state::encode_session_record(session),
        );
    }
    let post_b = crate::state::encode_post_b_committed(
        &post_b_records,
        source.next_action_item,
        &delegations,
    );
    let post_b_root =
        crate::state::post_b_root(&post_b_records, source.next_action_item, &delegations);
    LegacyDelegationFixture {
        root_run_id,
        next_action_item: source.next_action_item,
        post_a,
        post_a_root,
        post_b_records,
        delegations,
        post_b,
        post_b_root,
    }
}

#[test]
fn post_b_129_terminal_edges_install_and_query_after_compatibility_correction() {
    let fixture = legacy_delegation_fixture();
    let mut delegations = fixture.delegations.clone();
    let mut caller_run_id = fixture.root_run_id.clone();
    for index in 0..128 {
        let request_id = format!("legacy-over-128-{index}");
        let delegation_id = delegation_id_for(&caller_run_id, &request_id);
        let callee_run_id = delegated_run_id_for(&delegation_id, "worker");
        delegations.insert(
            delegation_id.clone(),
            DelegationState {
                view: DelegationView {
                    delegation_id: delegation_id.clone(),
                    request_id,
                    caller_run_id: caller_run_id.clone(),
                    root_run_id: fixture.root_run_id.clone(),
                    callee_run_id: callee_run_id.clone(),
                    callee_agent_id: "worker".into(),
                    status: DelegationStatus::Delivered,
                    result: Some(DelegationResult {
                        reply_blocks: vec![ReplyBlock {
                            kind: "paragraph".into(),
                            text: "historical result".into(),
                            lang: None,
                        }],
                        output_ref: None,
                        error: None,
                    }),
                    created_at: 1,
                    completed_at: Some(2),
                },
                request: DelegationRequest {
                    agent_id: "worker".into(),
                    instruction: "historical work".into(),
                    skills: Vec::new(),
                },
            },
        );
        caller_run_id = callee_run_id;
    }
    assert_eq!(delegations.len(), 130);
    let bytes = crate::state::encode_post_b_committed(
        &fixture.post_b_records,
        fixture.next_action_item,
        &delegations,
    );
    assert!(bytes.len() <= sdk::MAX_STORE_VALUE_BYTES);
    let root = crate::state::post_b_root(
        &fixture.post_b_records,
        fixture.next_action_item,
        &delegations,
    );
    let mut receiver = super::module();
    receiver.install(&bytes, root).unwrap();
    match state_reply(
        &receiver,
        RunsQuery::Delegations {
            caller_run_id: fixture.root_run_id,
        },
    ) {
        RunsReply::Delegations(delegations) => assert_eq!(delegations.len(), 3),
        other => panic!("unexpected delegation reply: {other:?}"),
    }
}

#[test]
fn historical_compatibility_cap_uses_the_conservative_entry_floor() {
    let delegation_id = delegation_id_for("", "x");
    let mut minimum = DelegationState {
        view: DelegationView {
            delegation_id: delegation_id.clone(),
            request_id: "x".into(),
            caller_run_id: String::new(),
            root_run_id: "x".into(),
            callee_run_id: String::new(),
            callee_agent_id: String::new(),
            status: DelegationStatus::Delivered,
            result: None,
            created_at: 0,
            completed_at: None,
        },
        request: DelegationRequest {
            agent_id: String::new(),
            instruction: String::new(),
            skills: Vec::new(),
        },
    };
    let mut minimums = Vec::new();
    for status in [
        DelegationStatus::Pending,
        DelegationStatus::Delivered,
        DelegationStatus::Failed,
        DelegationStatus::Cancelled,
    ] {
        let mut status_minimum = usize::MAX;
        for result in [
            None,
            Some(DelegationResult {
                reply_blocks: Vec::new(),
                output_ref: None,
                error: None,
            }),
        ] {
            for completed_at in [None, Some(0)] {
                minimum.view.status = status;
                minimum.view.result = result.clone();
                minimum.view.completed_at = completed_at;
                status_minimum = status_minimum.min(serde_json::to_vec(&minimum).unwrap().len());
            }
        }
        minimums.push((status, status_minimum));
    }
    assert_eq!(minimums[0].1, 264);
    assert_eq!(minimums[1].1, 266);
    assert_eq!(minimums[2].1, 263);
    assert_eq!(minimums[3].1, 266);
    assert!(minimums.iter().all(|(_, minimum)| *minimum >= 256));
    assert_eq!(crate::state::MAX_LEGACY_DELEGATION_EDGES_PER_RUN, 3_120);
    assert_eq!(
        crate::state::MAX_LEGACY_DELEGATION_EDGES_PER_RUN,
        sdk::MAX_STORE_VALUE_BYTES / (8 + 64 + 8 + 256)
    );
}

#[test]
fn post_b_legacy_1400_edges_abort_migrate_reinstall_and_refuse_new_admission() {
    const EDGE_COUNT: usize = 1_400;
    let fixture = legacy_delegation_fixture();
    let mut delegations = fixture.delegations.clone();
    let mut caller_run_id = fixture.root_run_id.clone();
    for index in 0..(EDGE_COUNT - delegations.len()) {
        let request_id = format!("legacy-chain-{index}");
        let delegation_id = delegation_id_for(&caller_run_id, &request_id);
        let callee_run_id = delegated_run_id_for(&delegation_id, "worker");
        delegations.insert(
            delegation_id.clone(),
            DelegationState {
                view: DelegationView {
                    delegation_id,
                    request_id,
                    caller_run_id: caller_run_id.clone(),
                    root_run_id: fixture.root_run_id.clone(),
                    callee_run_id: callee_run_id.clone(),
                    callee_agent_id: "worker".into(),
                    status: DelegationStatus::Delivered,
                    result: Some(DelegationResult {
                        reply_blocks: vec![ReplyBlock {
                            kind: "paragraph".into(),
                            text: "historical result".into(),
                            lang: None,
                        }],
                        output_ref: None,
                        error: None,
                    }),
                    created_at: 1,
                    completed_at: Some(2),
                },
                request: DelegationRequest {
                    agent_id: "worker".into(),
                    instruction: "historical work".into(),
                    skills: Vec::new(),
                },
            },
        );
        caller_run_id = callee_run_id;
    }
    assert_eq!(delegations.len(), EDGE_COUNT);
    let bytes = crate::state::encode_post_b_committed(
        &fixture.post_b_records,
        fixture.next_action_item,
        &delegations,
    );
    eprintln!(
        "legacy carry-over compatibility: edges={} pending=1 snapshot_bytes={}",
        delegations.len(),
        bytes.len()
    );
    assert!(bytes.len() <= sdk::MAX_STORE_VALUE_BYTES);
    let root = crate::state::post_b_root(
        &fixture.post_b_records,
        fixture.next_action_item,
        &delegations,
    );
    let mut module = super::module();
    module.install(&bytes, root).unwrap();
    let before_root = state_reply(
        &module,
        RunsQuery::Delegations {
            caller_run_id: fixture.root_run_id.clone(),
        },
    );
    let old_snapshot = module.snapshot();
    let old_root = module.root();
    let update = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::EnableJobWorker { enabled: true }),
    };
    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    block_on(module.execute(&mut ctx, &update)).unwrap();
    assert_eq!(module.snapshot(), old_snapshot);
    block_on(module.abort_block()).unwrap();
    assert_eq!(module.snapshot(), old_snapshot);
    assert_eq!(module.root(), old_root);
    assert_eq!(
        state_reply(
            &module,
            RunsQuery::Delegations {
                caller_run_id: fixture.root_run_id.clone(),
            },
        ),
        before_root
    );

    block_on(module.execute(&mut ctx, &update)).unwrap();
    block_on(module.commit_block()).unwrap();
    let migrated = module.receipts.snapshot();
    assert!(module.legacy_state_version.is_none());
    assert!(
        crate::state::decode_committed(&module.snapshot())
            .unwrap()
            .delegations
            .is_empty()
    );
    assert!(migrated.keys().all(|key| !key.starts_with("dlg/req/")));
    assert!(
        migrated
            .values()
            .all(|value| value.len() <= sdk::MAX_STORE_VALUE_BYTES)
    );
    for (id, delegation) in &delegations {
        let header = crate::state::decode_delegation_header(
            id,
            migrated.get(&crate::state::delegation_key(id)).unwrap(),
        )
        .unwrap();
        assert_eq!(header, DelegationHeader::from_state(delegation));
        match &delegation.view.result {
            Some(expected) => {
                let reply = crate::state::decode_delegation_result(
                    id,
                    migrated
                        .get(&crate::state::delegation_reply_key(id))
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(&reply, expected);
            }
            None => assert!(!migrated.contains_key(&crate::state::delegation_reply_key(id))),
        }
    }
    assert_eq!(
        state_reply(
            &module,
            RunsQuery::Delegations {
                caller_run_id: fixture.root_run_id.clone(),
            },
        ),
        before_root
    );
    let mut reinstall = super::module();
    reinstall
        .install(&module.snapshot(), module.root())
        .unwrap();
    assert_eq!(
        state_reply(
            &reinstall,
            RunsQuery::Delegations {
                caller_run_id: fixture.root_run_id.clone(),
            },
        ),
        before_root
    );

    let hosted_backing = super::receipts::Backing::default();
    let mut hosted = super::module();
    hosted.install(&bytes, root).unwrap();
    hosted.receipts = crate::receipts::Receipts::hosted(Box::new(hosted_backing.clone()));
    for (key, value) in &fixture.post_b_records {
        hosted.receipts.stage(key.clone(), value.clone()).unwrap();
    }
    commit(&mut hosted);
    hosted_backing.forget_reads();
    hosted_backing.forget_writes();
    let mut hosted_ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    block_on(hosted.execute(&mut hosted_ctx, &update)).unwrap();
    block_on(hosted.commit_block()).unwrap();
    eprintln!(
        "legacy migration writes: edges={} distinct_reads={} writes={} write_bytes={} largest_record={}",
        delegations.len(),
        hosted_backing.distinct_reads(),
        hosted_backing.writes(),
        hosted_backing.write_bytes(),
        hosted_backing.largest_write()
    );
    assert_eq!(hosted_backing.distinct_reads(), 0);
    assert!(hosted_backing.largest_write() <= sdk::MAX_STORE_VALUE_BYTES);

    let before_delegation_records: BTreeMap<_, _> = migrated
        .iter()
        .filter(|(key, _)| key.starts_with("dlg/"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let registry = registry(&["bot", "worker"]);
    let mut refusal_ctx = CaptureCtx::new()
        .at(5)
        .with_origin(Origin::External(vec![7; 32]))
        .with_registry(&registry)
        .with_transcript("general", transcript(2))
        .with_lease_holder(&fixture.root_run_id, &[8; 32]);
    let error = exec(
        &mut module,
        &mut refusal_ctx,
        &admin(&RunsMsg::AgentAction {
            run_id: fixture.root_run_id.clone(),
            request_id: "post-migration-overflow".into(),
            action: agent_call("worker", "after migration"),
        }),
    )
    .unwrap_err();
    eprintln!("post-migration admission refusal: {error:?}");
    assert!(matches!(error, Error::Module { reason, .. } if reason == refusal::CAPACITY));
    let after_refusal: BTreeMap<_, _> = module
        .receipts
        .snapshot()
        .into_iter()
        .filter(|(key, _)| key.starts_with("dlg/"))
        .collect();
    assert_eq!(after_refusal, before_delegation_records);
    abort(&mut module);
}

#[test]
fn post_c_tree_immediately_above_historical_compatibility_cap_is_rejected() {
    let fixture = legacy_delegation_fixture();
    let root = fixture.root_run_id.clone();
    let ids: Vec<_> = (0..=crate::state::MAX_LEGACY_DELEGATION_EDGES_PER_RUN)
        .map(|index| format!("{index:064x}"))
        .collect();
    let mut records = fixture.post_b_records.clone();
    records.insert(
        crate::state::delegation_tree_key(&root),
        serde_json::to_vec(&(&root, crate::state::DelegationTree { ids, pending: 0 })).unwrap(),
    );
    let bytes =
        crate::state::encode_committed(&records, fixture.next_action_item, &BTreeMap::new());
    let expected =
        crate::state::committed_root(&records, fixture.next_action_item, &BTreeMap::new());
    let mut receiver = super::module();
    let before = receiver.snapshot();
    let before_root = receiver.root();
    let error = receiver.install(&bytes, expected).unwrap_err();
    assert!(matches!(error, Error::Module { reason, .. } if reason == refusal::CORRUPT));
    assert_eq!(receiver.snapshot(), before);
    assert_eq!(receiver.root(), before_root);
}

#[test]
fn genuine_post_a_and_post_b_delegations_migrate_atomically_to_v3() {
    let fixture = legacy_delegation_fixture();
    for (label, bytes, root) in [
        ("post-a", fixture.post_a.clone(), fixture.post_a_root),
        ("post-b", fixture.post_b.clone(), fixture.post_b_root),
    ] {
        let mut module = super::module();
        module.install(&bytes, root).unwrap();
        let queries = [
            RunsQuery::PendingRuns,
            RunsQuery::AgentSessions,
            RunsQuery::Delegations {
                caller_run_id: fixture.root_run_id.clone(),
            },
            RunsQuery::Model {
                query: ModelQuery::Agents,
            },
        ];
        let before: Vec<_> = queries
            .iter()
            .cloned()
            .map(|query| state_reply(&module, query))
            .collect();
        let old_snapshot = module.snapshot();
        let old_root = module.root();
        assert_eq!(old_snapshot, bytes, "{label}: fixture changed on install");
        assert!(module.receipts.staged().is_empty());

        let update = Msg {
            target: "runs".into(),
            payload: encode_msg(&RunsMsg::EnableJobWorker { enabled: true }),
        };
        let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
        block_on(module.execute(&mut ctx, &update)).unwrap();
        assert_eq!(
            module.snapshot(),
            old_snapshot,
            "{label}: staged migration leaked"
        );
        block_on(module.abort_block()).unwrap();
        assert_eq!(module.snapshot(), old_snapshot);
        assert_eq!(module.root(), old_root);
        assert_eq!(
            queries
                .iter()
                .cloned()
                .map(|query| state_reply(&module, query))
                .collect::<Vec<_>>(),
            before,
            "{label}: abort changed a legacy query"
        );

        block_on(module.execute(&mut ctx, &update)).unwrap();
        block_on(module.commit_block()).unwrap();
        let decoded = crate::state::decode_committed(&module.snapshot()).unwrap();
        assert!(matches!(decoded.version, crate::state::StateVersion::V3));
        assert!(decoded.delegations.is_empty());
        assert!(module.legacy_state_version.is_none());
        assert_eq!(
            queries
                .iter()
                .cloned()
                .map(|query| state_reply(&module, query))
                .collect::<Vec<_>>(),
            before,
            "{label}: migration changed a query"
        );
        let records = module.receipts.snapshot();
        assert!(records.contains_key(&crate::state::delegation_tree_key(&fixture.root_run_id)));
        assert_eq!(
            records
                .keys()
                .filter(|key| key.starts_with("dlg/req/"))
                .count(),
            0
        );
        let terminal = match state_reply(
            &module,
            RunsQuery::Delegations {
                caller_run_id: fixture.root_run_id.clone(),
            },
        ) {
            RunsReply::Delegations(delegations) => delegations
                .into_iter()
                .find(|delegation| delegation.request_id == "legacy-terminal")
                .unwrap(),
            other => panic!("unexpected delegation reply: {other:?}"),
        };
        assert_eq!(terminal.status, DelegationStatus::Delivered);
        assert_eq!(
            terminal.result.unwrap().reply_blocks[0].text,
            "preserved result"
        );
    }

    let mut inconsistent = fixture.delegations.clone();
    inconsistent
        .get_mut(&delegation_id_for(&fixture.root_run_id, "legacy-terminal"))
        .unwrap()
        .view
        .result = None;
    let bytes = crate::state::encode_post_b_committed(
        &fixture.post_b_records,
        fixture.next_action_item,
        &inconsistent,
    );
    let root = crate::state::post_b_root(
        &fixture.post_b_records,
        fixture.next_action_item,
        &inconsistent,
    );
    let mut receiver = super::module();
    assert!(matches!(
        receiver.install(&bytes, root),
        Err(Error::Module { ref reason, .. }) if reason == refusal::CORRUPT
    ));
}

#[test]
fn v3_delegation_cross_links_reject_without_replacing_the_receiver() {
    let fixture = legacy_delegation_fixture();
    let mut source = super::module();
    source
        .install(&fixture.post_b, fixture.post_b_root)
        .unwrap();
    let update = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::EnableJobWorker { enabled: true }),
    };
    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    block_on(source.execute(&mut ctx, &update)).unwrap();
    block_on(source.commit_block()).unwrap();
    let valid_snapshot = source.snapshot();
    let valid_root = source.root();
    let records = source.receipts.snapshot();
    let pending_id = delegation_id_for(&fixture.root_run_id, "legacy-pending");
    let pending_child = delegated_run_id_for(&pending_id, "worker");
    let tree_key = crate::state::delegation_tree_key(&fixture.root_run_id);
    let header_key = crate::state::delegation_key(&pending_id);
    let index_key = crate::state::delegation_run_key(&pending_child);
    let tree =
        crate::state::decode_delegation_tree(&fixture.root_run_id, records.get(&tree_key).unwrap())
            .unwrap();
    let header =
        crate::state::decode_delegation_header(&pending_id, records.get(&header_key).unwrap())
            .unwrap();
    let mut cases = Vec::new();

    let mut membership = records.clone();
    let mut broken_tree = tree.clone();
    broken_tree.ids.retain(|id| id != &pending_id);
    broken_tree.pending = 0;
    membership.insert(
        tree_key.clone(),
        crate::state::encode_delegation_tree(&fixture.root_run_id, &broken_tree),
    );
    cases.push(("membership", membership));

    let mut wrong_root = records.clone();
    let mut broken_header = header.clone();
    broken_header.root_run_id = "wrong-root".into();
    wrong_root.insert(
        header_key.clone(),
        crate::state::encode_delegation_header(&broken_header),
    );
    cases.push(("root", wrong_root));

    let mut wrong_index = records.clone();
    let mut index = crate::state::decode_delegation_run_index(
        &dispatch_id_for(&pending_child),
        records.get(&index_key).unwrap(),
    )
    .unwrap();
    index.delegation_id = "f".repeat(64);
    wrong_index.insert(
        index_key.clone(),
        crate::state::encode_delegation_run_index(&index),
    );
    cases.push(("index", wrong_index));

    let mut wrong_status = records.clone();
    let mut terminal_header = header.clone();
    terminal_header.status = DelegationStatus::Delivered;
    terminal_header.completed_at = Some(9);
    let mut terminal_tree = tree.clone();
    terminal_tree.pending = 0;
    wrong_status.insert(
        tree_key.clone(),
        crate::state::encode_delegation_tree(&fixture.root_run_id, &terminal_tree),
    );
    wrong_status.insert(
        header_key.clone(),
        crate::state::encode_delegation_header(&terminal_header),
    );
    wrong_status.insert(
        crate::state::delegation_reply_key(&pending_id),
        crate::state::encode_delegation_result(
            &pending_id,
            &DelegationResult {
                reply_blocks: Vec::new(),
                output_ref: None,
                error: None,
            },
        )
        .unwrap(),
    );
    cases.push(("terminal-callee-still-pending", wrong_status));

    let mut orphan_body = records.clone();
    orphan_body.insert(
        crate::state::delegation_reply_key(&pending_id),
        crate::state::encode_delegation_result(
            &pending_id,
            &DelegationResult {
                reply_blocks: Vec::new(),
                output_ref: None,
                error: None,
            },
        )
        .unwrap(),
    );
    cases.push(("pending-body", orphan_body));

    let mut wrong_agent = records.clone();
    let pending_key = crate::state::pending_key(&dispatch_id_for(&pending_child));
    let (mut child, prev, next) = crate::state::decode_pending_record(
        &dispatch_id_for(&pending_child),
        records.get(&pending_key).unwrap(),
    )
    .unwrap();
    child.agent_id = "reviewer".into();
    wrong_agent.insert(
        pending_key,
        crate::state::encode_pending_record(&child, prev.as_deref(), next.as_deref()),
    );
    cases.push(("pending-agent", wrong_agent));

    let terminal_id = delegation_id_for(&fixture.root_run_id, "legacy-terminal");
    let terminal_header_key = crate::state::delegation_key(&terminal_id);
    let terminal_header = crate::state::decode_delegation_header(
        &terminal_id,
        records.get(&terminal_header_key).unwrap(),
    )
    .unwrap();
    let terminal_index_key = crate::state::delegation_run_key(&terminal_header.callee_run_id);
    let terminal_reply_key = crate::state::delegation_reply_key(&terminal_id);
    let terminal_reply = records.get(&terminal_reply_key).unwrap().clone();
    let mut disconnected = records.clone();
    let orphan_caller = "disconnected-caller";
    let orphan_id = delegation_id_for(orphan_caller, "legacy-pending");
    let orphan_callee = delegated_run_id_for(&orphan_id, "worker");
    let mut graph_tree = tree.clone();
    let terminal_position = graph_tree.ids.binary_search(&terminal_id).unwrap();
    graph_tree.ids[terminal_position] = orphan_id.clone();
    graph_tree.ids.sort();
    disconnected.insert(
        tree_key.clone(),
        crate::state::encode_delegation_tree(&fixture.root_run_id, &graph_tree),
    );
    disconnected.remove(&terminal_header_key);
    disconnected.remove(&terminal_index_key);
    disconnected.remove(&terminal_reply_key);
    let mut graph_header = terminal_header;
    graph_header.delegation_id = orphan_id.clone();
    graph_header.caller_run_id = orphan_caller.into();
    graph_header.request_id = "legacy-pending".into();
    graph_header.callee_run_id = orphan_callee.clone();
    disconnected.insert(
        crate::state::delegation_key(&orphan_id),
        crate::state::encode_delegation_header(&graph_header),
    );
    disconnected.insert(
        crate::state::delegation_run_key(&orphan_callee),
        crate::state::encode_delegation_run_index(&crate::state::DelegationRunIndex {
            run_id: orphan_callee,
            delegation_id: orphan_id.clone(),
            root_run_id: fixture.root_run_id.clone(),
        }),
    );
    disconnected.insert(
        crate::state::delegation_reply_key(&orphan_id),
        terminal_reply,
    );
    cases.push(("disconnected-caller", disconnected));

    let mut receiver = super::module();
    receiver.install(&valid_snapshot, valid_root).unwrap();
    let before_snapshot = receiver.snapshot();
    let before_root = receiver.root();
    for (label, corrupted) in cases {
        let snapshot =
            crate::state::encode_committed(&corrupted, source.next_action_item, &BTreeMap::new());
        let root =
            crate::state::committed_root(&corrupted, source.next_action_item, &BTreeMap::new());
        let error = receiver.install(&snapshot, root).unwrap_err();
        assert!(
            matches!(error, Error::Module { ref reason, .. } if reason == refusal::CORRUPT),
            "{label}: {error:?}"
        );
        assert_eq!(
            receiver.snapshot(),
            before_snapshot,
            "{label}: snapshot changed"
        );
        assert_eq!(receiver.root(), before_root, "{label}: root changed");
    }
}

#[test]
fn hosted_v3_state_size_is_constant_while_native_round_trips_point_records() {
    let (source, _registry, root_run_id) = awaiting_run();
    let zero_records = source.receipts.snapshot();
    let mut max_records = zero_records.clone();
    let mut ids = Vec::new();
    for index in 0..MAX_DELEGATION_EDGES_PER_RUN {
        let request_id = format!("max-{index}");
        let delegation_id = delegation_id_for(&root_run_id, &request_id);
        let callee_run_id = delegated_run_id_for(&delegation_id, "worker");
        ids.push(delegation_id.clone());
        let header = DelegationHeader {
            delegation_id: delegation_id.clone(),
            request_id,
            caller_run_id: root_run_id.clone(),
            root_run_id: root_run_id.clone(),
            callee_run_id: callee_run_id.clone(),
            callee_agent_id: "worker".into(),
            status: DelegationStatus::Delivered,
            request_digest: [index as u8; 32],
            created_at: 1,
            completed_at: Some(2),
        };
        max_records.insert(
            crate::state::delegation_key(&delegation_id),
            crate::state::encode_delegation_header(&header),
        );
        max_records.insert(
            crate::state::delegation_run_key(&callee_run_id),
            crate::state::encode_delegation_run_index(&crate::state::DelegationRunIndex {
                run_id: callee_run_id,
                delegation_id: delegation_id.clone(),
                root_run_id: root_run_id.clone(),
            }),
        );
        max_records.insert(
            crate::state::delegation_reply_key(&delegation_id),
            crate::state::encode_delegation_result(
                &delegation_id,
                &DelegationResult {
                    reply_blocks: vec![ReplyBlock {
                        kind: "paragraph".into(),
                        text: "result".into(),
                        lang: None,
                    }],
                    output_ref: None,
                    error: None,
                },
            )
            .unwrap(),
        );
    }
    ids.sort();
    max_records.insert(
        crate::state::delegation_tree_key(&root_run_id),
        crate::state::encode_delegation_tree(
            &root_run_id,
            &crate::state::DelegationTree { ids, pending: 0 },
        ),
    );
    let max_snapshot =
        crate::state::encode_committed(&max_records, source.next_action_item, &BTreeMap::new());
    let max_root =
        crate::state::committed_root(&max_records, source.next_action_item, &BTreeMap::new());

    let mut native = super::module();
    native.install(&max_snapshot, max_root).unwrap();
    assert_eq!(native.receipts.snapshot(), max_records);
    let native_delegations = state_reply(
        &native,
        RunsQuery::Delegations {
            caller_run_id: root_run_id.clone(),
        },
    );
    let mut native_joiner = super::module();
    native_joiner
        .install(&native.snapshot(), native.root())
        .unwrap();
    assert_eq!(
        state_reply(
            &native_joiner,
            RunsQuery::Delegations {
                caller_run_id: root_run_id.clone(),
            },
        ),
        native_delegations
    );

    let zero_backing = super::receipts::Backing::default();
    let mut hosted_zero = super::module().with_receipt_store(Box::new(zero_backing.clone()));
    for (key, value) in zero_records {
        hosted_zero.receipts.stage(key, value).unwrap();
    }
    commit(&mut hosted_zero);
    let max_backing = super::receipts::Backing::default();
    let mut hosted_max = super::module().with_receipt_store(Box::new(max_backing.clone()));
    for (key, value) in max_records {
        hosted_max.receipts.stage(key, value).unwrap();
    }
    commit(&mut hosted_max);
    assert_eq!(hosted_zero.receipts.snapshot().len(), 0);
    assert_eq!(hosted_max.receipts.snapshot().len(), 0);
    assert_eq!(hosted_zero.snapshot().len(), hosted_max.snapshot().len());
    assert!(
        max_backing
            .value_len(&crate::state::delegation_tree_key(&root_run_id))
            .is_some()
    );
    assert!(matches!(
        state_reply(
            &hosted_max,
            RunsQuery::Delegations {
                caller_run_id: root_run_id,
            }
        ),
        RunsReply::Delegations(delegations) if delegations.len() == MAX_DELEGATION_EDGES_PER_RUN
    ));
}

#[test]
fn maximum_legal_legacy_carry_over_is_point_staged_without_read_amplification() {
    let models: BTreeMap<_, _> = (0..MAX_REGISTERED_AGENTS)
        .map(|index| {
            let id = format!("legacy-{index:04}");
            let mut record = record(&id);
            record.owner = RunOrigin::External(vec![(index / MAX_AGENTS_PER_OWNER + 1) as u8; 32]);
            (id, record)
        })
        .collect();
    let (source, _, template_run_id) = awaiting_run();
    let template_id = dispatch_id_for(&template_run_id);
    let template = block_on(source.pending_entry(&template_id))
        .unwrap()
        .unwrap();
    let mut pending = BTreeMap::new();
    let mut sessions = BTreeMap::new();
    let mut old = crate::state::encode_legacy_committed(
        &BTreeMap::new(),
        0,
        &pending,
        &sessions,
        &BTreeMap::new(),
        &models,
    );
    for index in 0..MAX_PENDING_RUNS as usize {
        let mut entry = template.clone();
        entry.run_id = format!("legacy-run-{index:04}");
        let dispatch_id = dispatch_id_for(&entry.run_id);
        pending.insert(dispatch_id.clone(), entry.clone());
        sessions.insert(
            entry.run_id.clone(),
            AgentSession {
                run_id: entry.run_id.clone(),
                agent_id: entry.agent_id.clone(),
                session_key: vec![7; SESSION_KEY_LEN],
                lease: ExecutionLease {
                    holder: vec![8; 32],
                    attempt: 0,
                },
                opened_at: 1,
                actions: 0,
            },
        );
        let candidate = crate::state::encode_legacy_committed(
            &BTreeMap::new(),
            0,
            &pending,
            &sessions,
            &BTreeMap::new(),
            &models,
        );
        if candidate.len() > sdk::MAX_STORE_VALUE_BYTES {
            pending.remove(&dispatch_id);
            sessions.remove(&entry.run_id);
            break;
        }
        old = candidate;
    }
    let expected_root = crate::state::legacy_root(
        &BTreeMap::new(),
        0,
        &pending,
        &sessions,
        &BTreeMap::new(),
        &models,
    );
    let backing = super::receipts::Backing::default();
    let mut m = super::module().with_receipt_store(Box::new(backing.clone()));
    m.install(&old, expected_root).unwrap();

    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    let update = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::ConfigureModel {
            operation: ModelMsg::UpdateModel {
                agent_id: "legacy-0000".into(),
                display_name: Some("migrated".into()),
                capability: None,
                recipe_hash: None,
                skills: None,
            },
        }),
    };
    backing.forget_distinct();
    block_on(m.execute(&mut ctx, &update)).unwrap();
    block_on(m.commit_block()).unwrap();

    eprintln!(
        "legacy carry-over: old_state_bytes={} models={} pending={} sessions={} distinct_reads={} read_bytes={} write_count={} write_bytes={} largest_record={}",
        old.len(),
        models.len(),
        pending.len(),
        sessions.len(),
        backing.distinct_reads(),
        backing.read_bytes(),
        backing.writes(),
        backing.write_bytes(),
        backing.largest_write()
    );
    assert!(old.len() <= sdk::MAX_STORE_VALUE_BYTES);
    assert_eq!(models.len(), MAX_REGISTERED_AGENTS);
    assert_eq!(sessions.len(), pending.len());
    assert!(models.len() + pending.len() + sessions.len() > 3000);
    assert!(backing.distinct_reads() < 3000);
    assert_eq!(backing.read_bytes(), 0);
    assert_eq!(
        backing.writes(),
        models.len()
            + 1
            + MAX_REGISTERED_AGENTS / MAX_AGENTS_PER_OWNER
            + pending.len()
            + sessions.len()
            + 1
    );
    assert!(backing.write_bytes() > 0);
    assert!(backing.largest_write() <= sdk::MAX_STORE_VALUE_BYTES);
    assert!(backing.value_len("model/index").is_some());
    assert!(backing.value_len("model/item/legacy-0000").is_some());
}

#[test]
fn maximum_legacy_delegation_population_migrates_without_body_reads() {
    let (source, _, root_run_id) = awaiting_run();
    let root_entry = block_on(source.pending_entry(&dispatch_id_for(&root_run_id)))
        .unwrap()
        .unwrap();
    let pending = BTreeMap::from([(dispatch_id_for(&root_run_id), root_entry)]);
    let mut delegations = BTreeMap::new();
    let mut previous_callee = root_run_id.clone();
    for index in 0..MAX_DELEGATION_EDGES_PER_RUN {
        let request_id = format!("legacy-{index:03}");
        let caller_run_id = if index < MAX_ACTIONS_PER_SESSION as usize {
            root_run_id.clone()
        } else {
            previous_callee.clone()
        };
        let delegation_id = delegation_id_for(&caller_run_id, &request_id);
        let callee_run_id = delegated_run_id_for(&delegation_id, "worker");
        delegations.insert(
            delegation_id.clone(),
            DelegationState {
                view: DelegationView {
                    delegation_id,
                    request_id,
                    caller_run_id,
                    root_run_id: root_run_id.clone(),
                    callee_run_id: callee_run_id.clone(),
                    callee_agent_id: "worker".into(),
                    status: DelegationStatus::Delivered,
                    result: Some(DelegationResult {
                        reply_blocks: Vec::new(),
                        output_ref: None,
                        error: None,
                    }),
                    created_at: 1,
                    completed_at: Some(2),
                },
                request: DelegationRequest {
                    agent_id: "worker".into(),
                    instruction: "work".into(),
                    skills: Vec::new(),
                },
            },
        );
        previous_callee = callee_run_id;
    }
    let models = BTreeMap::from([(String::from("bot"), record("bot"))]);
    let old = crate::state::encode_legacy_committed(
        &BTreeMap::new(),
        0,
        &pending,
        &BTreeMap::new(),
        &delegations,
        &models,
    );
    let expected_root = crate::state::legacy_root(
        &BTreeMap::new(),
        0,
        &pending,
        &BTreeMap::new(),
        &delegations,
        &models,
    );
    let backing = super::receipts::Backing::default();
    let mut module = super::module().with_receipt_store(Box::new(backing.clone()));
    module.install(&old, expected_root).unwrap();
    backing.forget_reads();
    backing.forget_writes();
    let update = Msg {
        target: "runs".into(),
        payload: encode_msg(&RunsMsg::ConfigureModel {
            operation: ModelMsg::UpdateModel {
                agent_id: "bot".into(),
                display_name: Some("migrated".into()),
                capability: None,
                recipe_hash: None,
                skills: None,
            },
        }),
    };
    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![1; 32]));
    block_on(module.execute(&mut ctx, &update)).unwrap();
    block_on(module.commit_block()).unwrap();
    eprintln!(
        "legacy delegation migration: old_state_bytes={} edges={} distinct_reads={} read_bytes={} writes={} write_bytes={} largest_record={}",
        old.len(),
        delegations.len(),
        backing.distinct_reads(),
        backing.read_bytes(),
        backing.writes(),
        backing.write_bytes(),
        backing.largest_write()
    );
    assert_eq!(delegations.len(), MAX_DELEGATION_EDGES_PER_RUN);
    assert!(old.len() <= sdk::MAX_STORE_VALUE_BYTES);
    assert_eq!(backing.distinct_reads(), 0);
    assert_eq!(backing.read_bytes(), 0);
    assert_eq!(backing.writes(), 518);
    assert_eq!(backing.write_bytes(), 110_800);
    assert!(backing.writes() < 3000);
    assert_eq!(backing.largest_write(), 8_635);
    assert!(backing.largest_write() <= sdk::MAX_STORE_VALUE_BYTES);
    assert!(
        backing
            .value_len(&crate::state::delegation_tree_key(&root_run_id))
            .is_some()
    );
    assert_eq!(
        (0..MAX_DELEGATION_EDGES_PER_RUN)
            .map(|index| {
                let caller_run_id = if index < MAX_ACTIONS_PER_SESSION as usize {
                    root_run_id.clone()
                } else {
                    let mut caller = root_run_id.clone();
                    for prior in 0..index {
                        let prior_id = delegation_id_for(
                            if prior < MAX_ACTIONS_PER_SESSION as usize {
                                &root_run_id
                            } else {
                                &caller
                            },
                            &format!("legacy-{prior:03}"),
                        );
                        caller = delegated_run_id_for(&prior_id, "worker");
                    }
                    caller
                };
                let id = delegation_id_for(&caller_run_id, &format!("legacy-{index:03}"));
                backing.write_key_count(&crate::state::delegation_key(&id))
                    + backing.write_key_count(&crate::state::delegation_reply_key(&id))
                    + backing.write_key_count(&crate::state::delegation_run_key(
                        &delegated_run_id_for(&id, "worker"),
                    ))
            })
            .sum::<usize>(),
        MAX_DELEGATION_EDGES_PER_RUN * 3
    );
}

#[test]
fn post_b_corruption_is_refused_without_replacing_an_installed_state() {
    let (mut source, _, template_run_id) = awaiting_run();
    let template_id = dispatch_id_for(&template_run_id);
    let template = block_on(source.pending_entry(&template_id))
        .unwrap()
        .unwrap();
    for index in 0..2 {
        let mut entry = template.clone();
        entry.run_id = format!("post-b-corrupt-{index}");
        let id = dispatch_id_for(&entry.run_id);
        block_on(source.stage_pending_insert(id, entry)).unwrap();
        commit(&mut source);
    }
    let valid_snapshot = source.snapshot();
    let valid_root = source.root();
    let records = source.receipts.snapshot();
    let (head, count) =
        crate::state::decode_pending_meta(records.get(crate::state::RUN_META_KEY).unwrap())
            .unwrap();
    let head = head.unwrap();
    let tail = records
        .keys()
        .filter_map(|key| key.strip_prefix("run/"))
        .find(|id| {
            let (_, _, next) = crate::state::decode_pending_record(
                id,
                records.get(&crate::state::pending_key(id)).unwrap(),
            )
            .unwrap();
            next.is_none()
        })
        .unwrap()
        .to_owned();
    let missing = "f".repeat(64);
    assert!(!records.contains_key(&crate::state::pending_key(&missing)));

    let mut cases: Vec<(&str, BTreeMap<String, Vec<u8>>)> = Vec::new();
    let mut cycle = records.clone();
    let tail_key = crate::state::pending_key(&tail);
    let (entry, prev, _) =
        crate::state::decode_pending_record(&tail, cycle.get(&tail_key).unwrap()).unwrap();
    cycle.insert(
        tail_key,
        crate::state::encode_pending_record(&entry, prev.as_deref(), Some(&head)),
    );
    cases.push(("cycle", cycle));

    let mut broken = records.clone();
    let head_key = crate::state::pending_key(&head);
    let (entry, prev, _) =
        crate::state::decode_pending_record(&head, broken.get(&head_key).unwrap()).unwrap();
    broken.insert(
        head_key,
        crate::state::encode_pending_record(&entry, prev.as_deref(), Some(&missing)),
    );
    cases.push(("missing-link", broken));

    let mut mismatch = records.clone();
    mismatch.insert(
        crate::state::RUN_META_KEY.into(),
        crate::state::encode_pending_meta(Some(&head), count + 1),
    );
    cases.push(("count-mismatch", mismatch));

    let mut receiver = super::module();
    receiver.install(&valid_snapshot, valid_root).unwrap();
    let before_snapshot = receiver.snapshot();
    let before_root = receiver.root();
    for (label, corrupted) in cases {
        let snapshot = crate::state::encode_committed(
            &corrupted,
            source.next_action_item,
            &source.delegations,
        );
        let root =
            crate::state::committed_root(&corrupted, source.next_action_item, &source.delegations);
        let error = receiver.install(&snapshot, root).unwrap_err();
        assert!(
            matches!(error, Error::Module { ref reason, .. } if reason == refusal::CORRUPT),
            "{label}: {error:?}"
        );
        assert_eq!(
            receiver.snapshot(),
            before_snapshot,
            "{label}: snapshot changed"
        );
        assert_eq!(receiver.root(), before_root, "{label}: root changed");
    }

    let (source, _, run_id) = awaiting_run();
    let entry = block_on(source.pending_entry(&dispatch_id_for(&run_id)))
        .unwrap()
        .unwrap();
    let backing = super::receipts::Backing::default();
    let mut live = super::module().with_receipt_store(Box::new(backing));
    block_on(live.stage_pending_insert(dispatch_id_for(&run_id), entry)).unwrap();
    commit(&mut live);
    let meta = block_on(live.receipts.get(crate::state::RUN_META_KEY))
        .unwrap()
        .unwrap();
    let (head, count) = crate::state::decode_pending_meta(&meta).unwrap();
    live.receipts
        .stage(
            crate::state::RUN_META_KEY.into(),
            crate::state::encode_pending_meta(head.as_deref(), count + 1),
        )
        .unwrap();
    commit(&mut live);
    let error = block_on(live.pending_list()).unwrap_err();
    assert!(matches!(error, Error::Module { reason, .. } if reason == refusal::CORRUPT));
}

#[test]
fn delegation_records_stay_bounded_at_the_lifetime_cap() {
    let root = "root";
    let mut ids = Vec::new();
    let mut headers = Vec::new();
    for index in 0..MAX_DELEGATION_EDGES_PER_RUN {
        let caller = format!("caller-{index}");
        let request_id = format!("request-{index}");
        let delegation_id = delegation_id_for(&caller, &request_id);
        ids.push(delegation_id.clone());
        headers.push(DelegationHeader {
            callee_run_id: delegated_run_id_for(&delegation_id, "worker"),
            delegation_id,
            request_id,
            caller_run_id: caller,
            root_run_id: root.into(),
            callee_agent_id: "worker".into(),
            status: DelegationStatus::Pending,
            request_digest: [0; 32],
            created_at: 1,
            completed_at: None,
        });
    }
    ids.sort();
    let tree = crate::state::DelegationTree {
        ids,
        pending: MAX_DELEGATIONS_PER_RUN as u64,
    };
    let request = DelegationRequest {
        agent_id: "worker".into(),
        instruction: "x".repeat(MAX_DELEGATION_INSTRUCTION_BYTES),
        skills: Vec::new(),
    };
    let result = DelegationResult {
        reply_blocks: vec![ReplyBlock {
            kind: "paragraph".into(),
            text: "x".repeat(MAX_REPLY_BLOCKS_BYTES),
            lang: None,
        }],
        output_ref: None,
        error: None,
    };
    let tree_bytes = crate::state::encode_delegation_tree(root, &tree);
    let header_bytes = crate::state::encode_delegation_header(&headers[0]);
    let request_bytes = crate::state::encode_delegation_request(&request).unwrap();
    let result_bytes =
        crate::state::encode_delegation_result(&headers[0].delegation_id, &result).unwrap();
    let header_total: usize = headers
        .iter()
        .map(|header| crate::state::encode_delegation_header(header).len())
        .sum();
    let index_total: usize = headers
        .iter()
        .map(|header| {
            crate::state::encode_delegation_run_index(&crate::state::DelegationRunIndex {
                run_id: header.callee_run_id.clone(),
                delegation_id: header.delegation_id.clone(),
                root_run_id: root.into(),
            })
            .len()
        })
        .sum();
    let migration_write_bytes = tree_bytes.len()
        + header_total
        + index_total
        + result_bytes.len() * (MAX_DELEGATION_EDGES_PER_RUN - MAX_DELEGATIONS_PER_RUN);
    eprintln!(
        "delegation records: tree={} header={} request={} reply={} store_cap={} header_total={} index_total={} migration_8_pending_writes=505 migration_8_pending_bytes={}",
        tree_bytes.len(),
        header_bytes.len(),
        request_bytes.len(),
        result_bytes.len(),
        sdk::MAX_STORE_VALUE_BYTES,
        header_total,
        index_total,
        migration_write_bytes
    );
    for bytes in [&tree_bytes, &header_bytes, &request_bytes, &result_bytes] {
        assert!(bytes.len() <= sdk::MAX_STORE_VALUE_BYTES);
    }
    assert_eq!(tree.ids.len(), MAX_DELEGATION_EDGES_PER_RUN);
    assert!(
        crate::state::decode_delegation_tree(
            root,
            &crate::state::encode_delegation_tree(root, &crate::state::DelegationTree::default())
        )
        .is_err()
    );
}
