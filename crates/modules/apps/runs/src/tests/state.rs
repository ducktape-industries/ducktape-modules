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
