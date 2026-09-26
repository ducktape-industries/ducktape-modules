//! The module: every op and every query, each handed to its rule.

use guest::{Error, ExecCtx, Module, QueryCtx};

use crate::rules::{
    ACCOUNTS, CONTROLLED, OF_KEY, account, add_key, create, create_program, generation, profiles,
    remove_key, resolve, revoke, set_name, set_profile, set_status, transfer_control,
};
use crate::{Op, Query, Reply};

pub struct Identity;

impl Module for Identity {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Create { name, scheme } => create(ctx, name, scheme),
            Op::AddKey {
                scheme,
                label,
                consent,
            } => add_key(ctx, scheme, label, consent),
            Op::RemoveKey { key } => remove_key(ctx, &key),
            Op::SetName { account, name } => set_name(ctx, account, name),
            Op::SetProfile {
                account,
                avatar,
                bio,
            } => set_profile(ctx, account, avatar, bio),
            Op::CreateProgram { name, controller } => create_program(ctx, name, controller),
            Op::SetStatus { account, status } => set_status(ctx, account, status),
            Op::TransferControl { account, to } => transfer_control(ctx, account, to),
            Op::Revoke { account } => revoke(ctx, account),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let height = ctx.env().height;
        Ok(match query {
            Query::Get { number } => Reply::Account(ACCOUNTS.get(ctx, &number)?),
            Query::OfKey { key } => Reply::Number(OF_KEY.get(ctx, &key)?),
            Query::Profiles { after, limit } => profiles(ctx, after, limit)?,
            Query::Generation { key } => Reply::Generation(generation(ctx, &key)?),
            Query::Resolve { references } => Reply::Resolved(
                references
                    .iter()
                    .map(|reference| resolve(ctx, reference))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Query::List { page } => Reply::Accounts(
                ACCOUNTS
                    .range(ctx, &page, height)?
                    .map(|(_, account)| account),
            ),
            Query::Controlled { by, page } => Reply::Accounts(
                CONTROLLED
                    .range_of(ctx, &by, &page, height)?
                    .try_map(|(_, number)| account(ctx, number))?,
            ),
        })
    }
}

#[cfg(feature = "module")]
guest::export!(Identity);
