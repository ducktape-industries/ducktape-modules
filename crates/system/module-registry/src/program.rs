// The wasm32 program over the contract: guest contexts as the store, the schedule folded at each block, then the ops and queries.

use abi::{Env, HashKind, Refusal, Scan};
use guest::{Execute, Program, Query as QueryCtx, Reads};
use crate::helpers::{already_exists, invalid, not_found, u64_key};
use crate::{AUTHORITY, CODE_KIND, Change, Entry, Genesis, Op, Page, Query, Reply, Scheduled};

const PROGRAM: &str = "p/";
const SCHEDULE: &str = "s/";
const FOLDED: &[u8] = b"folded";

struct Modules;

fn program_key(program: &str) -> Vec<u8> {
    format!("{PROGRAM}{program}").into_bytes()
}

fn schedule_key(height: u64, program: &str) -> Vec<u8> {
    let mut key = u64_key(SCHEDULE, height);
    key.push(b'/');
    key.extend_from_slice(program.as_bytes());
    key
}

impl Program for Modules {
    fn init(ctx: &mut Execute, _env: &Env, params: &[u8]) -> Result<(), Refusal> {
        let genesis: Genesis = abi::decode(params)?;
        for entry in genesis.programs {
            ctx.put(program_key(&entry.program), &entry);
        }
        ctx.put(FOLDED, &0u64);
        Ok(())
    }

    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        fold(ctx, env.height)?;
        match abi::decode(payload)? {
            Op::Publish { body } => publish(ctx, body),
            Op::Schedule(scheduled) => schedule(ctx, env, scheduled),
            Op::Cancel { height, program } => cancel(ctx, env, height, &program),
        }
    }

    fn query(ctx: &mut QueryCtx, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = match abi::decode(request)? {
            Query::At(height) => Reply::Programs(at(ctx, height)?),
            Query::Scheduled { page } => {
                Reply::Scheduled(page.reply(env.height, scheduled(ctx, &page)?))
            }
            Query::Program(program) => Reply::Program {
                height: env.height,
                entry: at(ctx, env.height)?
                    .into_iter()
                    .find(|entry| entry.program == program),
            },
        };
        ctx.reply(&reply);
        Ok(())
    }
}

fn publish(ctx: &mut Execute, body: Vec<u8>) -> Result<(), Refusal> {
    let id = ctx.blob_put(HashKind::Sha256, CODE_KIND, body)?;
    ctx.output(abi::encode(&id));
    Ok(())
}

fn schedule(ctx: &mut Execute, env: &Env, scheduled: Scheduled) -> Result<(), Refusal> {
    crate::helpers::from(env, AUTHORITY)?;
    let in_the_future = scheduled.height > env.height;
    if !in_the_future {
        return Err(invalid(format!(
            "a change lands at a later block than {}",
            env.height
        )));
    }
    if let Change::Set(entry) = &scheduled.change {
        let code_is_here = ctx.blob_stat(entry.code).is_some();
        if !code_is_here {
            return Err(not_found(format!("code {:?} is not published", entry.code)));
        }
    }
    let key = schedule_key(scheduled.height, scheduled.change.program());
    let taken = ctx.get(&key).is_some();
    if taken {
        return Err(already_exists(format!(
            "{} already changes at {}",
            scheduled.change.program(),
            scheduled.height
        )));
    }
    ctx.put(key, &scheduled.change);
    Ok(())
}

fn cancel(ctx: &mut Execute, env: &Env, height: u64, program: &str) -> Result<(), Refusal> {
    crate::helpers::from(env, AUTHORITY)?;
    let key = schedule_key(height, program);
    let pending = ctx.get(&key).is_some();
    if !pending {
        return Err(not_found(format!("{program} does not change at {height}")));
    }
    ctx.delete(key);
    Ok(())
}

fn fold(ctx: &mut Execute, height: u64) -> Result<(), Refusal> {
    let folded: u64 = ctx.record(FOLDED)?.unwrap_or(0);
    let nothing_new = folded >= height;
    if nothing_new {
        return Ok(());
    }
    for (key, change) in due(ctx, height)? {
        apply(ctx, &change);
        ctx.delete(key);
    }
    ctx.put(FOLDED, &height);
    Ok(())
}

fn apply(ctx: &mut Execute, change: &Change) {
    match change {
        Change::Set(entry) => ctx.put(program_key(&entry.program), entry),
        Change::Remove(program) => ctx.delete(program_key(program)),
    }
}

fn due(ctx: &impl Reads, height: u64) -> Result<Vec<(Vec<u8>, Change)>, Refusal> {
    let past_due = u64_key(SCHEDULE, height + 1);
    ctx.records(Scan::range(SCHEDULE.as_bytes().to_vec(), Some(past_due)))
}

fn at(ctx: &impl Reads, height: u64) -> Result<Vec<Entry>, Refusal> {
    let mut entries: Vec<Entry> = ctx
        .records::<Entry>(Scan::prefix(PROGRAM))?
        .into_iter()
        .map(|(_, entry)| entry)
        .collect();
    for (_, change) in due(ctx, height)? {
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

fn scheduled(ctx: &impl Reads, page: &Page) -> Result<Vec<(Vec<u8>, Scheduled)>, Refusal> {
    ctx.records::<Change>(page.scan_ahead(SCHEDULE.as_bytes()))?
        .into_iter()
        .map(|(key, change)| {
            let height = height_of(&key)?;
            Ok((key, Scheduled { height, change }))
        })
        .collect()
}

fn height_of(key: &[u8]) -> Result<u64, Refusal> {
    let bytes = key
        .get(SCHEDULE.len()..SCHEDULE.len() + 8)
        .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
        .ok_or_else(|| invalid("a schedule key names no height"))?;
    Ok(u64::from_be_bytes(bytes))
}

guest::program!(Modules);
