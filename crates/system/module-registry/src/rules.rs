// The rules: the schedule folded at each block, then the ops and queries.

use std::collections::BTreeMap;

use guest::{Error, ExecCtx, QueryCtx, already_exists, invalid, not_found};
use guest::{HashKind, ModuleId};
use store::{Item, Map};

use crate::{AUTHORITY, CODE_KIND, Change, Entry, Genesis, Scheduled, View};

const PROGRAMS: Map<ModuleId, Entry> = Map::new("p/");
const VIEWS: Map<ModuleId, View> = Map::new("v/");
type At = (u64, ModuleId);
pub(crate) const SCHEDULE: Map<At, Change> = Map::new("s/");
const FOLDED: Item<u64> = Item::new("folded");

pub(crate) fn init(ctx: &ExecCtx, genesis: Genesis) {
    for entry in genesis.programs {
        PROGRAMS.put(ctx, &entry.program, &entry);
    }
    for view in genesis.views {
        VIEWS.put(ctx, &view.name, &view);
    }
    FOLDED.put(ctx, &0);
}

pub(crate) fn publish(ctx: &ExecCtx, body: Vec<u8>) -> Result<(), Error> {
    let id = ctx.blob_put(HashKind::Sha256, CODE_KIND, body)?;
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) fn schedule(ctx: &ExecCtx, scheduled: Scheduled) -> Result<(), Error> {
    let env = ctx.env();
    env.sent_by(AUTHORITY)?;
    let in_the_future = scheduled.height > env.height;
    if !in_the_future {
        return Err(invalid(format!(
            "a change lands at a later block than {}",
            env.height
        )));
    }
    if let Some(blob) = scheduled.change.code()
        && ctx.blob_stat(blob).is_none()
    {
        return Err(not_found(format!("code {blob:?} is not published")));
    }
    let name = scheduled.change.program();
    let (programs, views) = roster(ctx, scheduled.height)?;
    let clash = match &scheduled.change {
        Change::Set(_) => views.contains_key(name) || pending(ctx, name, Kind::View)?,
        Change::SetView(_) => programs.contains_key(name) || pending(ctx, name, Kind::Program)?,
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
    if SCHEDULE.has(ctx, &key) {
        return Err(already_exists(format!(
            "{} already changes at {}",
            key.1, key.0
        )));
    }
    SCHEDULE.put(ctx, &key, &scheduled.change);
    Ok(())
}

pub(crate) fn cancel(ctx: &ExecCtx, height: u64, program: ModuleId) -> Result<(), Error> {
    let env = ctx.env();
    env.sent_by(AUTHORITY)?;
    let key = (height, program);
    let Some(change) = SCHEDULE.get(ctx, &key)? else {
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
        && pending(ctx, &key.1, other)?
    {
        return Err(already_exists(format!(
            "{} is claimed by a pending change of the other kind",
            key.1
        )));
    }
    SCHEDULE.remove(ctx, &key);
    Ok(())
}

pub(crate) fn fold(ctx: &ExecCtx, height: u64) -> Result<(), Error> {
    let folded = FOLDED.get(ctx)?.unwrap_or(0);
    let nothing_new = folded >= height;
    if nothing_new {
        return Ok(());
    }
    for (key, change) in due(ctx, height)? {
        match &change {
            Change::Set(entry) => PROGRAMS.put(ctx, &entry.program, entry),
            Change::Remove(program) => PROGRAMS.remove(ctx, program),
            Change::SetView(view) => VIEWS.put(ctx, &view.name, view),
            Change::RemoveView(name) => VIEWS.remove(ctx, name),
        }
        SCHEDULE.remove(ctx, &key);
    }
    FOLDED.put(ctx, &height);
    Ok(())
}

fn due(ctx: &QueryCtx, height: u64) -> Result<Vec<(At, Change)>, Error> {
    SCHEDULE.scan(ctx, SCHEDULE.below(&(height + 1)))
}

pub(crate) fn at(ctx: &QueryCtx, height: u64) -> Result<Vec<Entry>, Error> {
    Ok(roster(ctx, height)?.0.into_values().collect())
}

pub(crate) fn views_at(ctx: &QueryCtx, height: u64) -> Result<Vec<View>, Error> {
    Ok(roster(ctx, height)?.1.into_values().collect())
}

type Roster = (BTreeMap<ModuleId, Entry>, BTreeMap<ModuleId, View>);

/// Both lists at a height, by name: what is folded, and every change due by then.
fn roster(ctx: &QueryCtx, height: u64) -> Result<Roster, Error> {
    let mut programs: BTreeMap<_, _> = PROGRAMS.all(ctx)?.into_iter().collect();
    let mut views: BTreeMap<_, _> = VIEWS.all(ctx)?.into_iter().collect();
    for (_, change) in due(ctx, height)? {
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
fn pending(ctx: &QueryCtx, name: &str, kind: Kind) -> Result<bool, Error> {
    Ok(SCHEDULE
        .all(ctx)?
        .into_iter()
        .any(|((_, program), change)| {
            program == name
                && matches!(
                    (kind, change),
                    (Kind::Program, Change::Set(_)) | (Kind::View, Change::SetView(_))
                )
        }))
}
