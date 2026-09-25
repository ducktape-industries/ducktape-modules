// The rules over any store: the schedule folded at each block, then the ops and queries.

use std::collections::BTreeMap;

use abi::{Env, HashKind, ProgramId, Refusal};
use store::{Item, Map, Reads, Writes, already_exists, invalid, not_found};

use crate::{AUTHORITY, CODE_KIND, Change, Entry, Genesis, Op, Query, Reply, Scheduled, View};

const PROGRAMS: Map<ProgramId, Entry> = Map::new("p/");
const VIEWS: Map<ProgramId, View> = Map::new("v/");
type At = (u64, ProgramId);
const SCHEDULE: Map<At, Change> = Map::new("s/");
const FOLDED: Item<u64> = Item::new("folded");

pub fn init(store: &mut impl Writes, genesis: Genesis) {
    for entry in genesis.programs {
        PROGRAMS.put(store, &entry.program, &entry);
    }
    for view in genesis.views {
        VIEWS.put(store, &view.name, &view);
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
        Query::Views(height) => Reply::Views(views_at(store, height)?),
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
    if let Some(blob) = scheduled.change.code()
        && store.blob_stat(blob).is_none()
    {
        return Err(not_found(format!("code {blob:?} is not published")));
    }
    let name = scheduled.change.program();
    let (programs, views) = roster(store, scheduled.height)?;
    let clash = match &scheduled.change {
        Change::Set(_) => views.contains_key(name) || pending(store, name, Kind::View)?,
        Change::SetView(_) => programs.contains_key(name) || pending(store, name, Kind::Program)?,
        Change::Remove(_) if !programs.contains_key(name) => {
            return Err(not_found(format!(
                "no program {name} runs at {}",
                scheduled.height
            )));
        }
        Change::RemoveView(_) if !views.contains_key(name) => {
            return Err(not_found(format!(
                "no view {name} is listed at {}",
                scheduled.height
            )));
        }
        Change::Remove(_) | Change::RemoveView(_) => false,
    };
    if clash {
        return Err(already_exists(format!(
            "{name} already names a program or a view"
        )));
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
    let Some(change) = SCHEDULE.get(store, &key)? else {
        return Err(not_found(format!("{} does not change at {height}", key.1)));
    };
    // A removal cancelled keeps its name held, which a pending set of the
    // other kind may since have claimed.
    let reinstated = match change {
        Change::Remove(_) => Some(Kind::View),
        Change::RemoveView(_) => Some(Kind::Program),
        Change::Set(_) | Change::SetView(_) => None,
    };
    if let Some(other) = reinstated
        && pending(store, &key.1, other)?
    {
        return Err(already_exists(format!(
            "{} is claimed by a pending change of the other kind",
            key.1
        )));
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
            Change::SetView(view) => VIEWS.put(store, &view.name, view),
            Change::RemoveView(name) => VIEWS.remove(store, name),
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
    Ok(roster(store, height)?.0.into_values().collect())
}

fn views_at(store: &impl Reads, height: u64) -> Result<Vec<View>, Refusal> {
    Ok(roster(store, height)?.1.into_values().collect())
}

type Roster = (BTreeMap<ProgramId, Entry>, BTreeMap<ProgramId, View>);

/// Both lists at a height, by name: what is folded, and every change due by then.
fn roster(store: &impl Reads, height: u64) -> Result<Roster, Refusal> {
    let mut programs: BTreeMap<_, _> = PROGRAMS.all(store)?.into_iter().collect();
    let mut views: BTreeMap<_, _> = VIEWS.all(store)?.into_iter().collect();
    for (_, change) in due(store, height)? {
        match change {
            Change::Set(entry) => {
                programs.insert(entry.program.clone(), entry);
            }
            Change::Remove(program) => {
                programs.remove(&program);
            }
            Change::SetView(view) => {
                views.insert(view.name.clone(), view);
            }
            Change::RemoveView(name) => {
                views.remove(&name);
            }
        }
    }
    Ok((programs, views))
}

#[derive(Clone, Copy)]
enum Kind {
    Program,
    View,
}

/// Whether a set of `kind` under `name` waits anywhere in the schedule, at any height.
fn pending(store: &impl Reads, name: &str, kind: Kind) -> Result<bool, Refusal> {
    Ok(SCHEDULE
        .all(store)?
        .into_iter()
        .any(|((_, program), change)| {
            program == name
                && matches!(
                    (kind, change),
                    (Kind::Program, Change::Set(_)) | (Kind::View, Change::SetView(_))
                )
        }))
}
