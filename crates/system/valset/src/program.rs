//! The module: every op and every query, each handed to its rule.

use guest::{Error, ExecCtx, Module, QueryCtx, decoded};
use module_registry::AUTHORITY;

use crate::rules::{MEMBERS, init, memberships, remove, set};
use crate::{Genesis, MODULE, Membership, Op, Query, Reply, Role};

pub struct Valset;

impl Module for Valset {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, decoded::<Genesis>(MODULE, "Genesis", params)?)
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        module_registry::helpers::from(ctx.env(), AUTHORITY)?;
        match op {
            Op::Set(membership) => set(ctx, membership),
            Op::Remove { key } => remove(ctx, &key),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Validators => Reply::Validators(
                memberships(ctx)?
                    .into_iter()
                    .filter(|membership| membership.role == Role::Validator)
                    .map(|membership| membership.key)
                    .collect(),
            ),
            Query::Members => {
                Reply::Members(memberships(ctx)?.iter().map(Membership::member).collect())
            }
            Query::Memberships { page } => Reply::Memberships(
                MEMBERS
                    .range(ctx, &page, ctx.env().height)?
                    .map(|(_, membership)| membership),
            ),
            Query::Membership { key } => Reply::Membership(MEMBERS.get(ctx, &key)?),
        })
    }
}

#[cfg(feature = "module")]
guest::export!(Valset);
