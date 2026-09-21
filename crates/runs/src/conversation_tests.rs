use super::*;
use futures::executor::block_on;
use sdk::refusal;
use sdk_testkit::TestCtx;

fn state() -> ConversationView {
    ConversationView {
        conversation_id: "room".into(),
        agent_id: "coordinator".into(),
        account: 7,
        source: ConversationSource::Detached,
        history_prefix: "/shared/history/room".into(),
        session_path: "session.jsonl".into(),
        packages: Vec::new(),
        status: ConversationStatus::Active,
        source_cursor: 0,
        admitted_cursor: 0,
        completed_cursor: 0,
        next_turn: 1,
        active_turn: None,
        history: None,
    }
}
fn event(sequence: u64) -> ConversationEvent {
    ConversationEvent {
        sequence,
        operation_id: format!("input-{sequence}"),
        actor: Origin::External(vec![sequence as u8]),
        input: ConversationInput::Control {
            content: format!("human {sequence}"),
        },
        admitted_at: 1,
    }
}
fn transition(state: &mut ConversationView, input: Input) -> Vec<Command> {
    let commands = step(state, input).unwrap();
    for command in &commands {
        if let Command::State(next) = command {
            *state = next.as_ref().clone();
        }
    }
    commands
}
fn checkpoint_for(state: &ConversationView, revision: u64) -> ConversationCheckpoint {
    ConversationCheckpoint {
        run_id: state.active_turn.as_ref().unwrap().run_id.clone(),
        attempt: 1,
        operation_id: format!("checkpoint-{revision}"),
        delivery: true,
        history: ConversationHistory {
            revision,
            snapshot: "a".repeat(64),
        },
    }
}
#[test]
fn two_human_inputs_serialize_and_model_end_does_not_release_ownership() {
    let mut s = state();
    transition(&mut s, Input::Append(event(1)));
    transition(&mut s, Input::Queue);
    let first = s.active_turn.as_ref().unwrap().clone();
    transition(&mut s, Input::Append(event(2)));
    transition(&mut s, Input::Queue);
    assert_eq!(s.active_turn.as_ref().unwrap(), &first);
    transition(&mut s, Input::Requested);
    transition(&mut s, Input::Started);
    let checkpoint = checkpoint_for(&s, 1);
    transition(&mut s, Input::Checkpoint(checkpoint));
    transition(&mut s, Input::Action("late-tool-receipt".into()));
    transition(&mut s, Input::ModelEnded(RunOutcome::ResultAccepted));
    assert!(
        step(&s, Input::Drained(Some(1))).is_err(),
        "tool completion is a separate fact"
    );
    transition(&mut s, Input::ActionsDrained(1));
    transition(&mut s, Input::Drained(Some(1)));
    assert_eq!(s.completed_cursor, 1);
    assert_eq!(s.history.as_ref().unwrap().revision, 1);
    transition(&mut s, Input::Queue);
    let second = s.active_turn.as_ref().unwrap();
    assert_eq!(
        (second.from_cursor, second.through_cursor, second.turn),
        (1, 2, 2)
    );
    assert_ne!(second.run_id, first.run_id);
}
#[test]
fn missing_native_history_pauses_without_consuming_input() {
    let mut s = state();
    transition(&mut s, Input::Append(event(1)));
    transition(&mut s, Input::Queue);
    transition(&mut s, Input::Requested);
    transition(&mut s, Input::Started);
    transition(&mut s, Input::ModelEnded(RunOutcome::Failed));
    transition(&mut s, Input::Drained(Some(1)));
    assert_eq!(s.completed_cursor, 0);
    assert!(matches!(s.status, ConversationStatus::Paused { .. }));
    assert!(s.active_turn.is_some());
}
#[test]
fn checkpoint_revision_and_turn_are_fenced() {
    let mut s = state();
    transition(&mut s, Input::Append(event(1)));
    transition(&mut s, Input::Queue);
    transition(&mut s, Input::Requested);
    transition(&mut s, Input::Started);
    let mut bad = checkpoint_for(&s, 2);
    assert!(step(&s, Input::Checkpoint(bad.clone())).is_err());
    bad.history.revision = 1;
    bad.run_id = "stale-turn".into();
    assert!(step(&s, Input::Checkpoint(bad)).is_err());
}
#[test]
fn failed_new_attempt_keeps_last_native_checkpoint_but_cannot_consume_its_turn() {
    let mut s = state();
    transition(&mut s, Input::Append(event(1)));
    transition(&mut s, Input::Queue);
    transition(&mut s, Input::Requested);
    transition(&mut s, Input::Started);
    let checkpoint = checkpoint_for(&s, 1);
    transition(&mut s, Input::Checkpoint(checkpoint));
    transition(&mut s, Input::ModelEnded(RunOutcome::Failed));
    transition(&mut s, Input::Drained(Some(2)));
    assert_eq!(s.completed_cursor, 0);
    assert!(
        s.active_turn.as_ref().unwrap().checkpoint.is_some(),
        "attempt loss cannot erase committed native history"
    );
    transition(&mut s, Input::Retry);
    assert_eq!(s.history.as_ref().unwrap().revision, 1);
    assert_eq!(s.active_turn.as_ref().unwrap().through_cursor, 1);
}
#[test]
fn duplicate_admission_restart_and_root_authentication() {
    block_on(async {
        let mut module = RunsModule::new(
            "runs",
            "chat",
            "saga",
            "attribution",
            "dispatch",
            "agent",
            None,
            None,
        );
        let initial = state();
        module.write_conversation_state(&initial).unwrap();
        let ctx = TestCtx::with_env(sdk::Env {
            height: 1,
            consensus_time: 1,
            origin: Origin::Program(7),
            me: "runs".into(),
            cause: sdk::Cause::Direct,
        });
        let input = ConversationInput::Control {
            content: "human input".into(),
        };
        module
            .append_conversation_input(&ctx, "room".into(), "stable-op".into(), input.clone())
            .await
            .unwrap();
        module
            .append_conversation_input(&ctx, "room".into(), "stable-op".into(), input)
            .await
            .unwrap();
        assert_eq!(
            module
                .require_conversation("room")
                .await
                .unwrap()
                .admitted_cursor,
            1
        );
        assert!(
            module
                .append_conversation_input(
                    &ctx,
                    "room".into(),
                    "stable-op".into(),
                    ConversationInput::Control {
                        content: "different".into()
                    }
                )
                .await
                .is_err()
        );
        module.commit_block().await.unwrap();
        let root = module.root();
        let snapshot = module.snapshot();
        let mut restored = RunsModule::new(
            "runs",
            "chat",
            "saga",
            "attribution",
            "dispatch",
            "agent",
            None,
            None,
        );
        restored.install(&snapshot, root).unwrap();
        assert_eq!(restored.root(), root);
        assert_eq!(
            restored.conversation("room").await.unwrap(),
            module.conversation("room").await.unwrap()
        );
        assert_eq!(
            restored
                .conversation_events("room", 1, 64)
                .await
                .unwrap()
                .len(),
            1
        );
        let mut invalid = module.receipts.snapshot();
        let mut bad = module.require_conversation("room").await.unwrap();
        bad.admitted_cursor = 2;
        invalid.insert(key("state", "room"), sdk::wire::encode(&bad));
        assert!(
            validate_records(&invalid).is_err(),
            "missing source events fail closed even under a recomputed hash"
        );
    });
}
#[test]
fn duplicate_model_settlement_and_job_continuation_keep_history() {
    let mut s = state();
    s.source = ConversationSource::Job {
        job_id: "first-job".into(),
    };
    transition(
        &mut s,
        Input::JobStarted {
            run_id: "first-run".into(),
            event: event(1),
        },
    );
    let checkpoint = checkpoint_for(&s, 1);
    transition(&mut s, Input::Checkpoint(checkpoint));
    transition(&mut s, Input::ModelEnded(RunOutcome::ResultAccepted));
    let ended = s.clone();
    transition(&mut s, Input::ModelEnded(RunOutcome::ResultAccepted));
    assert_eq!(s, ended);
    transition(&mut s, Input::Drained(Some(1)));
    transition(
        &mut s,
        Input::JobStarted {
            run_id: "continuation-run".into(),
            event: event(2),
        },
    );
    assert_eq!(s.history.as_ref().unwrap().revision, 1);
    assert_eq!(s.active_turn.as_ref().unwrap().run_id, "continuation-run");
    assert_eq!(s.active_turn.as_ref().unwrap().turn, 2);
}
#[test]
fn conversation_administration_tracks_current_identity_controller() {
    block_on(async {
        let mut module = RunsModule::new(
            "runs",
            "chat",
            "saga",
            "attribution",
            "dispatch",
            "agent",
            None,
            None,
        );
        module.write_conversation_state(&state()).unwrap();
        let context = |signer: u8, controller: u64| {
            TestCtx::with_env(sdk::Env {
                height: 1,
                consensus_time: 1,
                origin: Origin::External(vec![signer]),
                me: "runs".into(),
                cause: sdk::Cause::Direct,
            })
            .on_query("identity", move |bytes| {
                let (number, control) =
                    match identity::decode_query(bytes).map_err(|sentence| Error::Module {
                        reason: refusal::INVALID_INPUT.into(),
                        sentence,
                    })? {
                        identity::IdentityQuery::Get { number } => (
                            number,
                            identity::Control::Program {
                                controller,
                                executor: "agent".into(),
                                generation: 1,
                                standing: identity::ProgramStanding::Active,
                            },
                        ),
                        identity::IdentityQuery::OfKey { key } => {
                            (u64::from(key[0]), identity::Control::Keys)
                        }
                    };
                Ok(identity::encode_reply(&identity::IdentityReply::Account(
                    Some(identity::AccountView {
                        number,
                        name: "member".into(),
                        control,
                        keys: Vec::new(),
                        avatar: None,
                        bio: None,
                        updated_at: 1,
                    }),
                )))
            })
        };
        module
            .activate_conversation(&context(9, 9), "room".into(), "first".into(), true)
            .await
            .unwrap();
        assert!(
            module
                .activate_conversation(&context(9, 8), "room".into(), "first".into(), true)
                .await
                .is_err(),
            "a historical controller cannot replay its old capability"
        );
        module
            .activate_conversation(
                &context(8, 8),
                "room".into(),
                "new-controller".into(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            module.require_conversation("room").await.unwrap().status,
            ConversationStatus::Inactive
        );
    });
}
#[test]
fn scheduled_input_uses_network_clock_replaces_cancels_and_fires_once() {
    block_on(async {
        for (unit, due_at) in [
            (sdk::genesis_config::TimeUnit::Height, 110),
            (sdk::genesis_config::TimeUnit::Millis, 10_100),
        ] {
            let mut module = RunsModule::new(
                "runs",
                "chat",
                "saga",
                "attribution",
                "dispatch",
                "agent",
                None,
                None,
            )
            .with_time_unit(unit);
            module.write_conversation_state(&state()).unwrap();
            let ctx = TestCtx::with_env(sdk::Env {
                height: 1,
                consensus_time: 100,
                origin: Origin::Program(7),
                me: "runs".into(),
                cause: sdk::Cause::Direct,
            });
            let input = ConversationInput::Event {
                kind: "checkin".into(),
                content: serde_json::json!({}),
            };
            module
                .schedule_conversation_input(
                    &ctx,
                    "room".into(),
                    "schedule".into(),
                    "checkin".into(),
                    Some(10),
                    input.clone(),
                )
                .await
                .unwrap();
            module
                .schedule_conversation_input(
                    &ctx,
                    "room".into(),
                    "schedule".into(),
                    "checkin".into(),
                    Some(10),
                    input.clone(),
                )
                .await
                .unwrap();
            assert_eq!(
                module.conversation_schedules("room").await.unwrap()[0].status,
                ConversationScheduleStatus::Pending { due_at }
            );
            let before_due = TestCtx::at_height(due_at - 1);
            module.crank_conversation_inputs(&before_due).await.unwrap();
            assert_eq!(
                module
                    .require_conversation("room")
                    .await
                    .unwrap()
                    .admitted_cursor,
                0
            );
            module.commit_block().await.unwrap();
            let mut restored = RunsModule::new(
                "runs",
                "chat",
                "saga",
                "attribution",
                "dispatch",
                "agent",
                None,
                None,
            )
            .with_time_unit(unit);
            restored.install(&module.snapshot(), module.root()).unwrap();
            let due = TestCtx::at_height(due_at);
            restored.crank_conversation_inputs(&due).await.unwrap();
            restored.crank_conversation_inputs(&due).await.unwrap();
            assert_eq!(
                restored
                    .require_conversation("room")
                    .await
                    .unwrap()
                    .admitted_cursor,
                1
            );
            assert_eq!(
                restored.conversation_events("room", 1, 1).await.unwrap()[0].actor,
                Origin::Program(7)
            );
            restored
                .schedule_conversation_input(
                    &ctx,
                    "room".into(),
                    "replace".into(),
                    "checkin".into(),
                    Some(20),
                    input.clone(),
                )
                .await
                .unwrap();
            restored
                .schedule_conversation_input(
                    &ctx,
                    "room".into(),
                    "cancel".into(),
                    "checkin".into(),
                    None,
                    input,
                )
                .await
                .unwrap();
            restored
                .crank_conversation_inputs(&TestCtx::at_height(u64::MAX))
                .await
                .unwrap();
            assert_eq!(
                restored
                    .require_conversation("room")
                    .await
                    .unwrap()
                    .admitted_cursor,
                1
            );
            assert_eq!(
                restored.conversation_schedules("room").await.unwrap()[0].status,
                ConversationScheduleStatus::Cancelled
            );
        }
    });
}
#[test]
fn explicit_recovery_never_reopens_a_terminal_worker_job() {
    let mut s = state();
    s.source = ConversationSource::Job {
        job_id: "original".into(),
    };
    transition(
        &mut s,
        Input::JobStarted {
            run_id: "run".into(),
            event: event(1),
        },
    );
    transition(&mut s, Input::ModelEnded(RunOutcome::Failed));
    transition(&mut s, Input::Drained(Some(1)));
    transition(&mut s, Input::Retry);
    assert!(
        s.active_turn.is_none(),
        "a new Job must authorize the next independent execution"
    );
    assert_eq!(s.completed_cursor, 1);
}
fn assert_dispatch_shape(source: &str, name: &str) {
    let file = syn::parse_file(source).unwrap();
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == name => Some(function),
            _ => None,
        })
        .unwrap();
    assert_eq!(function.block.stmts.len(), 1);
    let syn::Stmt::Expr(syn::Expr::Match(dispatch), None) = &function.block.stmts[0] else {
        panic!("step must be only a match");
    };
    for arm in &dispatch.arms {
        assert!(arm.guard.is_none());
        assert!(!matches!(arm.pat, syn::Pat::Wild(_)));
        assert!(
            matches!(*arm.body, syn::Expr::Call(_)),
            "each input delegates exactly once"
        );
    }
}
#[test]
fn dispatch_is_one_match_with_one_handler_per_variant() {
    assert_dispatch_shape(include_str!("conversations.rs"), "step");
    assert_dispatch_shape(include_str!("conversation_schedule.rs"), "schedule_step");
}

/// Conversations holding an open wake or a pending timer while ONE more is
/// written, acked, delivered or cranked. Large enough that a queue read whole
/// would dwarf the fixed cost of a point-addressed one.
const CROWD: u64 = 2000;

fn counted() -> (RunsModule, crate::tests::receipts::Backing) {
    let backing = crate::tests::receipts::Backing::default();
    let module = RunsModule::new(
        "runs",
        "chat",
        "saga",
        "attribution",
        "dispatch",
        "agent",
        None,
        None,
    )
    .with_time_unit(sdk::genesis_config::TimeUnit::Height)
    .with_receipt_store(Box::new(backing.clone()));
    (module, backing)
}

/// Point reads and the bytes they decoded. The count alone would not catch a
/// whole-record queue: reading one map is one read however many entries it
/// holds. The bytes are what grew with the network.
fn cost(backing: &crate::tests::receipts::Backing) -> (usize, usize) {
    (backing.reads(), backing.read_bytes())
}
fn since(before: (usize, usize), backing: &crate::tests::receipts::Backing) -> (usize, usize) {
    let (reads, bytes) = cost(backing);
    (reads - before.0, bytes - before.1)
}

/// The same point reads either way, and byte drift only from the width of a
/// varint item id or due time — never from the number of conversations. An
/// unconverted queue would drift by tens of kilobytes here.
fn independent_of_n<const N: usize>(small: [(usize, usize); N], large: [(usize, usize); N]) {
    assert_eq!(
        small.map(|(reads, _)| reads),
        large.map(|(reads, _)| reads),
        "read count grew with the network: {small:?} then {large:?}"
    );
    for ((_, small), (_, large)) in small.into_iter().zip(large) {
        assert!(
            large <= small + 64,
            "bytes read grew with the network: {small} then {large}"
        );
    }
}

fn program_ctx(height: u64) -> TestCtx {
    TestCtx::with_env(sdk::Env {
        height,
        consensus_time: height,
        origin: Origin::Program(7),
        me: "runs".into(),
        cause: sdk::Cause::Direct,
    })
}

/// R5/R6: a wake write, the per-block delivery round and an ack each cost the
/// same whether one conversation is waiting or two thousand are.
#[test]
fn wake_writes_deliveries_and_acks_read_past_every_other_conversation() {
    block_on(async {
        let mut counts = Vec::new();
        for crowd in [1, CROWD] {
            let (mut module, backing) = counted();
            module.write_conversation_state(&state()).unwrap();
            module
                .write_conversation_state(&ConversationView {
                    conversation_id: "crowd-0".into(),
                    ..state()
                })
                .unwrap();
            let ctx = program_ctx(1);
            for n in 0..crowd {
                module
                    .write_conversation_wake(&ctx, &format!("crowd-{n}"))
                    .await
                    .unwrap();
            }
            module.commit_block().await.unwrap();
            let before = cost(&backing);
            module.write_conversation_wake(&ctx, "room").await.unwrap();
            let write = since(before, &backing);
            module.commit_block().await.unwrap();
            // The wake is already open: the duplicate check is a point read,
            // never a walk of everyone else's.
            let before = cost(&backing);
            module.write_conversation_wake(&ctx, "room").await.unwrap();
            let duplicate = since(before, &backing);
            module.commit_block().await.unwrap();
            let before = cost(&backing);
            let pending = module.conversation_deliveries(2).await.unwrap();
            let round = since(before, &backing);
            assert_eq!(pending.len(), 2, "the queue head is not being served");
            // Head of the queue either way, so the ack detaches from the same
            // place in both networks.
            let item = pending[0].item;
            let system = TestCtx::with_env(sdk::Env {
                height: 2,
                consensus_time: 2,
                origin: Origin::System,
                me: "runs".into(),
                cause: sdk::Cause::Direct,
            });
            let before = cost(&backing);
            assert!(
                module
                    .acknowledge_conversation(
                        &system,
                        &sdk::Ack {
                            item,
                            target: "runs".into(),
                            outcome: sdk::DeliveryOutcome::Applied,
                        },
                    )
                    .await
                    .unwrap()
            );
            counts.push([write, duplicate, round, since(before, &backing)]);
        }
        independent_of_n(counts[0], counts[1]);
    });
}

/// R7/R8: one insert and one crank round, against a network holding two
/// thousand pending timers.
#[test]
fn timer_inserts_and_cranks_read_past_every_other_pending_timer() {
    block_on(async {
        let mut counts = Vec::new();
        for crowd in [1, CROWD] {
            let (mut module, backing) = counted();
            module.write_conversation_state(&state()).unwrap();
            let ctx = program_ctx(100);
            for n in 0..crowd {
                module
                    .seed_conversation_timer(&format!("crowd-{n}"), "wait", 10_000 + n)
                    .await
                    .unwrap();
            }
            module.commit_block().await.unwrap();
            let before = cost(&backing);
            module
                .schedule_conversation_input(
                    &ctx,
                    "room".into(),
                    "schedule".into(),
                    "checkin".into(),
                    Some(10),
                    ConversationInput::Event {
                        kind: "checkin".into(),
                        content: serde_json::json!({}),
                    },
                )
                .await
                .unwrap();
            let insert = since(before, &backing);
            module.commit_block().await.unwrap();
            assert_eq!(
                module.next_conversation_input_due().await.unwrap(),
                Some(110)
            );
            let before = cost(&backing);
            module
                .crank_conversation_inputs(&program_ctx(110))
                .await
                .unwrap();
            let crank = since(before, &backing);
            assert_eq!(
                module
                    .require_conversation("room")
                    .await
                    .unwrap()
                    .admitted_cursor,
                1
            );
            counts.push([insert, crank]);
        }
        independent_of_n(counts[0], counts[1]);
    });
}

/// The whole-map wake queue a running network already holds is delivered as
/// it stands, carried over by the next write, and then gone.
#[test]
fn a_wake_queue_from_before_the_carry_over_is_delivered_then_converted() {
    block_on(async {
        let (mut module, _) = counted();
        let queued = BTreeMap::from([(4u64, "old-a".to_string()), (9, "old-b".to_string())]);
        for (item, id) in &queued {
            module
                .receipts
                .stage(
                    wake_key(*item),
                    sdk::wire::encode(&Wake {
                        conversation_id: id.clone(),
                        cause: sdk::Cause::Direct,
                        acknowledged: None,
                    }),
                )
                .unwrap();
        }
        module
            .receipts
            .stage(LEGACY_WAKE_QUEUE.into(), sdk::wire::encode(&queued))
            .unwrap();
        module.commit_block().await.unwrap();
        let served: Vec<_> = module
            .conversation_deliveries(8)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.item)
            .collect();
        assert_eq!(
            served,
            [4, 9],
            "a queue written before the carry-over stalls"
        );
        module.next_action_item = 12;
        module
            .write_conversation_wake(&program_ctx(1), "fresh")
            .await
            .unwrap();
        module.commit_block().await.unwrap();
        assert!(
            module
                .receipts
                .get(LEGACY_WAKE_QUEUE)
                .await
                .unwrap()
                .is_none(),
            "the old whole-map key outlived its carry-over"
        );
        let served: Vec<_> = module
            .conversation_deliveries(8)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.item)
            .collect();
        assert_eq!(served, [4, 9, 12], "the carry-over lost a queued wake");
    });
}

/// The same for the whole-Vec timer queue: it fires in its old order, and the
/// timers still pending outlive the conversion.
#[test]
fn a_timer_queue_from_before_the_carry_over_still_fires_in_order() {
    block_on(async {
        let (mut module, _) = counted();
        module.write_conversation_state(&state()).unwrap();
        let timers = [("room", "now", 110u64), ("room", "later", 200)];
        for (id, slot, due_at) in timers {
            module
                .receipts
                .stage(
                    schedule::schedule_key(id, slot),
                    sdk::wire::encode(&ConversationSchedule {
                        conversation_id: id.into(),
                        schedule_id: slot.into(),
                        operation_id: schedule::timer_operation_id(id, slot),
                        actor: Origin::Program(7),
                        input: ConversationInput::Event {
                            kind: slot.into(),
                            content: serde_json::json!({}),
                        },
                        status: ConversationScheduleStatus::Pending { due_at },
                    }),
                )
                .unwrap();
        }
        module
            .receipts
            .stage(
                schedule::schedule_index("room"),
                sdk::wire::encode(&["later", "now"]),
            )
            .unwrap();
        module
            .receipts
            .stage(
                schedule::LEGACY_SCHEDULE_QUEUE.into(),
                schedule::legacy_timer_queue(&timers),
            )
            .unwrap();
        module
            .receipts
            .stage(
                schedule::NEXT_SCHEDULE_DUE.into(),
                sdk::wire::encode(&Some(110u64)),
            )
            .unwrap();
        module.commit_block().await.unwrap();
        assert_eq!(
            module.next_conversation_input_due().await.unwrap(),
            Some(110)
        );
        module
            .crank_conversation_inputs(&program_ctx(110))
            .await
            .unwrap();
        module.commit_block().await.unwrap();
        assert!(
            module
                .receipts
                .get(schedule::LEGACY_SCHEDULE_QUEUE)
                .await
                .unwrap()
                .is_none(),
            "the old whole-queue key outlived its carry-over"
        );
        assert_eq!(
            module
                .require_conversation("room")
                .await
                .unwrap()
                .admitted_cursor,
            1
        );
        // The timer that was not due survived the conversion and is the head.
        assert_eq!(
            module.next_conversation_input_due().await.unwrap(),
            Some(200)
        );
        module
            .crank_conversation_inputs(&program_ctx(200))
            .await
            .unwrap();
        assert_eq!(
            module
                .require_conversation("room")
                .await
                .unwrap()
                .admitted_cursor,
            2
        );
        assert_eq!(module.next_conversation_input_due().await.unwrap(), None);
    });
}

/// What the old whole-record queues held at the 1 MiB store value bound they
/// were already up against — the largest legacy record a network can present.
const TIMER_CLIFF: u64 = 2600;
const WAKE_CLIFF: u64 = 3900;
/// The wasm host refuses an op past `MAX_STORE_READS` (4096) distinct store
/// reads. An op that carries entries over is also doing its own work, so the
/// carry-over is held well under it rather than merely inside it.
const READ_BUDGET: usize = 3000;

/// 2600 timers over 100 conversations, every one at its own due time, in the
/// (due, conversation, schedule) order the old queue kept.
fn cliff_timers(count: u64) -> Vec<(String, String, u64)> {
    let conversations = count.min(100);
    let mut timers: Vec<(String, String, u64)> = (0..count)
        .map(|n| {
            let conversation = n % conversations;
            let slot = n / conversations;
            (
                format!("room-{conversation}"),
                format!("timer-{slot:04}"),
                1_000 + n,
            )
        })
        .collect();
    timers.sort_by_key(|(_, _, due_at)| *due_at);
    timers
}

/// R7: the carry-over of a legacy queue at its cliff size stays under the
/// host's read ceiling, drains, and fires every timer it held exactly once in
/// its old order.
///
/// Carrying the whole record over cost one read per entry with no bound but
/// the record's own: past the ceiling the host refuses the op, and since every
/// write path carries over first, EVERY write — so the timers wedge with no
/// way out.
#[test]
fn a_timer_queue_at_its_cliff_carries_over_under_the_host_read_ceiling() {
    block_on(async {
        // What one op reads is the chunk it takes, not the record it takes it
        // from: a legacy queue twenty times longer costs under a chunk more,
        // and that residue is the trie descent over more distinct due times.
        // Carrying the whole record over instead cost one read per entry.
        let small = drain_timer_queue(CARRY_OVER_CHUNK as u64 + 8).await;
        let cliff = drain_timer_queue(TIMER_CLIFF).await;
        assert!(
            cliff <= small + CARRY_OVER_CHUNK,
            "the carry-over's worst op grew with the legacy record: {small} then {cliff}"
        );
        assert!(
            cliff < READ_BUDGET,
            "one op read {cliff} distinct records, past the {READ_BUDGET} budget"
        );
    });
}

/// Seeds `count` timers under the old whole-queue key, cranks until it has
/// drained and every one of them has fired, and answers the most distinct
/// records any single op read on the way.
async fn drain_timer_queue(count: u64) -> usize {
    let (mut module, backing) = counted();
    let timers = cliff_timers(count);
    for (id, slot, due_at) in &timers {
        module
            .write_conversation_state(&ConversationView {
                conversation_id: id.clone(),
                ..state()
            })
            .unwrap();
        module
            .receipts
            .stage(
                schedule::schedule_key(id, slot),
                sdk::wire::encode(&ConversationSchedule {
                    conversation_id: id.clone(),
                    schedule_id: slot.clone(),
                    operation_id: schedule::timer_operation_id(id, slot),
                    actor: Origin::Program(7),
                    input: ConversationInput::Event {
                        kind: slot.clone(),
                        content: serde_json::json!({}),
                    },
                    status: ConversationScheduleStatus::Pending { due_at: *due_at },
                }),
            )
            .unwrap();
    }
    for conversation in 0..count.min(100) {
        let id = format!("room-{conversation}");
        let mut slots: Vec<&str> = timers
            .iter()
            .filter(|(timer, _, _)| *timer == id)
            .map(|(_, slot, _)| slot.as_str())
            .collect();
        slots.sort_unstable();
        module
            .receipts
            .stage(schedule::schedule_index(&id), sdk::wire::encode(&slots))
            .unwrap();
    }
    let borrowed: Vec<(&str, &str, u64)> = timers
        .iter()
        .map(|(id, slot, due_at)| (id.as_str(), slot.as_str(), *due_at))
        .collect();
    module
        .receipts
        .stage(
            schedule::LEGACY_SCHEDULE_QUEUE.into(),
            schedule::legacy_timer_queue(&borrowed),
        )
        .unwrap();
    module
        .receipts
        .stage(
            schedule::NEXT_SCHEDULE_DUE.into(),
            sdk::wire::encode(&Some(timers[0].2)),
        )
        .unwrap();
    module.commit_block().await.unwrap();

    let last_due = timers.last().unwrap().2;
    let now = program_ctx(last_due + 1);
    let mut peak = 0;
    let mut earliest = 0;
    let mut ops = 0;
    while let Some(due_at) = module.next_conversation_input_due().await.unwrap() {
        assert!(
            due_at >= earliest,
            "the crank went backwards: {due_at} after {earliest}"
        );
        earliest = due_at;
        backing.forget_distinct();
        module.crank_conversation_inputs(&now).await.unwrap();
        peak = peak.max(backing.distinct_reads());
        module.commit_block().await.unwrap();
        ops += 1;
        assert!(ops < 4 * count, "the carry-over stopped draining");
    }
    assert!(
        module
            .receipts
            .get(schedule::LEGACY_SCHEDULE_QUEUE)
            .await
            .unwrap()
            .is_none(),
        "the old whole-queue key outlived its carry-over"
    );
    // Every timer fired, once, and within each conversation in due order —
    // which, since the crank only ever moved forward above, is the old
    // queue's order.
    let mut fired = 0;
    for conversation in 0..count.min(100) {
        let schedules = module
            .conversation_schedules(&format!("room-{conversation}"))
            .await
            .unwrap();
        let sequences: Vec<u64> = schedules
            .iter()
            .map(|schedule| match schedule.status {
                ConversationScheduleStatus::Fired { sequence } => sequence,
                ref other => panic!("{} did not fire: {other:?}", schedule.schedule_id),
            })
            .collect();
        let mut ordered = sequences.clone();
        ordered.sort_unstable();
        ordered.dedup();
        assert_eq!(sequences, ordered, "timers fired out of their due order");
        fired += sequences.len() as u64;
    }
    assert_eq!(fired, count, "the carry-over lost a timer");
    peak
}

/// The same for the wake map: a cliff-size legacy queue carries over under the
/// ceiling, drains, and every wake it held is delivered exactly once, oldest
/// item first.
#[test]
fn a_wake_queue_at_its_cliff_carries_over_under_the_host_read_ceiling() {
    block_on(async {
        let small = drain_wake_queue(CARRY_OVER_CHUNK as u64 + 8).await;
        let cliff = drain_wake_queue(WAKE_CLIFF).await;
        assert!(
            cliff <= small + CARRY_OVER_CHUNK,
            "the carry-over's worst op grew with the legacy record: {small} then {cliff}"
        );
        assert!(
            cliff < READ_BUDGET,
            "one op read {cliff} distinct records, past the {READ_BUDGET} budget"
        );
    });
}

/// Seeds `count` wakes under the old whole-map key, carries it over until it
/// has drained, and answers the most distinct records any single op read —
/// having checked on the way out that every wake is still deliverable, once
/// and in its old order.
async fn drain_wake_queue(count: u64) -> usize {
    let (mut module, backing) = counted();
    let legacy: BTreeMap<u64, String> = (0..count)
        .map(|item| (item, format!("room-{item}")))
        .collect();
    for (item, id) in &legacy {
        module
            .write_conversation_state(&ConversationView {
                conversation_id: id.clone(),
                ..state()
            })
            .unwrap();
        module
            .receipts
            .stage(
                wake_key(*item),
                sdk::wire::encode(&Wake {
                    conversation_id: id.clone(),
                    cause: sdk::Cause::Direct,
                    acknowledged: None,
                }),
            )
            .unwrap();
    }
    module
        .receipts
        .stage(LEGACY_WAKE_QUEUE.into(), sdk::wire::encode(&legacy))
        .unwrap();
    module.commit_block().await.unwrap();

    let ctx = program_ctx(1);
    let mut peak = 0;
    let mut ops = 0;
    // Acked while still sitting in the unconverted remainder: the
    // carry-over must not queue it again. Counted like every other op —
    // it carries a chunk over before it looks for the item.
    let acked = count - 1;
    backing.forget_distinct();
    module
        .detach_conversation_wake(acked, &format!("room-{acked}"))
        .await
        .unwrap();
    peak = peak.max(backing.distinct_reads());
    module.commit_block().await.unwrap();
    while module
        .legacy_wake_queue(View::Live)
        .await
        .unwrap()
        .is_some()
    {
        backing.forget_distinct();
        // `room-0` is in the queue already, so this is a carry-over and
        // the duplicate check, and it queues nothing of its own.
        module
            .write_conversation_wake(&ctx, "room-0")
            .await
            .unwrap();
        peak = peak.max(backing.distinct_reads());
        module.commit_block().await.unwrap();
        ops += 1;
        assert!(ops < count, "the carry-over stopped draining");
    }
    let delivered: Vec<u64> = module
        .conversation_deliveries(count as usize + 8)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.item)
        .collect();
    let expected: Vec<u64> = (0..acked).collect();
    assert_eq!(
        delivered, expected,
        "the carry-over lost, duplicated or reordered a wake"
    );
    peak
}

/// Gap 2, pinned: timers sharing a due time fire newest-inserted first, since
/// each insert pushes onto that due time's list head. The old whole-Vec queue
/// fired them in (conversation, schedule) order instead. Deterministic either
/// way — this is here so a change to it is a deliberate one.
#[test]
fn timers_sharing_a_due_time_fire_newest_first() {
    block_on(async {
        let (mut module, _) = counted();
        module.write_conversation_state(&state()).unwrap();
        for slot in ["a", "b", "c"] {
            module
                .receipts
                .stage(
                    schedule::schedule_key("room", slot),
                    sdk::wire::encode(&ConversationSchedule {
                        conversation_id: "room".into(),
                        schedule_id: slot.into(),
                        operation_id: schedule::timer_operation_id("room", slot),
                        actor: Origin::Program(7),
                        input: ConversationInput::Event {
                            kind: slot.into(),
                            content: serde_json::json!({}),
                        },
                        status: ConversationScheduleStatus::Pending { due_at: 500 },
                    }),
                )
                .unwrap();
            module
                .seed_conversation_timer("room", slot, 500)
                .await
                .unwrap();
        }
        module
            .receipts
            .stage(
                schedule::schedule_index("room"),
                sdk::wire::encode(&["a", "b", "c"]),
            )
            .unwrap();
        module.commit_block().await.unwrap();
        module
            .crank_conversation_inputs(&program_ctx(500))
            .await
            .unwrap();
        module.commit_block().await.unwrap();
        let fired: Vec<(String, u64)> = module
            .conversation_schedules("room")
            .await
            .unwrap()
            .into_iter()
            .map(|schedule| match schedule.status {
                ConversationScheduleStatus::Fired { sequence } => (schedule.schedule_id, sequence),
                ref other => panic!("{} did not fire: {other:?}", schedule.schedule_id),
            })
            .collect();
        assert_eq!(
            fired,
            [
                ("a".to_string(), 3),
                ("b".to_string(), 2),
                ("c".to_string(), 1)
            ],
            "same-due timers no longer fire newest-inserted first"
        );
    });
}
