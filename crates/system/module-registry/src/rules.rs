// The rules over any store: the schedule folded at each block, then the ops and queries.

use abi::{Env, HashKind, ProgramId, Refusal};
use store::{Item, Map, Reads, Writes, already_exists, invalid, not_found};

use crate::{AUTHORITY, CODE_KIND, Change, Entry, Genesis, Op, Query, Reply, Scheduled};

const PROGRAMS: Map<ProgramId, Entry> = Map::new("p/");
type At = (u64, ProgramId);
const SCHEDULE: Map<At, Change> = Map::new("s/");
const FOLDED: Item<u64> = Item::new("folded");

pub fn init(store: &mut impl Writes, genesis: Genesis) {
    for entry in genesis.programs {
        PROGRAMS.put(store, &entry.program, &entry);
    }
    FOLDED.put(store, &0);
}

pub fn execute(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Refusal> {
    fold(store, env.height)?;
    match op {
        Op::Publish { body } => publish(store, body),
        Op::Schedule(scheduled) => schedule(store, env, scheduled),
        Op::Cancel { height, program } => cancel(store, env, height, program),
    }
}

pub fn query(store: &impl Reads, env: &Env, query: Query) -> Result<Reply, Refusal> {
    Ok(match query {
        Query::At(height) => Reply::Programs(at(store, height)?),
        Query::Scheduled { page } => Reply::Scheduled(
            SCHEDULE
                .range(store, &page, env.height)?
                .map(|((height, _), change)| Scheduled { height, change }),
        ),
        Query::Program(program) => Reply::Program {
            height: env.height,
            entry: at(store, env.height)?
                .into_iter()
                .find(|entry| entry.program == program),
        },
    })
}

fn publish(store: &mut impl Writes, body: Vec<u8>) -> Result<(), Refusal> {
    let id = store.blob_put(HashKind::Sha256, CODE_KIND, body)?;
    store.output(abi::encode(&id));
    Ok(())
}

fn schedule(store: &mut impl Writes, env: &Env, scheduled: Scheduled) -> Result<(), Refusal> {
    crate::helpers::from(env, AUTHORITY)?;
    let in_the_future = scheduled.height > env.height;
    if !in_the_future {
        return Err(invalid(format!(
            "a change lands at a later block than {}",
            env.height
        )));
    }
    if let Change::Set(entry) = &scheduled.change {
        let code_is_here = store.blob_stat(entry.code).is_some();
        if !code_is_here {
            return Err(not_found(format!("code {:?} is not published", entry.code)));
        }
    }
    let key = (scheduled.height, scheduled.change.program().to_owned());
    if SCHEDULE.has(store, &key) {
        return Err(already_exists(format!(
            "{} already changes at {}",
            key.1, key.0
        )));
    }
    SCHEDULE.put(store, &key, &scheduled.change);
    Ok(())
}

fn cancel(
    store: &mut impl Writes,
    env: &Env,
    height: u64,
    program: ProgramId,
) -> Result<(), Refusal> {
    crate::helpers::from(env, AUTHORITY)?;
    let key = (height, program);
    if !SCHEDULE.has(store, &key) {
        return Err(not_found(format!("{} does not change at {height}", key.1)));
    }
    SCHEDULE.remove(store, &key);
    Ok(())
}

fn fold(store: &mut impl Writes, height: u64) -> Result<(), Refusal> {
    let folded = FOLDED.get(store)?.unwrap_or(0);
    let nothing_new = folded >= height;
    if nothing_new {
        return Ok(());
    }
    for (key, change) in due(store, height)? {
        match &change {
            Change::Set(entry) => PROGRAMS.put(store, &entry.program, entry),
            Change::Remove(program) => PROGRAMS.remove(store, program),
        }
        SCHEDULE.remove(store, &key);
    }
    FOLDED.put(store, &height);
    Ok(())
}

fn due(store: &impl Reads, height: u64) -> Result<Vec<(At, Change)>, Refusal> {
    SCHEDULE.scan(store, SCHEDULE.below(&(height + 1)))
}

fn at(store: &impl Reads, height: u64) -> Result<Vec<Entry>, Refusal> {
    let mut entries: Vec<Entry> = PROGRAMS
        .all(store)?
        .into_iter()
        .map(|(_, entry)| entry)
        .collect();
    for (_, change) in due(store, height)? {
        match change {
            Change::Set(entry) => {
                entries.retain(|running| running.program != entry.program);
                entries.push(entry);
            }
            Change::Remove(program) => entries.retain(|running| running.program != program),
        }
    }
    entries.sort_by(|a, b| a.program.cmp(&b.program));
    Ok(entries)
}
