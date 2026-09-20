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
    let before: Vec<_> = queries
        .iter()
        .cloned()
        .map(|query| state_reply(&m, query))
        .collect();

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
fn maximum_legacy_carry_over_is_point_staged_without_read_amplification() {
    let models: BTreeMap<_, _> = (0..MAX_REGISTERED_AGENTS)
        .map(|index| {
            let id = format!("legacy-{index:04}");
            let mut record = record(&id);
            record.owner = RunOrigin::External(vec![(index / MAX_AGENTS_PER_OWNER + 1) as u8; 32]);
            (id, record)
        })
        .collect();
    let old = crate::state::encode_legacy_committed(
        &BTreeMap::new(),
        0,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &models,
    );
    let expected_root = crate::state::legacy_root(
        &BTreeMap::new(),
        0,
        &BTreeMap::new(),
        &BTreeMap::new(),
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
        "legacy carry-over: distinct_reads={} read_bytes={} write_count={} write_bytes={}",
        backing.distinct_reads(),
        backing.read_bytes(),
        backing.writes(),
        backing.write_bytes()
    );
    assert!(backing.distinct_reads() < 3000);
    assert_eq!(backing.read_bytes(), 0);
    assert_eq!(
        backing.writes(),
        MAX_REGISTERED_AGENTS + 1 + MAX_REGISTERED_AGENTS / MAX_AGENTS_PER_OWNER
    );
    assert!(backing.write_bytes() > 0);
    assert!(backing.value_len("model/index").is_some());
    assert!(backing.value_len("model/item/legacy-0000").is_some());
}
