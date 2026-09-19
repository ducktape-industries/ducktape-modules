//! Named one-shot timers. Crank is driven by committed consensus time, not a host clock.
//!
//! The due set is point-addressed, never a record anyone reads whole. Each due
//! time owns a doubly linked list of the timers due then (`entry_key`), headed
//! by [`due_key`]; which due times are non-empty is a radix trie of 64-bit
//! words over the due time itself ([`trie_key`]), so the earliest is found by
//! descending eleven 8-byte records instead of sorting a queue. Insert, cancel
//! and one crank each touch a fixed number of records whatever the network is
//! holding; `next_conversation_input_due` stays a single read of the cached
//! head.
use super::*;
use sdk::refusal;
/// The pre-point-address whole queue: one sorted Vec of every pending timer.
/// Read once on the first touch of the new layout, carried over, then deleted;
/// the fallback goes in a later round.
pub(super) const LEGACY_SCHEDULE_QUEUE: &str = "conversation/schedule_queue";
pub(super) const NEXT_SCHEDULE_DUE: &str = "conversation/next_schedule_due";
/// A due time's list head — the entry slot of its first timer.
fn due_key(due_at: u64) -> String {
    format!("conversation/schedule_due/{due_at}")
}
/// One timer's place in its due time's list. The conversation and its slot
/// name it, so a cancel addresses its entry without searching for it.
fn entry_slot(id: &str, slot: &str) -> String {
    dispatch_id_for(&format!("{id}/{slot}"))
}
fn entry_key(due_at: u64, entry: &str) -> String {
    format!("conversation/schedule_at/{due_at}/{entry}")
}
/// One 64-bit word of the due-time trie: bit `b` of level `l` says the subtree
/// below it holds a pending timer. Eleven levels of 6 bits cover every u64.
fn trie_key(level: u32, index: u64) -> String {
    format!("conversation/schedule_trie/{level}/{index}")
}
const TRIE_LEVELS: u32 = 11;
fn trie_node(bucket: u64, level: u32) -> (u64, u32) {
    let index = bucket.checked_shr(6 * (level + 1)).unwrap_or(0);
    let bit = (bucket >> (6 * level)) & 63;
    (index, bit as u32)
}
pub(super) fn schedule_key(id: &str, slot: &str) -> String {
    format!("{}/{}", key("schedule", id), dispatch_id_for(slot))
}
pub(super) fn schedule_index(id: &str) -> String {
    key("schedules", id)
}
#[derive(Clone, Serialize, Deserialize)]
struct ScheduledRef {
    conversation_id: String,
    schedule_id: String,
    operation_id: String,
    due_at: u64,
}
/// A queued timer and its neighbours in its due time's list. Doubly linked so
/// a cancel unlinks from the middle without walking from the head.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedSchedule {
    entry: ScheduledRef,
    prev: Option<String>,
    next: Option<String>,
}

pub(super) fn validate_queue(records: &crate::receipts::Records) -> Result<(), String> {
    let head: Option<u64> = match records.get(NEXT_SCHEDULE_DUE) {
        Some(bytes) => sdk::wire::decode(bytes)?,
        None => None,
    };
    if let Some(bytes) = records.get(LEGACY_SCHEDULE_QUEUE) {
        return validate_legacy_queue(records, head, bytes);
    }
    let mut queued: BTreeMap<u64, BTreeMap<String, QueuedSchedule>> = BTreeMap::new();
    for (record_key, bytes) in records {
        let Some(rest) = record_key.strip_prefix("conversation/schedule_at/") else {
            continue;
        };
        let (due, slot) = rest
            .split_once('/')
            .ok_or_else(|| "malformed conversation timer entry key".to_string())?;
        let due: u64 = due
            .parse()
            .map_err(|_| "malformed conversation timer due time".to_string())?;
        let entry: QueuedSchedule = sdk::wire::decode(bytes)?;
        let addressed = entry.entry.due_at == due
            && entry_slot(&entry.entry.conversation_id, &entry.entry.schedule_id) == slot;
        if !addressed {
            return Err("conversation timer entry is not at its own key".into());
        }
        let bytes = records
            .get(&schedule_key(
                &entry.entry.conversation_id,
                &entry.entry.schedule_id,
            ))
            .ok_or_else(|| "missing queued conversation timer".to_string())?;
        let schedule: ConversationSchedule = sdk::wire::decode(bytes)?;
        let exact = schedule.operation_id == entry.entry.operation_id
            && schedule.status == ConversationScheduleStatus::Pending { due_at: due };
        if !exact {
            return Err("conversation timer queue record mismatch".into());
        }
        queued.entry(due).or_default().insert(slot.into(), entry);
    }
    for (due, entries) in &queued {
        let head = records
            .get(&due_key(*due))
            .ok_or_else(|| "missing conversation timer list head".to_string())?;
        let mut cursor: Option<String> = Some(sdk::wire::decode(head)?);
        let mut previous: Option<String> = None;
        let mut seen = 0;
        while let Some(slot) = cursor {
            let entry = entries
                .get(&slot)
                .ok_or_else(|| "conversation timer list leaves its due time".to_string())?;
            if entry.prev != previous {
                return Err("conversation timer list is not doubly linked".into());
            }
            seen += 1;
            if seen > entries.len() {
                return Err("conversation timer list cycles".into());
            }
            cursor = entry.next.clone();
            previous = Some(slot);
        }
        if seen != entries.len() {
            return Err("conversation timer list does not reach every entry".into());
        }
        for level in 0..TRIE_LEVELS {
            let (index, bit) = trie_node(*due, level);
            let word: u64 = match records.get(&trie_key(level, index)) {
                Some(bytes) => sdk::wire::decode(bytes)?,
                None => 0,
            };
            if word & (1 << bit) == 0 {
                return Err("conversation timer index does not mark a due time".into());
            }
        }
    }
    for record_key in records.keys() {
        let Some(due) = record_key.strip_prefix("conversation/schedule_due/") else {
            continue;
        };
        let due: u64 = due
            .parse()
            .map_err(|_| "malformed conversation timer due time".to_string())?;
        if !queued.contains_key(&due) {
            return Err("conversation timer due time holds nothing".into());
        }
    }
    validate_trie(records, &queued)?;
    if head != queued.keys().next().copied() {
        return Err("conversation timer head mismatch".into());
    }
    Ok(())
}

/// Every marked subtree leads to a pending timer: a bit left set behind a
/// cancelled timer would make the crank chase a due time that is not there.
fn validate_trie(
    records: &crate::receipts::Records,
    queued: &BTreeMap<u64, BTreeMap<String, QueuedSchedule>>,
) -> Result<(), String> {
    for (record_key, bytes) in records {
        let Some(rest) = record_key.strip_prefix("conversation/schedule_trie/") else {
            continue;
        };
        let (level, index) = rest
            .split_once('/')
            .ok_or_else(|| "malformed conversation timer index key".to_string())?;
        let level: u32 = level
            .parse()
            .map_err(|_| "malformed conversation timer index level".to_string())?;
        let index: u64 = index
            .parse()
            .map_err(|_| "malformed conversation timer index node".to_string())?;
        let word: u64 = sdk::wire::decode(bytes)?;
        if level >= TRIE_LEVELS || word == 0 {
            return Err("conversation timer index node is not addressable".into());
        }
        for bit in 0..64 {
            if word & (1 << bit) == 0 {
                continue;
            }
            let child = index
                .checked_mul(64)
                .and_then(|base| base.checked_add(bit))
                .ok_or_else(|| "conversation timer index node overflows".to_string())?;
            let occupied = match level {
                0 => queued.contains_key(&child),
                _ => records.contains_key(&trie_key(level - 1, child)),
            };
            if !occupied {
                return Err("conversation timer index marks an empty due time".into());
            }
        }
    }
    Ok(())
}

/// A snapshot taken before the carry-over still carries the whole queue, and
/// is still the truth until the next write. Goes with the fallback.
fn validate_legacy_queue(
    records: &crate::receipts::Records,
    head: Option<u64>,
    bytes: &[u8],
) -> Result<(), String> {
    let queue: Vec<ScheduledRef> = sdk::wire::decode(bytes)?;
    if head != queue.first().map(|entry| entry.due_at) {
        return Err("conversation timer head mismatch".into());
    }
    let ordered = queue.windows(2).all(|pair| {
        (
            pair[0].due_at,
            &pair[0].conversation_id,
            &pair[0].schedule_id,
        ) < (
            pair[1].due_at,
            &pair[1].conversation_id,
            &pair[1].schedule_id,
        )
    });
    if !ordered {
        return Err("conversation timer queue is not ordered".into());
    }
    for entry in queue {
        let bytes = records
            .get(&schedule_key(&entry.conversation_id, &entry.schedule_id))
            .ok_or_else(|| "missing queued conversation timer".to_string())?;
        let schedule: ConversationSchedule = sdk::wire::decode(bytes)?;
        let exact = schedule.operation_id == entry.operation_id
            && schedule.status
                == ConversationScheduleStatus::Pending {
                    due_at: entry.due_at,
                };
        if !exact {
            return Err("conversation timer queue record mismatch".into());
        }
    }
    Ok(())
}

pub(super) fn validate_for_conversation(
    records: &crate::receipts::Records,
    state: &ConversationView,
) -> Result<(), String> {
    let Some(bytes) = records.get(&schedule_index(&state.conversation_id)) else {
        return Ok(());
    };
    let slots: Vec<String> = sdk::wire::decode(bytes)?;
    let ordered = slots.windows(2).all(|pair| pair[0] < pair[1]);
    if slots.len() > 64 || !ordered {
        return Err("invalid conversation schedule index".into());
    }
    for slot in slots {
        let bytes = records
            .get(&schedule_key(&state.conversation_id, &slot))
            .ok_or_else(|| "missing conversation schedule record".to_string())?;
        let schedule: ConversationSchedule = sdk::wire::decode(bytes)?;
        let bound =
            schedule.conversation_id == state.conversation_id && schedule.schedule_id == slot;
        if !bound {
            return Err("conversation schedule identity mismatch".into());
        }
        let invalid_receipt = matches!(schedule.status, ConversationScheduleStatus::Fired { sequence } if sequence == 0 || sequence > state.admitted_cursor);
        if invalid_receipt {
            return Err("conversation timer has no admitted event".into());
        }
    }
    Ok(())
}

enum ScheduleInput {
    Set {
        schedule: ConversationSchedule,
        due_at: u64,
    },
    Cancel {
        schedule: ConversationSchedule,
    },
    Fire {
        schedule: ConversationSchedule,
        sequence: u64,
        now: u64,
    },
}
fn schedule_step(input: ScheduleInput) -> Result<ConversationSchedule, Error> {
    match input {
        ScheduleInput::Set { schedule, due_at } => schedule_set(schedule, due_at),
        ScheduleInput::Cancel { schedule } => schedule_cancel(schedule),
        ScheduleInput::Fire {
            schedule,
            sequence,
            now,
        } => schedule_fire(schedule, sequence, now),
    }
}
fn schedule_set(
    mut schedule: ConversationSchedule,
    due_at: u64,
) -> Result<ConversationSchedule, Error> {
    schedule.status = ConversationScheduleStatus::Pending { due_at };
    Ok(schedule)
}
fn schedule_cancel(mut schedule: ConversationSchedule) -> Result<ConversationSchedule, Error> {
    schedule.status = ConversationScheduleStatus::Cancelled;
    Ok(schedule)
}
fn schedule_fire(
    mut schedule: ConversationSchedule,
    sequence: u64,
    now: u64,
) -> Result<ConversationSchedule, Error> {
    let ConversationScheduleStatus::Pending { due_at } = schedule.status else {
        return Err(Error::Module {
            reason: refusal::WRONG_STATE.into(),
            sentence: format!(
                "schedule {} of conversation {} is not pending",
                schedule.schedule_id, schedule.conversation_id
            ),
        });
    };
    if now < due_at {
        return Err(Error::Module {
            reason: refusal::NOT_YET.into(),
            sentence: format!(
                "schedule {} of conversation {} is due at consensus time {due_at}, not yet at {now}",
                schedule.schedule_id, schedule.conversation_id
            ),
        });
    }
    schedule.status = ConversationScheduleStatus::Fired { sequence };
    Ok(schedule)
}

impl RunsModule {
    async fn trie_word(&self, level: u32, index: u64) -> Result<u64, Error> {
        Ok(self
            .conversation_read::<u64>(&trie_key(level, index))
            .await?
            .unwrap_or_default())
    }
    /// Mark `due` occupied. Stops at the first level already marked: an
    /// ancestor set means every ancestor above it is set too.
    async fn trie_insert(&mut self, due: u64) -> Result<(), Error> {
        for level in 0..TRIE_LEVELS {
            let (index, bit) = trie_node(due, level);
            let word = self.trie_word(level, index).await?;
            if word & (1 << bit) != 0 {
                return Ok(());
            }
            self.receipts.stage(
                trie_key(level, index),
                sdk::wire::encode(&(word | 1 << bit)),
            )?;
        }
        Ok(())
    }
    /// Unmark `due`, dropping each node that empties. Stops at the first level
    /// that still holds something.
    async fn trie_remove(&mut self, due: u64) -> Result<(), Error> {
        for level in 0..TRIE_LEVELS {
            let (index, bit) = trie_node(due, level);
            let word = self.trie_word(level, index).await? & !(1u64 << bit);
            if word == 0 {
                self.receipts.remove(trie_key(level, index));
                continue;
            }
            return self
                .receipts
                .stage(trie_key(level, index), sdk::wire::encode(&word));
        }
        Ok(())
    }
    /// The earliest occupied due time: one descent, one 8-byte record a level.
    async fn trie_min(&self) -> Result<Option<u64>, Error> {
        let mut index = 0u64;
        for level in (0..TRIE_LEVELS).rev() {
            let word = self.trie_word(level, index).await?;
            if word == 0 {
                return Ok(None);
            }
            index = index
                .checked_mul(64)
                .and_then(|base| base.checked_add(word.trailing_zeros().into()))
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: "conversation timer index addresses a due time past u64".into(),
                })?;
        }
        Ok(Some(index))
    }
    /// Push one timer onto the head of its due time's list.
    async fn queue_schedule(&mut self, entry: ScheduledRef) -> Result<(), Error> {
        let due = entry.due_at;
        let slot = entry_slot(&entry.conversation_id, &entry.schedule_id);
        let head: Option<String> = self.conversation_read(&due_key(due)).await?;
        if let Some(head) = &head {
            let mut first: QueuedSchedule = self
                .conversation_read(&entry_key(due, head))
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!("conversation timer list at {due} has no head record"),
                })?;
            first.prev = Some(slot.clone());
            self.receipts
                .stage(entry_key(due, head), sdk::wire::encode(&first))?;
        }
        let queued = QueuedSchedule {
            entry,
            prev: None,
            next: head.clone(),
        };
        self.receipts
            .stage(entry_key(due, &slot), sdk::wire::encode(&queued))?;
        self.receipts
            .stage(due_key(due), sdk::wire::encode(&slot))?;
        if head.is_some() {
            return Ok(());
        }
        self.trie_insert(due).await?;
        let earliest = self.next_conversation_input_due().await?;
        if matches!(earliest, Some(earliest) if earliest <= due) {
            return Ok(());
        }
        self.receipts
            .stage(NEXT_SCHEDULE_DUE.into(), sdk::wire::encode(&Some(due)))
    }
    /// Unlink one timer from its due time's list, reading only its neighbours.
    async fn unqueue_schedule(&mut self, due: u64, id: &str, slot: &str) -> Result<(), Error> {
        let slot = entry_slot(id, slot);
        let Some(queued) = self
            .conversation_read::<QueuedSchedule>(&entry_key(due, &slot))
            .await?
        else {
            return Ok(());
        };
        self.receipts.remove(entry_key(due, &slot));
        if let Some(next) = &queued.next {
            let mut following: QueuedSchedule = self
                .conversation_read(&entry_key(due, next))
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!("conversation timer list at {due} breaks after {slot}"),
                })?;
            following.prev.clone_from(&queued.prev);
            self.receipts
                .stage(entry_key(due, next), sdk::wire::encode(&following))?;
        }
        if let Some(previous) = &queued.prev {
            let mut earlier: QueuedSchedule = self
                .conversation_read(&entry_key(due, previous))
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!("conversation timer list at {due} breaks before {slot}"),
                })?;
            earlier.next.clone_from(&queued.next);
            return self
                .receipts
                .stage(entry_key(due, previous), sdk::wire::encode(&earlier));
        }
        if let Some(next) = queued.next {
            return self.receipts.stage(due_key(due), sdk::wire::encode(&next));
        }
        self.receipts.remove(due_key(due));
        self.trie_remove(due).await?;
        if self.next_conversation_input_due().await? != Some(due) {
            return Ok(());
        }
        let earliest = self.trie_min().await?;
        self.receipts
            .stage(NEXT_SCHEDULE_DUE.into(), sdk::wire::encode(&earliest))
    }
    /// Carry the whole queue into the point-addressed layout, once. O(the old
    /// record) — bounded by the store value cap that made it a problem — and
    /// then the old key is gone and every timer path is point-addressed.
    async fn carry_over_schedule_queue(&mut self) -> Result<(), Error> {
        let Some(bytes) = self.receipts.get(LEGACY_SCHEDULE_QUEUE).await? else {
            return Ok(());
        };
        self.receipts.remove(LEGACY_SCHEDULE_QUEUE.into());
        let queue: Vec<ScheduledRef> =
            sdk::wire::decode(&bytes).map_err(|sentence| Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence,
            })?;
        // The old queue was sorted ascending and each entry is pushed onto its
        // list head, so descending carry-over preserves the firing order.
        for entry in queue.into_iter().rev() {
            self.queue_schedule(entry).await?;
        }
        Ok(())
    }
    pub(crate) async fn next_conversation_input_due(&self) -> Result<Option<u64>, Error> {
        Ok(self
            .conversation_read::<Option<u64>>(NEXT_SCHEDULE_DUE)
            .await?
            .flatten())
    }
    pub(crate) async fn conversation_schedules(
        &self,
        id: &str,
    ) -> Result<Vec<ConversationSchedule>, Error> {
        let slots: Vec<String> = self
            .conversation_read(&schedule_index(id))
            .await?
            .unwrap_or_default();
        let mut schedules = Vec::new();
        for slot in slots {
            let schedule = self
                .conversation_read(&schedule_key(id, &slot))
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!(
                        "conversation {id} lists schedule {slot} but has no record of it"
                    ),
                })?;
            schedules.push(schedule);
        }
        Ok(schedules)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn schedule_conversation_input(
        &mut self,
        ctx: &dyn Ctx,
        id: String,
        op: String,
        slot: String,
        after_secs: Option<u64>,
        input: ConversationInput,
    ) -> Result<(), Error> {
        let state = self.require_conversation(&id).await?;
        self.conversation_controller(ctx, &state).await?;
        require_coordinating_source(&state)?;
        let valid_slot = !slot.is_empty() && slot.len() <= MAX_REQUEST_ID_BYTES;
        if !valid_slot {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!("a schedule id is 1 to {MAX_REQUEST_ID_BYTES} bytes"),
            });
        }
        if matches!(input, ConversationInput::Chat { .. }) {
            return Err(Error::Module {
                reason: refusal::UNAUTHORIZED.into(),
                sentence: "scheduled inputs cannot forge Chat snapshots".into(),
            });
        }
        let payload = sdk::wire::encode(&("schedule", &slot, after_secs, &input));
        if self.operation_seen(&id, &op, &payload).await? {
            return Ok(());
        }
        let schedule = ConversationSchedule {
            conversation_id: id.clone(),
            schedule_id: slot.clone(),
            operation_id: op.clone(),
            actor: ctx.env().origin.clone(),
            input,
            status: ConversationScheduleStatus::Cancelled,
        };
        let transition = match after_secs {
            None => ScheduleInput::Cancel { schedule },
            Some(seconds) => {
                let unit = self.time_unit.ok_or_else(|| Error::Module {
                    reason: refusal::UNSUPPORTED.into(),
                    sentence: "conversation scheduling requires genesis time_unit".into(),
                })?;
                let duration =
                    seconds
                        .checked_mul(unit.per_second())
                        .ok_or_else(|| Error::Module {
                            reason: refusal::INVALID_INPUT.into(),
                            sentence: format!(
                                "a delay of {seconds} seconds is too long to schedule"
                            ),
                        })?;
                let due_at = ctx
                    .env()
                    .consensus_time
                    .checked_add(duration)
                    .ok_or_else(|| Error::Module {
                        reason: refusal::INVALID_INPUT.into(),
                        sentence: format!(
                            "{seconds} seconds from now is past the last representable time"
                        ),
                    })?;
                ScheduleInput::Set { schedule, due_at }
            }
        };
        let schedule = schedule_step(transition)?;
        self.carry_over_schedule_queue().await?;
        let replaced = self
            .conversation_read::<ConversationSchedule>(&schedule_key(&id, &slot))
            .await?
            .map(|previous| previous.status);
        if let Some(ConversationScheduleStatus::Pending { due_at }) = replaced {
            self.unqueue_schedule(due_at, &id, &slot).await?;
        }
        let mut slots: Vec<String> = self
            .conversation_read(&schedule_index(&id))
            .await?
            .unwrap_or_default();
        if !slots.contains(&slot) {
            if slots.len() >= 64 {
                return Err(Error::Module {
                    reason: refusal::CAPACITY.into(),
                    sentence: format!("conversation {id} already has 64 schedules, its limit"),
                });
            }
            slots.push(slot.clone());
            slots.sort();
        }
        if let ConversationScheduleStatus::Pending { due_at } = schedule.status {
            self.queue_schedule(ScheduledRef {
                conversation_id: id.clone(),
                schedule_id: slot,
                operation_id: op.clone(),
                due_at,
            })
            .await?;
        }
        let records = [
            (
                schedule_key(&id, &schedule.schedule_id),
                sdk::wire::encode(&schedule),
            ),
            (schedule_index(&id), sdk::wire::encode(&slots)),
            (op_key(&id, &op), payload),
        ];
        self.write_schedule_records(records)
    }
    fn write_schedule_records<const N: usize>(
        &mut self,
        records: [(String, Vec<u8>); N],
    ) -> Result<(), Error> {
        let fits = records
            .iter()
            .all(|(_, bytes)| bytes.len() <= sdk::MAX_STORE_VALUE_BYTES);
        if !fits {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!(
                    "a conversation schedule record exceeds the {}-byte store value bound",
                    sdk::MAX_STORE_VALUE_BYTES
                ),
            });
        }
        for (key, bytes) in records {
            self.receipts.stage(key, bytes)?;
        }
        Ok(())
    }
    pub(crate) async fn crank_conversation_inputs(&mut self, ctx: &dyn Ctx) -> Result<(), Error> {
        self.carry_over_schedule_queue().await?;
        for _ in 0..32 {
            let Some(due_at) = self.next_conversation_input_due().await? else {
                return Ok(());
            };
            if due_at > ctx.env().consensus_time {
                return Ok(());
            }
            let slot: String =
                self.conversation_read(&due_key(due_at))
                    .await?
                    .ok_or_else(|| Error::Module {
                        reason: refusal::CORRUPT.into(),
                        sentence: format!("conversation timer {due_at} is indexed but has no list"),
                    })?;
            let queued: QueuedSchedule = self
                .conversation_read(&entry_key(due_at, &slot))
                .await?
                .ok_or_else(|| Error::Module {
                reason: refusal::CORRUPT.into(),
                sentence: format!("conversation timer list at {due_at} has no head record"),
            })?;
            let entry = queued.entry;
            let record_key = schedule_key(&entry.conversation_id, &entry.schedule_id);
            let schedule: ConversationSchedule = self
                .conversation_read(&record_key)
                .await?
                .ok_or_else(|| Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: format!(
                        "schedule {} of conversation {} is queued but has no record",
                        entry.schedule_id, entry.conversation_id
                    ),
                })?;
            let current = schedule.operation_id == entry.operation_id
                && schedule.status
                    == ConversationScheduleStatus::Pending {
                        due_at: entry.due_at,
                    };
            if !current {
                return Err(Error::Module {
                    reason: refusal::CORRUPT.into(),
                    sentence: "scheduled input index disagrees with its record".into(),
                });
            }
            let state = self.require_conversation(&entry.conversation_id).await?;
            let sequence = state
                .admitted_cursor
                .checked_add(1)
                .ok_or_else(|| Error::Module {
                    reason: refusal::EXHAUSTED.into(),
                    sentence: format!(
                        "conversation {} has no input sequence numbers left",
                        entry.conversation_id
                    ),
                })?;
            let operation_id = format!(
                "timer/{}",
                dispatch_id_for(&format!("{}/{}", entry.schedule_id, entry.operation_id))
            );
            // Timer provenance is the account that configured this exact slot,
            // never the permissionless member that happened to crank it.
            let event = ConversationEvent {
                sequence,
                operation_id,
                actor: schedule.actor.clone(),
                input: schedule.input.clone(),
                admitted_at: ctx.env().height,
            };
            self.apply_conversation(ctx, &state, Input::Append(event))
                .await?;
            let schedule = schedule_step(ScheduleInput::Fire {
                schedule,
                sequence,
                now: ctx.env().consensus_time,
            })?;
            self.write_schedule_records([(record_key, sdk::wire::encode(&schedule))])?;
            self.unqueue_schedule(due_at, &entry.conversation_id, &entry.schedule_id)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl RunsModule {
    /// Seed one pending timer straight into the queue. A bound test needs a
    /// crowded network, not a crowded operation transcript.
    pub(super) async fn seed_conversation_timer(
        &mut self,
        id: &str,
        slot: &str,
        due_at: u64,
    ) -> Result<(), Error> {
        self.queue_schedule(ScheduledRef {
            conversation_id: id.into(),
            schedule_id: slot.into(),
            operation_id: timer_operation_id(id, slot),
            due_at,
        })
        .await
    }
}

/// The whole-queue record exactly as a pre-carry-over snapshot holds it.
#[cfg(test)]
pub(super) fn legacy_timer_queue(timers: &[(&str, &str, u64)]) -> Vec<u8> {
    let queue: Vec<ScheduledRef> = timers
        .iter()
        .map(|(id, slot, due_at)| ScheduledRef {
            conversation_id: (*id).into(),
            schedule_id: (*slot).into(),
            operation_id: timer_operation_id(id, slot),
            due_at: *due_at,
        })
        .collect();
    sdk::wire::encode(&queue)
}
#[cfg(test)]
pub(super) fn timer_operation_id(id: &str, slot: &str) -> String {
    format!("{id}/{slot}")
}
