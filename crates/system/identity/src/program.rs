//! The module: every op and every query, each handed to its rule.

use guest::{Error, ExecCtx, Module, QueryCtx};

use crate::rules::{
    ACCOUNTS, MANAGED, OF_MODULE, account, add_key, create, create_agent, generation, of_key,
    profiles, register_module, remove_key, resolve, set_name, set_profile, set_status,
    transfer_manager,
};
use crate::{Op, Query, Reply};

pub struct Identity;

impl Module for Identity {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::RegisterModule { module } => register_module(ctx, module),
            Op::Create { name, scheme } => create(ctx, name, scheme),
            Op::CreateAgent { name } => create_agent(ctx, name),
            Op::AddKey {
                scheme,
                label,
                consent,
            } => add_key(ctx, scheme, label, consent),
            Op::RemoveKey { account, key } => remove_key(ctx, account, &key),
            Op::SetName { account, name } => set_name(ctx, account, name),
            Op::SetProfile {
                account,
                avatar,
                bio,
            } => set_profile(ctx, account, avatar, bio),
            Op::SetStatus { account, status } => set_status(ctx, account, status),
            Op::TransferManager { account, to } => transfer_manager(ctx, account, to),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        let height = ctx.env().height;
        Ok(match query {
            Query::OfKey { key } => Reply::Number(of_key(ctx, &key)?),
            Query::Profiles { after, limit } => profiles(ctx, after, limit)?,
            Query::OfModule { module } => Reply::Number(OF_MODULE.get(ctx, &module)?),
            Query::Get { number } => Reply::Account(ACCOUNTS.get(ctx, &number)?),
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
            Query::Managed { by, page } => Reply::Accounts(
                MANAGED
                    .range_of(ctx, &by, &page, height)?
                    .try_map(|(_, number)| account(ctx, number))?,
            ),
        })
    }
}

#[cfg(feature = "module")]
guest::export!(Identity);
