//! The module: the schedule folded at each block, then every op and every
//! query, each handed to its rule.

use guest::{ExecCtx, Module, QueryCtx, Refusal, decoded};

use crate::rules::{SCHEDULE, at, cancel, fold, init, publish, schedule, views_at};
use crate::{Genesis, Op, PROGRAM, Query, Reply, Scheduled};

pub struct Modules;

impl Module for Modules {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Refusal> {
        init(ctx, decoded::<Genesis>(PROGRAM, "Genesis", params)?);
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Refusal> {
        fold(ctx, ctx.env().height)?;
        match op {
            Op::Publish { body } => publish(ctx, body),
            Op::Schedule(scheduled) => schedule(ctx, scheduled),
            Op::Cancel { height, program } => cancel(ctx, height, program),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Refusal> {
        let height = ctx.env().height;
        Ok(match query {
            Query::At(height) => Reply::Programs(at(ctx, height)?),
            Query::Views(height) => Reply::Views(views_at(ctx, height)?),
            Query::Scheduled { page } => Reply::Scheduled(
                SCHEDULE
                    .range(ctx, &page, height)?
                    .map(|((height, _), change)| Scheduled { height, change }),
            ),
            Query::Program(program) => Reply::Program {
                height,
                entry: at(ctx, height)?
                    .into_iter()
                    .find(|entry| entry.program == program),
            },
        })
    }
}

#[cfg(feature = "program")]
guest::export!(Modules);
