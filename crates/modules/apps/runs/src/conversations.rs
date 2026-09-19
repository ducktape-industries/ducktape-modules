//! Durable intake, turn ownership, and native history. Effects are written only by
//! the executor below; source hooks never run a model in the source write cascade.
use super::*;
use crate::receipts::View;
use sdk::refusal;
use serde::de::DeserializeOwned;
#[path = "conversation_runtime.rs"]
mod runtime;
#[path = "conversation_schedule.rs"]
mod schedule;
#[path = "conversation_validation.rs"]
mod validation;
pub(super) use validation::validate_records;
#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;

fn key(kind: &str, id: &str) -> String {
    format!("conversation/{kind}/{}", dispatch_id_for(id))
}
fn numbered(kind: &str, id: &str, n: u64) -> String {
    format!("{}/{n}", key(kind, id))
}
fn op_key(id: &str, op: &str) -> String {
    format!("{}/{}", key("operation", id), dispatch_id_for(op))
}
/// The pre-point-address whole-map queue. Carried over a chunk at a time by
/// the writes that touch the new layout, and deleted once it is drained.
const LEGACY_WAKE_QUEUE: &str = "conversation/wakes";
/// How many pre-point-address entries one op carries over, for both the wake
/// map and the timer queue.
///
/// Carrying one entry costs about one store read — the record it is about to
/// claim, absent until this op stages it — and the wasm host refuses an op
/// past `MAX_STORE_READS` (4096) distinct reads. A legacy record holds
/// whatever fits under the 1 MiB store value bound, thousands of entries, so
/// carrying the whole of one was an op the host could refuse EVERY time:
/// timers and wakes wedged for good, with no write able to unwedge them. A
/// chunk leaves the budget all but untouched and drains in
/// `entries / CARRY_OVER_CHUNK` writes.
pub(super) const CARRY_OVER_CHUNK: usize = 128;
/// head/tail of the wake FIFO — two `Option<u64>`, fixed width forever.
const WAKE_QUEUE: &str = "conversation/wake_queue";
fn wake_key(item: u64) -> String {
    format!("conversation/wake/{item}")
}
/// The queue links of one queued item. Written when the item joins the FIFO
/// and deleted when it leaves, so the record exists only while it is queued —
/// unlike `wake_key`, which outlives the queue to answer a repeated ack.
fn wake_link_key(item: u64) -> String {
    format!("conversation/wake_link/{item}")
}
/// The open wake of one conversation. Its presence IS the duplicate check.
fn wake_of_key(id: &str) -> String {
    format!("conversation/wake_of/{}", dispatch_id_for(id))
}

fn require_coordinating_source(state: &ConversationView) -> Result<(), Error> {
    let job_backed = matches!(state.source, ConversationSource::Job { .. });
    if job_backed {
        return Err(Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence: "job execution inputs require canonical Tasks operations".into(),
        });
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wake {
    conversation_id: String,
    cause: sdk::Cause,
    acknowledged: Option<[u8; 32]>,
}

/// The FIFO ends. Both are `None` exactly when no wake is queued.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WakeQueue {
    head: Option<u64>,
    tail: Option<u64>,
}

/// One queued item's neighbours. Doubly linked because a wake is detached
/// where it is delivered, not where it sits: `begin_conversation_wake` and a
/// late ack both unlink from the middle, and neither may walk the queue.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WakeLink {
    prev: Option<u64>,
    next: Option<u64>,
}

/// Every state-machine input is visible here. Authentication and sibling reads
/// happen before this pure decision; ordered writers execute its commands.
enum Input {
    Activate(bool),
    Append(ConversationEvent),
    SourceCursor(u64),
    Pause(String),
    Queue,
    Requested,
    Started,
    JobStarted {
        run_id: String,
        event: ConversationEvent,
    },
    Checkpoint(ConversationCheckpoint),
    Action(String),
    ModelEnded(RunOutcome),
    ActionsDrained(u64),
    Drained(Option<u32>),
    Retry,
}
enum Command {
    State(Box<ConversationView>),
    Event(ConversationEvent),
    Turn(ConversationTurn),
    Wake,
}
fn step(state: &ConversationView, input: Input) -> Result<Vec<Command>, Error> {
    match input {
        Input::Activate(active) => activate(state, active),
        Input::Append(event) => append(state, event),
        Input::SourceCursor(cursor) => source_cursor(state, cursor),
        Input::Pause(reason) => pause(state, reason),
        Input::Queue => queue(state),
        Input::Requested => requested(state),
        Input::Started => started(state),
        Input::JobStarted { run_id, event } => job_started(state, run_id, event),
        Input::Checkpoint(value) => checkpoint(state, value),
        Input::Action(id) => action(state, id),
        Input::ModelEnded(outcome) => model_ended(state, outcome),
        Input::ActionsDrained(count) => actions_drained(state, count),
        Input::Drained(attempt) => drained(state, attempt),
        Input::Retry => retry(state),
    }
}
fn activate(state: &ConversationView, active: bool) -> Result<Vec<Command>, Error> {
    let mut next = state.clone();
    next.status = if active {
        ConversationStatus::Active
    } else {
        ConversationStatus::Inactive
    };
    Ok(vec![Command::State(Box::new(next)), Command::Wake])
}
fn append(state: &ConversationView, event: ConversationEvent) -> Result<Vec<Command>, Error> {
    let expected = state
        .admitted_cursor
        .checked_add(1)
        .ok_or_else(|| Error::Module {
            reason: refusal::EXHAUSTED.into(),
            sentence: format!(
                "conversation {} has no event sequence numbers left",
                state.conversation_id
            ),
        })?;
    if event.sequence != expected {
        return Err(Error::Module {
            reason: refusal::STALE.into(),
            sentence: format!(
                "event {} is not the next event {expected} of conversation {}",
                event.sequence, state.conversation_id
            ),
        });
    }
    let mut next = state.clone();
    next.admitted_cursor = expected;
    Ok(vec![
        Command::Wake,
        Command::Event(event),
        Command::State(Box::new(next)),
    ])
}
fn source_cursor(state: &ConversationView, cursor: u64) -> Result<Vec<Command>, Error> {
    let mut next = state.clone();
    next.source_cursor = next.source_cursor.max(cursor);
    Ok(vec![Command::State(Box::new(next))])
}
fn pause(state: &ConversationView, reason: String) -> Result<Vec<Command>, Error> {
    let mut next = state.clone();
    next.status = ConversationStatus::Paused { reason };
    Ok(vec![Command::State(Box::new(next))])
}
fn queue(state: &ConversationView) -> Result<Vec<Command>, Error> {
    let eligible = state.status == ConversationStatus::Active
        && state.active_turn.is_none()
        && state.completed_cursor < state.admitted_cursor;
    if !eligible {
        return Ok(Vec::new());
    }
    let mut next = state.clone();
    // One event per turn means arbitrarily long backlogs never become a bounded
    // transcript pretending to be history. Every turn restores the native tree.
    let through_cursor = state
        .completed_cursor
        .checked_add(1)
        .ok_or_else(|| Error::Module {
            reason: refusal::EXHAUSTED.into(),
            sentence: format!(
                "conversation {} has no event sequence numbers left",
                state.conversation_id
            ),
        })?;
    let turn = ConversationTurn {
        turn: state.next_turn,
        run_id: format!(
            "conversation/{}/{}",
            dispatch_id_for(&state.conversation_id),
            state.next_turn
        ),
        from_cursor: state.completed_cursor,
        through_cursor,
        phase: ConversationTurnPhase::Queued,
        checkpoint: None,
        actions: Vec::new(),
        drained_actions: 0,
        outcome: None,
    };
    next.next_turn = next.next_turn.checked_add(1).ok_or_else(|| Error::Module {
        reason: refusal::EXHAUSTED.into(),
        sentence: format!(
            "conversation {} has no turn numbers left",
            state.conversation_id
        ),
    })?;
    next.active_turn = Some(turn.clone());
    Ok(vec![Command::Turn(turn), Command::State(Box::new(next))])
}
fn update_turn(state: &ConversationView, turn: ConversationTurn) -> Vec<Command> {
    let mut next = state.clone();
    next.active_turn = Some(turn.clone());
    vec![Command::Turn(turn), Command::State(Box::new(next))]
}
fn active_turn(state: &ConversationView) -> Result<ConversationTurn, Error> {
    state.active_turn.clone().ok_or_else(|| Error::Module {
        reason: refusal::WRONG_STATE.into(),
        sentence: "conversation has no active turn".into(),
    })
}
fn requested(state: &ConversationView) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    if turn.phase != ConversationTurnPhase::Queued {
        return Ok(Vec::new());
    }
    turn.phase = ConversationTurnPhase::AwaitingProgram;
    Ok(update_turn(state, turn))
}
fn started(state: &ConversationView) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    if turn.phase != ConversationTurnPhase::AwaitingProgram {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: "conversation turn was not requested".into(),
        });
    }
    turn.phase = ConversationTurnPhase::Running;
    Ok(update_turn(state, turn))
}
fn job_started(
    state: &ConversationView,
    run_id: String,
    event: ConversationEvent,
) -> Result<Vec<Command>, Error> {
    if state.active_turn.is_some() {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: format!(
                "worker conversation {} is still running a turn",
                state.conversation_id
            ),
        });
    }
    let mut next = state.clone();
    next.admitted_cursor = state
        .admitted_cursor
        .checked_add(1)
        .ok_or_else(|| Error::Module {
            reason: refusal::EXHAUSTED.into(),
            sentence: format!(
                "conversation {} has no input sequence numbers left",
                state.conversation_id
            ),
        })?;
    if event.sequence != next.admitted_cursor {
        return Err(Error::Module {
            reason: refusal::STALE.into(),
            sentence: format!(
                "event {} is not the next input {} of conversation {}",
                event.sequence, next.admitted_cursor, state.conversation_id
            ),
        });
    }
    let turn = ConversationTurn {
        turn: next.next_turn,
        run_id,
        from_cursor: state.completed_cursor,
        through_cursor: next.admitted_cursor,
        phase: ConversationTurnPhase::Running,
        checkpoint: None,
        actions: Vec::new(),
        drained_actions: 0,
        outcome: None,
    };
    next.next_turn = next.next_turn.checked_add(1).ok_or_else(|| Error::Module {
        reason: refusal::EXHAUSTED.into(),
        sentence: format!(
            "conversation {} has no turn numbers left",
            state.conversation_id
        ),
    })?;
    next.active_turn = Some(turn.clone());
    Ok(vec![
        Command::Event(event),
        Command::Turn(turn),
        Command::State(Box::new(next)),
    ])
}
fn checkpoint(
    state: &ConversationView,
    mut checkpoint: ConversationCheckpoint,
) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    let current = turn.phase == ConversationTurnPhase::Running && turn.run_id == checkpoint.run_id;
    if !current {
        return Err(Error::Module {
            reason: refusal::STALE.into(),
            sentence: "checkpoint is not for the active execution".into(),
        });
    }
    let prior_revision = turn
        .checkpoint
        .as_ref()
        .map(|c| c.history.revision)
        .or_else(|| state.history.as_ref().map(|h| h.revision))
        .unwrap_or(0);
    let next_revision = prior_revision.checked_add(1).ok_or_else(|| Error::Module {
        reason: refusal::EXHAUSTED.into(),
        sentence: format!(
            "conversation {} has no history revision numbers left",
            state.conversation_id
        ),
    })?;
    if checkpoint.history.revision != next_revision {
        return Err(Error::Module {
            reason: refusal::STALE.into(),
            sentence: format!(
                "checkpoint revision {} is not the next history revision {next_revision}",
                checkpoint.history.revision
            ),
        });
    }
    checkpoint.delivery |= turn
        .checkpoint
        .as_ref()
        .is_some_and(|previous| previous.delivery);
    turn.checkpoint = Some(checkpoint);
    Ok(update_turn(state, turn))
}
fn action(state: &ConversationView, id: String) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    if turn.actions.contains(&id) {
        return Ok(Vec::new());
    }
    let accepts_actions = matches!(
        turn.phase,
        ConversationTurnPhase::Running | ConversationTurnPhase::Draining
    );
    if !accepts_actions {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: format!(
                "turn {} of conversation {} accepts no more actions",
                turn.turn, state.conversation_id
            ),
        });
    }
    turn.actions.push(id);
    Ok(update_turn(state, turn))
}
fn model_ended(state: &ConversationView, outcome: RunOutcome) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    if turn.phase == ConversationTurnPhase::Draining {
        return Ok(Vec::new());
    }
    if turn.phase != ConversationTurnPhase::Running {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: "conversation completion is not for a running turn".into(),
        });
    }
    turn.phase = ConversationTurnPhase::Draining;
    turn.outcome = Some(outcome);
    let mut commands = update_turn(state, turn);
    commands.push(Command::Wake);
    Ok(commands)
}
fn actions_drained(state: &ConversationView, count: u64) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    let valid = turn.phase == ConversationTurnPhase::Draining
        && count >= turn.drained_actions
        && count <= turn.actions.len() as u64;
    if !valid {
        return Err(Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence: format!(
                "conversation {} cannot mark {count} of its turn's actions drained",
                state.conversation_id
            ),
        });
    }
    turn.drained_actions = count;
    Ok(update_turn(state, turn))
}
fn drained(state: &ConversationView, attempt: Option<u32>) -> Result<Vec<Command>, Error> {
    let mut turn = active_turn(state)?;
    let ready = turn.phase == ConversationTurnPhase::Draining
        && turn.drained_actions == turn.actions.len() as u64;
    if !ready {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: format!(
                "conversation {} still has actions to drain",
                state.conversation_id
            ),
        });
    }
    let Some(checkpoint) = &turn.checkpoint else {
        return pause(state, "history_checkpoint_missing".into());
    };
    if Some(checkpoint.attempt) != attempt {
        return pause(state, "history_checkpoint_stale_attempt".into());
    }
    let input_accounted = checkpoint.delivery || turn.outcome == Some(RunOutcome::Cancelled);
    if !input_accounted {
        return pause(state, "native_input_not_delivered".into());
    }
    let mut next = state.clone();
    next.completed_cursor = turn.through_cursor;
    next.history = Some(checkpoint.history.clone());
    turn.phase = ConversationTurnPhase::Settled;
    next.active_turn = None;
    Ok(vec![
        Command::Turn(turn),
        Command::State(Box::new(next)),
        Command::Wake,
    ])
}

fn retry(state: &ConversationView) -> Result<Vec<Command>, Error> {
    let mut previous = active_turn(state)?;
    let safe_boundary = previous.phase == ConversationTurnPhase::Draining
        && previous.drained_actions == previous.actions.len() as u64;
    if !safe_boundary {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: "conversation retry requires drained execution effects".into(),
        });
    }
    let mut next = state.clone();
    next.status = ConversationStatus::Active;
    next.history = previous
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.history.clone())
        .or_else(|| state.history.clone());
    previous.phase = ConversationTurnPhase::Settled;
    let commands = match state.source {
        ConversationSource::Job { .. } => {
            next.completed_cursor = previous.through_cursor;
            next.active_turn = None;
            vec![
                Command::Turn(previous),
                Command::State(Box::new(next)),
                Command::Wake,
            ]
        }
        ConversationSource::Channel { .. } | ConversationSource::Detached => {
            let turn = ConversationTurn {
                turn: next.next_turn,
                run_id: format!(
                    "conversation/{}/{}",
                    dispatch_id_for(&state.conversation_id),
                    next.next_turn
                ),
                from_cursor: previous.from_cursor,
                through_cursor: previous.through_cursor,
                phase: ConversationTurnPhase::Queued,
                checkpoint: None,
                actions: Vec::new(),
                drained_actions: 0,
                outcome: None,
            };
            next.next_turn = next.next_turn.checked_add(1).ok_or_else(|| Error::Module {
                reason: refusal::EXHAUSTED.into(),
                sentence: format!(
                    "conversation {} has no turn numbers left",
                    state.conversation_id
                ),
            })?;
            next.active_turn = Some(turn.clone());
            vec![
                Command::Turn(previous),
                Command::Turn(turn),
                Command::State(Box::new(next)),
                Command::Wake,
            ]
        }
    };
    Ok(commands)
}

impl RunsModule {
    async fn conversation_read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error> {
        self.receipts
            .get(key)
            .await?
            .map(|b| {
                sdk::wire::decode(&b).map_err(|sentence| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence,
                })
            })
            .transpose()
    }
    /// The same point read against the previous block boundary. `pending_items`
    /// runs before the block's writes are committed and must not see them.
    async fn committed_record<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, Error> {
        self.receipts
            .committed(key)
            .await?
            .map(|b| {
                sdk::wire::decode(&b).map_err(|sentence| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence,
                })
            })
            .transpose()
    }
    pub(super) async fn conversation(&self, id: &str) -> Result<Option<ConversationView>, Error> {
        self.conversation_read(&key("state", id)).await
    }
    pub(super) async fn conversation_for_account(
        &self,
        account: u64,
    ) -> Result<Option<ConversationView>, Error> {
        let Some(id) = self
            .conversation_read::<String>(&key("account", &account.to_string()))
            .await?
        else {
            return Ok(None);
        };
        self.conversation(&id).await
    }
    pub(super) async fn conversation_for_channel(
        &self,
        channel: &str,
    ) -> Result<Option<ConversationView>, Error> {
        let Some(id) = self
            .conversation_read::<String>(&key("channel", channel))
            .await?
        else {
            return Ok(None);
        };
        self.conversation(&id).await
    }
    pub(super) async fn conversation_turn(
        &self,
        id: &str,
        turn: u64,
    ) -> Result<Option<ConversationTurn>, Error> {
        self.conversation_read(&numbered("turn", id, turn)).await
    }
    pub(super) async fn conversation_events(
        &self,
        id: &str,
        from: u64,
        limit: u64,
    ) -> Result<Vec<ConversationEvent>, Error> {
        let Some(state) = self.conversation(id).await? else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        let end = from
            .saturating_add(limit.min(64))
            .min(state.admitted_cursor.saturating_add(1));
        for n in from.max(1)..end {
            let event = self
                .conversation_read(&numbered("event", id, n))
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!("conversation {id} has no event {n}"),
                })?;
            events.push(event);
        }
        Ok(events)
    }
    fn write_conversation_state(&mut self, state: &ConversationView) -> Result<(), Error> {
        self.receipts.stage(
            key("state", &state.conversation_id),
            sdk::wire::encode(state),
        )
    }
    fn write_conversation_event(
        &mut self,
        id: &str,
        event: &ConversationEvent,
    ) -> Result<(), Error> {
        self.receipts.stage(
            numbered("event", id, event.sequence),
            sdk::wire::encode(event),
        )
    }
    fn write_conversation_turn(&mut self, id: &str, turn: &ConversationTurn) -> Result<(), Error> {
        self.receipts
            .stage(numbered("turn", id, turn.turn), sdk::wire::encode(turn))?;
        self.receipts
            .stage(key("run", &turn.run_id), sdk::wire::encode(&id))
    }
    /// What the whole-map queue still holds, absent once it has drained.
    pub(super) async fn legacy_wake_queue(
        &self,
        view: View,
    ) -> Result<Option<BTreeMap<u64, String>>, Error> {
        let Some(bytes) = self.receipts.read(LEGACY_WAKE_QUEUE, view).await? else {
            return Ok(None);
        };
        sdk::wire::decode(&bytes)
            .map(Some)
            .map_err(|sentence| Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence,
            })
    }
    /// Carry [`CARRY_OVER_CHUNK`] of the whole-map queue into the linked
    /// layout, oldest item first, and leave the rest under the old key for the
    /// next write. The FIFO appends, so carrying the OLDEST items first is
    /// what keeps the map's delivery order: what is left is always younger
    /// than what is linked, and `conversation_deliveries` serves it last.
    async fn carry_over_wake_queue(&mut self) -> Result<(), Error> {
        let Some(mut legacy) = self.legacy_wake_queue(View::Live).await? else {
            return Ok(());
        };
        let remainder = match legacy.keys().nth(CARRY_OVER_CHUNK).copied() {
            Some(first_left) => legacy.split_off(&first_left),
            None => BTreeMap::new(),
        };
        let mut queue: WakeQueue = self
            .conversation_read(WAKE_QUEUE)
            .await?
            .unwrap_or_default();
        // A conversation holds one open wake. While the old key stands a write
        // can open one for a conversation still sitting in it, so this read is
        // not just the map's own duplicates: the slot is checked per entry and
        // the one already linked keeps it.
        for (item, id) in legacy {
            let already = self
                .conversation_read::<u64>(&wake_of_key(&id))
                .await?
                .is_some();
            if already {
                continue;
            }
            self.link_wake(&mut queue, item, &id).await?;
        }
        match remainder.is_empty() {
            true => self.receipts.remove(LEGACY_WAKE_QUEUE.into()),
            false => self
                .receipts
                .stage(LEGACY_WAKE_QUEUE.into(), sdk::wire::encode(&remainder))?,
        }
        self.receipts
            .stage(WAKE_QUEUE.into(), sdk::wire::encode(&queue))
    }
    /// Forget one item the old map still holds. An ack can land on an item
    /// whose chunk has not been carried over yet, and dropping it from the
    /// linked layout alone would leave the carry-over to re-queue it.
    async fn drop_legacy_wake(&mut self, item: u64) -> Result<(), Error> {
        let Some(mut legacy) = self.legacy_wake_queue(View::Live).await? else {
            return Ok(());
        };
        if legacy.remove(&item).is_none() {
            return Ok(());
        }
        match legacy.is_empty() {
            true => self.receipts.remove(LEGACY_WAKE_QUEUE.into()),
            false => self
                .receipts
                .stage(LEGACY_WAKE_QUEUE.into(), sdk::wire::encode(&legacy))?,
        }
        Ok(())
    }
    /// Append `item` to the FIFO tail and claim the conversation's open slot.
    async fn link_wake(&mut self, queue: &mut WakeQueue, item: u64, id: &str) -> Result<(), Error> {
        let link = WakeLink {
            prev: queue.tail,
            next: None,
        };
        if let Some(tail) = queue.tail {
            let mut previous: WakeLink = self
                .conversation_read(&wake_link_key(tail))
                .await?
                .unwrap_or_default();
            previous.next = Some(item);
            self.receipts
                .stage(wake_link_key(tail), sdk::wire::encode(&previous))?;
        } else {
            queue.head = Some(item);
        }
        queue.tail = Some(item);
        self.receipts
            .stage(wake_link_key(item), sdk::wire::encode(&link))?;
        self.receipts
            .stage(wake_of_key(id), sdk::wire::encode(&item))
    }
    async fn write_conversation_wake(&mut self, ctx: &dyn Ctx, id: &str) -> Result<(), Error> {
        self.carry_over_wake_queue().await?;
        let already_queued = self
            .conversation_read::<u64>(&wake_of_key(id))
            .await?
            .is_some();
        if already_queued {
            return Ok(());
        }
        let item = self
            .staged_next_action_item
            .unwrap_or(self.next_action_item);
        let next = item.checked_add(1).ok_or_else(|| Error::Module {
            reason: refusal::EXHAUSTED.into(),
            sentence: "no action item numbers are left for a conversation wake".into(),
        })?;
        let wake = Wake {
            conversation_id: id.into(),
            cause: ctx.env().cause.clone(),
            acknowledged: None,
        };
        self.receipts
            .stage(wake_key(item), sdk::wire::encode(&wake))?;
        let mut queue: WakeQueue = self
            .conversation_read(WAKE_QUEUE)
            .await?
            .unwrap_or_default();
        self.link_wake(&mut queue, item, id).await?;
        self.receipts
            .stage(WAKE_QUEUE.into(), sdk::wire::encode(&queue))?;
        self.staged_next_action_item = Some(next);
        Ok(())
    }
    /// Unlink one item wherever it sits. Reads its two neighbours and the
    /// ends, never the queue between them.
    async fn detach_conversation_wake(&mut self, item: u64, id: &str) -> Result<(), Error> {
        self.carry_over_wake_queue().await?;
        let Some(link) = self
            .conversation_read::<WakeLink>(&wake_link_key(item))
            .await?
        else {
            // Either already detached, or acked before its chunk was carried
            // over — in which case the old map is still holding it.
            return self.drop_legacy_wake(item).await;
        };
        let mut queue: WakeQueue = self
            .conversation_read(WAKE_QUEUE)
            .await?
            .unwrap_or_default();
        match link.prev {
            Some(prev) => {
                let mut previous: WakeLink = self
                    .conversation_read(&wake_link_key(prev))
                    .await?
                    .unwrap_or_default();
                previous.next = link.next;
                self.receipts
                    .stage(wake_link_key(prev), sdk::wire::encode(&previous))?;
            }
            None => queue.head = link.next,
        }
        match link.next {
            Some(next) => {
                let mut following: WakeLink = self
                    .conversation_read(&wake_link_key(next))
                    .await?
                    .unwrap_or_default();
                following.prev = link.prev;
                self.receipts
                    .stage(wake_link_key(next), sdk::wire::encode(&following))?;
            }
            None => queue.tail = link.prev,
        }
        self.receipts.remove(wake_link_key(item));
        self.receipts.remove(wake_of_key(id));
        self.receipts
            .stage(WAKE_QUEUE.into(), sdk::wire::encode(&queue))
    }
    async fn apply_conversation(
        &mut self,
        ctx: &dyn Ctx,
        state: &ConversationView,
        input: Input,
    ) -> Result<(), Error> {
        let commands = step(state, input)?;
        let fits = commands.iter().all(|command| match command {
            Command::State(value) => sdk::wire::encode(value).len() <= sdk::MAX_STORE_VALUE_BYTES,
            Command::Event(value) => sdk::wire::encode(value).len() <= sdk::MAX_STORE_VALUE_BYTES,
            Command::Turn(value) => sdk::wire::encode(value).len() <= sdk::MAX_STORE_VALUE_BYTES,
            Command::Wake => true,
        });
        if !fits {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: "conversation record exceeds the store bound".into(),
            });
        }
        for command in commands {
            match command {
                Command::State(next) => self.write_conversation_state(&next)?,
                Command::Event(event) => {
                    self.write_conversation_event(&state.conversation_id, &event)?
                }
                Command::Turn(turn) => {
                    self.write_conversation_turn(&state.conversation_id, &turn)?
                }
                Command::Wake => {
                    self.write_conversation_wake(ctx, &state.conversation_id)
                        .await?
                }
            }
        }
        Ok(())
    }
    async fn require_conversation(&self, id: &str) -> Result<ConversationView, Error> {
        self.conversation(id).await?.ok_or_else(|| Error::Module {
            reason: refusal::NOT_FOUND.into(),
            sentence: format!("no conversation {id}"),
        })
    }
    async fn conversation_controller(
        &self,
        ctx: &dyn Ctx,
        state: &ConversationView,
    ) -> Result<(), Error> {
        if ctx.env().origin == Origin::Program(state.account) {
            return Ok(());
        }
        let controller = match self.account_control(ctx, state.account).await? {
            identity::Control::Program { controller, .. }
            | identity::Control::Revoked { controller } => controller,
            identity::Control::Keys => {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: "conversation requires a program account".into(),
                });
            }
        };
        let actor = match &ctx.env().origin {
            Origin::Program(account) => *account,
            Origin::External(signing_key) => {
                let bytes = ctx
                    .query(
                        "identity",
                        &identity::encode_query(&identity::IdentityQuery::OfKey {
                            key: signing_key.clone(),
                        }),
                    )
                    .await?;
                let identity::IdentityReply::Account(Some(account)) =
                    identity::decode_reply(&bytes).map_err(|sentence| Error::Module {
                        reason: refusal::UNEXPECTED_REPLY.into(),
                        sentence,
                    })?
                else {
                    return Err(Error::Module {
                        reason: refusal::NOT_FOUND.into(),
                        sentence: "conversation controller signer has no account".into(),
                    });
                };
                account.number
            }
            Origin::Module(_) | Origin::System => {
                return Err(Error::Module {
                    reason: refusal::UNAUTHORIZED.into(),
                    sentence: "conversation control requires an account".into(),
                });
            }
        };
        if actor != controller {
            return Err(Error::Module {
                reason: refusal::UNAUTHORIZED.into(),
                sentence: "conversation control requires its current controller".into(),
            });
        }
        Ok(())
    }
    async fn operation_seen(&self, id: &str, op: &str, payload: &[u8]) -> Result<bool, Error> {
        let valid = !op.is_empty() && op.len() <= MAX_REQUEST_ID_BYTES;
        if !valid {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!("an operation id is 1 to {MAX_REQUEST_ID_BYTES} bytes"),
            });
        }
        let Some(previous) = self.receipts.get(&op_key(id, op)).await? else {
            return Ok(false);
        };
        if previous != payload {
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: format!(
                    "operation {op} of conversation {id} already names different work"
                ),
            });
        }
        Ok(true)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn configure_conversation(
        &mut self,
        ctx: &mut dyn Ctx,
        id: String,
        agent_id: String,
        source: ConversationSource,
        history_prefix: String,
        session_path: String,
        packages: Vec<run_envelope::ConversationPackage>,
    ) -> Result<(), Error> {
        let worker_owned =
            id.starts_with("job/") || matches!(source, ConversationSource::Job { .. });
        if worker_owned {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: "job conversations are created by canonical Tasks intake".into(),
            });
        }
        let valid_id = !id.is_empty() && id.len() <= 256;
        if !valid_id {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: "a conversation id is 1 to 256 bytes".into(),
            });
        }
        let model = self
            .model(&agent_id)
            .cloned()
            .ok_or_else(|| Error::Module {
                reason: refusal::NOT_FOUND.into(),
                sentence: format!("no model {agent_id}"),
            })?;
        run_envelope::NativeConversation {
            conversation_id: id.clone(),
            turn_id: crate::conversation_turn_id(0, 0),
            revision: 0,
            history_prefix: history_prefix.clone(),
            history_snapshot: None,
            session_path: session_path.clone(),
            packages: packages.clone(),
            events: Vec::new(),
        }
        .validate()
        .map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        let state = ConversationView {
            conversation_id: id.clone(),
            agent_id,
            account: model.account,
            source: source.clone(),
            history_prefix,
            session_path,
            packages,
            status: ConversationStatus::Inactive,
            source_cursor: 0,
            admitted_cursor: 0,
            completed_cursor: 0,
            next_turn: 1,
            active_turn: None,
            history: None,
        };
        self.conversation_controller(ctx, &state).await?;
        if let Some(existing) = self.conversation(&id).await? {
            let exact = existing.agent_id == state.agent_id
                && existing.account == state.account
                && existing.source == source
                && existing.history_prefix == state.history_prefix
                && existing.session_path == state.session_path
                && existing.packages == state.packages;
            if exact {
                return Ok(());
            }
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: "conversation identity cannot be rebound".into(),
            });
        }
        let ConversationSource::Channel { channel_id } = source else {
            self.retain_conversation_packages(ctx, &state)?;
            return self.write_conversation_state(&state);
        };
        if self.conversation_for_channel(&channel_id).await?.is_some() {
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: format!("channel {channel_id} already has a resident conversation"),
            });
        }
        if self
            .conversation_for_account(state.account)
            .await?
            .is_some()
        {
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: "account already has a coordinating channel conversation".into(),
            });
        }
        let bytes = ctx
            .query(
                &self.chat,
                &chat::encode_query(&chat::ChatQuery::Channel {
                    channel_id: channel_id.clone(),
                }),
            )
            .await?;
        let chat::ChatReply::Channel(Some(channel)) =
            chat::decode_reply(&bytes).map_err(|sentence| Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: refusal::NOT_FOUND.into(),
                sentence: format!("no channel {channel_id}"),
            });
        };
        if channel.archived {
            return Err(Error::Module {
                reason: refusal::WRONG_STATE.into(),
                sentence: format!("channel {channel_id} is archived"),
            });
        }
        let mut state = state;
        // Binding an existing channel never replays its pre-install history.
        // Activation resumes from this committed source watermark.
        state.source_cursor = channel.head_seq;
        self.retain_conversation_packages(ctx, &state)?;
        self.write_conversation_state(&state)?;
        self.receipts
            .stage(key("channel", &channel_id), sdk::wire::encode(&id))?;
        self.receipts.stage(
            key("account", &state.account.to_string()),
            sdk::wire::encode(&id),
        )?;
        ctx.emit_msg(Msg {
            target: self.chat.clone(),
            payload: chat::encode_msg(&chat::ChatMsg::RegisterHook {
                channel_id,
                module_id: self.id.clone(),
            }),
        });
        Ok(())
    }
    fn retain_conversation_packages(
        &self,
        ctx: &mut dyn Ctx,
        state: &ConversationView,
    ) -> Result<(), Error> {
        if state.packages.is_empty() {
            return Ok(());
        }
        let files = self.files.clone().ok_or_else(|| Error::Module {
            reason: refusal::UNSUPPORTED.into(),
            sentence:
                "retaining conversation packages needs a Files module, and none is configured"
                    .into(),
        })?;
        for package in &state.packages {
            ctx.emit_msg(Msg {
                target: files.clone(),
                payload: files::encode_msg(&files::FilesMsg::CompareExchangeRetention {
                    key: format!(
                        "conversation/package/{}",
                        dispatch_id_for(
                            &serde_json::to_string(&(&state.conversation_id, &package.name))
                                .expect("package identity serializes")
                        )
                    ),
                    expected: None,
                    replacement: Some(files::RetentionReference {
                        snapshot: package.source_snapshot.clone(),
                        revision: 1,
                    }),
                }),
            });
        }
        Ok(())
    }
    pub(super) async fn activate_conversation(
        &mut self,
        ctx: &dyn Ctx,
        id: String,
        op: String,
        active: bool,
    ) -> Result<(), Error> {
        let state = self.require_conversation(&id).await?;
        self.conversation_controller(ctx, &state).await?;
        let payload = sdk::wire::encode(&("activate", active));
        if self.operation_seen(&id, &op, &payload).await? {
            return Ok(());
        }
        self.apply_conversation(ctx, &state, Input::Activate(active))
            .await?;
        self.receipts.stage(op_key(&id, &op), payload)
    }
    pub(super) async fn append_conversation_input(
        &mut self,
        ctx: &dyn Ctx,
        id: String,
        op: String,
        input: ConversationInput,
    ) -> Result<(), Error> {
        let state = self.require_conversation(&id).await?;
        self.conversation_controller(ctx, &state).await?;
        require_coordinating_source(&state)?;
        if matches!(input, ConversationInput::Chat { .. }) {
            return Err(Error::Module {
                reason: refusal::UNAUTHORIZED.into(),
                sentence: "chat snapshots require the authenticated source hook".into(),
            });
        }
        self.admit_conversation_input(ctx, &state, op, input, ctx.env().origin.clone())
            .await
    }
    async fn admit_conversation_input(
        &mut self,
        ctx: &dyn Ctx,
        state: &ConversationView,
        op: String,
        input: ConversationInput,
        actor: Origin,
    ) -> Result<(), Error> {
        let payload = sdk::wire::encode(&("append", &actor, &input));
        if self
            .operation_seen(&state.conversation_id, &op, &payload)
            .await?
        {
            return Ok(());
        }
        let sequence = state
            .admitted_cursor
            .checked_add(1)
            .ok_or_else(|| Error::Module {
                reason: refusal::EXHAUSTED.into(),
                sentence: format!(
                    "conversation {} has no event sequence numbers left",
                    state.conversation_id
                ),
            })?;
        let event = ConversationEvent {
            sequence,
            operation_id: op.clone(),
            actor,
            input,
            admitted_at: ctx.env().height,
        };
        self.apply_conversation(ctx, state, Input::Append(event))
            .await?;
        self.receipts
            .stage(op_key(&state.conversation_id, &op), payload)
    }
    /// Hook failures are recorded/paused, never propagated into the source post.
    pub(super) async fn conversation_chat_hook(
        &mut self,
        ctx: &dyn Ctx,
        payload: &[u8],
    ) -> Result<(), Error> {
        let Ok(chat::ChatEvent::MessagePosted { channel_id, .. }) = chat::decode_event(payload)
        else {
            return Ok(());
        };
        let Ok(Some(state)) = self.conversation_for_channel(&channel_id).await else {
            return Ok(());
        };
        if self.capture_conversation_source(ctx, &state).await.is_err() {
            let current = self
                .conversation(&state.conversation_id)
                .await
                .ok()
                .flatten()
                .unwrap_or(state);
            // Best-effort diagnostic only: no downstream failure may reject Chat's source write.
            let _ = self
                .apply_conversation(ctx, &current, Input::Pause("source_capture_failed".into()))
                .await;
        }
        Ok(())
    }
    async fn capture_conversation_source(
        &mut self,
        ctx: &dyn Ctx,
        state: &ConversationView,
    ) -> Result<(), Error> {
        if state.status != ConversationStatus::Active {
            return Ok(());
        }
        let ConversationSource::Channel { channel_id } = &state.source else {
            return Ok(());
        };
        let bytes = ctx
            .query(
                &self.chat,
                &chat::encode_query(&chat::ChatQuery::Channel {
                    channel_id: channel_id.clone(),
                }),
            )
            .await?;
        let chat::ChatReply::Channel(channel) =
            chat::decode_reply(&bytes).map_err(|sentence| Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence: format!(
                    "chat answered the lookup for channel {channel_id} with something other than a channel"
                ),
            });
        };
        let Some(channel) = channel else {
            return self
                .apply_conversation(ctx, state, Input::Pause("channel_missing".into()))
                .await;
        };
        if channel.archived {
            return self
                .apply_conversation(ctx, state, Input::Pause("channel_archived".into()))
                .await;
        }
        if state.source_cursor >= channel.head_seq {
            return Ok(());
        }
        let bytes = ctx
            .query(
                &self.chat,
                &chat::encode_query(&chat::ChatQuery::MessagesRange {
                    channel_id: channel_id.clone(),
                    from_seq: state.source_cursor + 1,
                    limit: 64,
                }),
            )
            .await?;
        let chat::ChatReply::Messages(messages) =
            chat::decode_reply(&bytes).map_err(|sentence| Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence: format!(
                    "chat answered the message lookup for channel {channel_id} with something other than messages"
                ),
            });
        };
        if messages.is_empty() {
            return Err(Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence: "conversation source range is empty before its head".into(),
            });
        }
        for message in messages {
            let current = self.require_conversation(&state.conversation_id).await?;
            let expected = current
                .source_cursor
                .checked_add(1)
                .ok_or_else(|| Error::Module {
                    reason: refusal::EXHAUSTED.into(),
                    sentence: format!(
                        "conversation {} has no source sequence numbers left",
                        state.conversation_id
                    ),
                })?;
            let contiguous = &message.channel_id == channel_id && message.seq == expected;
            if !contiguous {
                return Err(Error::Module {
                    reason: refusal::UNEXPECTED_REPLY.into(),
                    sentence: format!(
                        "message {}/{} is not the next source message {channel_id}/{expected}",
                        message.channel_id, message.seq
                    ),
                });
            }
            let human = matches!(message.head.content_origin, Origin::External(_));
            let retain = human && !message.head.deleted;
            let seq = message.seq;
            if retain {
                let actor = message.head.content_origin.clone();
                self.admit_conversation_input(
                    ctx,
                    &current,
                    format!("chat/{seq}"),
                    ConversationInput::Chat {
                        message: Box::new(message),
                    },
                    actor,
                )
                .await?;
            }
            let current = self.require_conversation(&state.conversation_id).await?;
            self.apply_conversation(ctx, &current, Input::SourceCursor(seq))
                .await?;
        }
        let current = self.require_conversation(&state.conversation_id).await?;
        if current.source_cursor < channel.head_seq {
            self.write_conversation_wake(ctx, &state.conversation_id)
                .await?;
        }
        Ok(())
    }
}
