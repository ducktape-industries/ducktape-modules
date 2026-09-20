use super::*;

// ---- model configuration and recipe updates --------------------------------

#[test]
fn a_model_registration_registers_the_dispatch_recipe() {
    let mut m = module();
    let mut ctx = CaptureCtx::new().with_agent_origin();
    m.apply_model_change(
        &mut ctx,
        ModelChange::Registered {
            agent_id: "bot".into(),
            capability: "model-1".into(),
        },
    )
    .unwrap();

    let recipes = ctx.dispatch_msgs();
    assert_eq!(recipes.len(), 1);
    let DispatchMsg::RegisterRecipe {
        recipe_id,
        capability,
        routing,
        output_contract,
        max_attempts,
        deadline_views,
        lease_views,
        ..
    } = &recipes[0]
    else {
        panic!("expected a recipe registration");
    };
    assert_eq!(*recipe_id, recipe_id_for("bot"));
    assert_eq!(*capability, "model-1");
    assert_eq!(*routing, Routing::Rendezvous);
    assert_eq!(
        *output_contract,
        OutputContract::Text,
        "raw model text back; THIS module normalizes"
    );
    assert_eq!(*max_attempts, RUN_MAX_ATTEMPTS);
    assert_eq!(*deadline_views, Some(RUN_DEADLINE_VIEWS));
    assert_eq!(*lease_views, Some(RUN_LEASE_VIEWS));
}

#[test]
fn a_capability_change_event_retunes_the_dispatch_recipe() {
    let mut m = module();
    let mut ctx = CaptureCtx::new().with_agent_origin();
    m.apply_model_change(
        &mut ctx,
        ModelChange::CapabilityChanged {
            agent_id: "bot".into(),
            capability: "model-2".into(),
        },
    )
    .unwrap();
    assert_eq!(
        ctx.dispatch_msgs(),
        vec![DispatchMsg::UpdateRecipe {
            recipe_id: recipe_id_for("bot"),
            description: None,
            capability: Some("model-2".into()),
            routing: None,
            output_contract: None,
            max_attempts: None,
        }]
    );
}

#[test]
fn a_model_removal_removes_the_dispatch_recipe() {
    let mut m = module();
    let mut ctx = CaptureCtx::new().with_agent_origin();
    m.apply_model_change(
        &mut ctx,
        ModelChange::Deregistered {
            agent_id: "bot".into(),
        },
    )
    .unwrap();
    assert_eq!(
        ctx.dispatch_msgs(),
        vec![DispatchMsg::RemoveRecipe {
            recipe_id: recipe_id_for("bot"),
        }]
    );
}

#[test]
fn the_model_recipe_update_may_error_to_abort_the_registration_block() {
    let mut m = module();

    // an agent id whose recipe id would blow the dispatch id cap: the
    // hook ERRORS, aborting the registration block — the atomic recipe
    // seam (the registry record must never land without its recipe).
    let oversized = "x".repeat(dispatch::MAX_ID_BYTES);
    let mut ctx = CaptureCtx::new().with_agent_origin();
    let err = m
        .apply_model_change(
            &mut ctx,
            ModelChange::Registered {
                agent_id: oversized,
                capability: "model-1".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::Module { sentence: reason, .. } if reason.contains("recipe id")));

    // malformed bytes from the registry origin error the same way — the
    // registry is genesis-trusted code, so this is a bug, not traffic.
    let mut ctx = CaptureCtx::new().with_agent_origin();
    let err = exec(
        &mut m,
        &mut ctx,
        &Msg {
            target: "runs".into(),
            payload: b"not an agent event".to_vec(),
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::Module { .. }));
}

fn register_model(m: &mut RunsModule, id: &str, owner: u8) -> Result<(), Error> {
    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![owner; 32]));
    block_on(m.configure_model(
        &mut ctx,
        ModelMsg::RegisterModel {
            account: 2,
            agent_id: id.into(),
            display_name: id.into(),
            capability: "model-1".into(),
            recipe_hash: None,
            skills: None,
        },
    ))
}

fn model_reply(m: &RunsModule, query: ModelQuery) -> ModelReply {
    match runs_decode_reply(&block_on(m.query(&encode_query(&RunsQuery::Model { query }))).unwrap())
        .unwrap()
    {
        RunsReply::Model(reply) => reply,
        other => panic!("unexpected reply: {other:?}"),
    }
}

#[test]
fn point_registry_is_bounded_and_has_constant_post_a_state() {
    let one_backing = super::receipts::Backing::default();
    let mut one = module().with_receipt_store(Box::new(one_backing.clone()));
    register_model(&mut one, "agent-0000", 1).unwrap();
    commit(&mut one);

    let backing = super::receipts::Backing::default();
    let mut m = module().with_receipt_store(Box::new(backing.clone()));
    let ids: Vec<String> = (0..MAX_REGISTERED_AGENTS)
        .map(|index| format!("agent-{index:04}"))
        .collect();
    for (index, id) in ids.iter().enumerate() {
        register_model(&mut m, id, (index / MAX_AGENTS_PER_OWNER + 1) as u8).unwrap();
    }
    commit(&mut m);

    let one_state_len = one.snapshot().len();
    let many_state_len = m.snapshot().len();
    eprintln!("post-A __state lengths: one={one_state_len} max={many_state_len}");
    assert_eq!(one_state_len, many_state_len);
    assert!(
        !m.snapshot()
            .windows(b"model/index".len())
            .any(|window| { window == b"model/index" })
    );
    let agents = model_reply(&m, ModelQuery::Agents).into_agents();
    assert_eq!(agents.len(), MAX_REGISTERED_AGENTS);
    assert_eq!(
        agents
            .iter()
            .map(|record| record.agent_id.as_str())
            .collect::<Vec<_>>(),
        ids.iter().map(String::as_str).collect::<Vec<_>>()
    );

    backing.forget_reads();
    let target = ids[MAX_REGISTERED_AGENTS / 2].clone();
    let expected = match model_reply(
        &m,
        ModelQuery::Agent {
            agent_id: target.clone(),
        },
    ) {
        ModelReply::Agent(Some(record)) => record,
        reply => panic!("unexpected reply: {reply:?}"),
    };
    eprintln!(
        "post-A Model::Agent distinct_reads={} read_bytes={}",
        backing.distinct_reads(),
        backing.read_bytes()
    );
    assert_eq!(backing.distinct_reads(), 1);
    assert_eq!(
        backing.value_len(&format!("model/item/{target}")),
        Some(sdk::wire::encode(&expected).len())
    );

    let largest_model = ids
        .iter()
        .map(|id| backing.value_len(&format!("model/item/{id}")).unwrap())
        .max()
        .unwrap();
    let largest_owner = (1..=MAX_REGISTERED_AGENTS / MAX_AGENTS_PER_OWNER)
        .map(|owner| {
            let key = RunsModule::model_owner_key(&Origin::External(vec![owner as u8; 32]));
            backing.value_len(&key).unwrap()
        })
        .max()
        .unwrap();
    eprintln!(
        "post-A largest values: index={} model={} owner={}",
        backing.value_len("model/index").unwrap(),
        largest_model,
        largest_owner
    );
    assert!(largest_model <= sdk::MAX_STORE_VALUE_BYTES);
    assert!(largest_owner <= sdk::MAX_STORE_VALUE_BYTES);
    assert!(
        backing.value_len("model/index").expect("registry index") <= sdk::MAX_STORE_VALUE_BYTES
    );

    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![99; 32]));
    let err = block_on(m.configure_model(
        &mut ctx,
        ModelMsg::RegisterModel {
            account: 2,
            agent_id: "agent-over-cap".into(),
            display_name: "agent-over-cap".into(),
            capability: "model-1".into(),
            recipe_hash: None,
            skills: None,
        },
    ))
    .unwrap_err();
    assert!(matches!(err, Error::Module { reason, .. } if reason == refusal::CAPACITY));
    eprintln!("post-A 1025th register refusal=CAPACITY");
}

#[test]
fn owner_cap_and_model_indexes_are_read_your_writes_and_atomic() {
    let backing = super::receipts::Backing::default();
    let mut m = module().with_receipt_store(Box::new(backing.clone()));
    for index in 0..MAX_AGENTS_PER_OWNER {
        register_model(&mut m, &format!("owned-{index:02}"), 7).unwrap();
    }
    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![7; 32]));
    let err = block_on(m.configure_model(
        &mut ctx,
        ModelMsg::RegisterModel {
            account: 2,
            agent_id: "owned-over-cap".into(),
            display_name: "owned-over-cap".into(),
            capability: "model-1".into(),
            recipe_hash: None,
            skills: None,
        },
    ))
    .unwrap_err();
    assert!(matches!(err, Error::Module { reason, .. } if reason == refusal::CAPACITY));
    assert_eq!(
        model_reply(&m, ModelQuery::Agents).into_agents().len(),
        MAX_AGENTS_PER_OWNER
    );
    commit(&mut m);

    let mut ctx = CaptureCtx::new().with_origin(Origin::External(vec![7; 32]));
    block_on(m.configure_model(
        &mut ctx,
        ModelMsg::PauseModel {
            agent_id: "owned-00".into(),
        },
    ))
    .unwrap();
    assert!(matches!(
        model_reply(
            &m,
            ModelQuery::Agent {
                agent_id: "owned-00".into()
            }
        ),
        ModelReply::Agent(Some(ModelRecord {
            status: ModelStatus::Paused,
            ..
        }))
    ));
    block_on(m.configure_model(
        &mut ctx,
        ModelMsg::DeregisterModel {
            agent_id: "owned-00".into(),
        },
    ))
    .unwrap();
    assert!(matches!(
        model_reply(
            &m,
            ModelQuery::Agent {
                agent_id: "owned-00".into()
            }
        ),
        ModelReply::Agent(None)
    ));
    abort(&mut m);
    assert!(matches!(
        model_reply(
            &m,
            ModelQuery::Agent {
                agent_id: "owned-00".into()
            }
        ),
        ModelReply::Agent(Some(_))
    ));
    block_on(m.configure_model(
        &mut ctx,
        ModelMsg::DeregisterModel {
            agent_id: "owned-00".into(),
        },
    ))
    .unwrap();
    commit(&mut m);
    assert!(backing.value_len("model/item/owned-00").is_none());
    assert!(matches!(
        model_reply(
            &m,
            ModelQuery::Agent {
                agent_id: "owned-00".into()
            }
        ),
        ModelReply::Agent(None)
    ));
}

trait ModelReplyExt {
    fn into_agents(self) -> Vec<ModelRecord>;
}

impl ModelReplyExt for ModelReply {
    fn into_agents(self) -> Vec<ModelRecord> {
        match self {
            ModelReply::Agents(records) => records,
            other => panic!("expected agent list, got {other:?}"),
        }
    }
}
