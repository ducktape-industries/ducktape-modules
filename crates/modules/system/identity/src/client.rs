//! Identity for a sibling module: the queries this module answers, as
//! functions over [`Ctx`]. A caller names the identity module by id and
//! never touches the bytes or the reply enum.
use sdk::{AccountNumber, Ctx, Error, refusal};

use crate::{
    AccountRef, AccountView, IdentityQuery, IdentityReply, MAX_QUERY_LIMIT, decode_reply,
    encode_query,
};

/// The account holding `key`, if identity knows one.
pub async fn account_of_key(
    ctx: &dyn Ctx,
    identity: &str,
    key: &[u8],
) -> Result<Option<AccountView>, Error> {
    match ask(ctx, identity, &IdentityQuery::OfKey { key: key.to_vec() }).await? {
        IdentityReply::Account(account) => Ok(account),
        other => Err(unexpected(other)),
    }
}

/// Account `number`, if it exists. Identity numbers accounts from 1.
pub async fn account(
    ctx: &dyn Ctx,
    identity: &str,
    number: AccountNumber,
) -> Result<Option<AccountView>, Error> {
    if number == 0 {
        return Ok(None);
    }
    match ask(ctx, identity, &IdentityQuery::Get { number }).await? {
        IdentityReply::Account(account) => Ok(account),
        other => Err(unexpected(other)),
    }
}

/// Each reference as the account it names (an account that exists, or the
/// account holding a key), in order; `None` where it names none. Chunked
/// at [`MAX_QUERY_LIMIT`], so any length goes.
pub async fn resolve(
    ctx: &dyn Ctx,
    identity: &str,
    references: &[AccountRef],
) -> Result<Vec<Option<AccountNumber>>, Error> {
    let mut numbers = Vec::with_capacity(references.len());
    for chunk in references.chunks(MAX_QUERY_LIMIT as usize) {
        let query = IdentityQuery::Resolve {
            references: chunk.to_vec(),
        };
        let resolved = match ask(ctx, identity, &query).await? {
            IdentityReply::Resolved(resolved) => resolved,
            other => return Err(unexpected(other)),
        };
        if resolved.len() != chunk.len() {
            return Err(Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence: format!(
                    "identity resolved {} of {} references",
                    resolved.len(),
                    chunk.len()
                ),
            });
        }
        numbers.extend(resolved);
    }
    Ok(numbers)
}

/// One page of accounts from number `from`, at most `limit` (capped at
/// [`MAX_QUERY_LIMIT`]), ascending.
pub async fn accounts(
    ctx: &dyn Ctx,
    identity: &str,
    from: u64,
    limit: u64,
) -> Result<Vec<AccountView>, Error> {
    let query = IdentityQuery::All {
        from,
        limit: limit.min(MAX_QUERY_LIMIT),
    };
    match ask(ctx, identity, &query).await? {
        IdentityReply::Accounts(accounts) => Ok(accounts),
        other => Err(unexpected(other)),
    }
}

async fn ask(ctx: &dyn Ctx, identity: &str, query: &IdentityQuery) -> Result<IdentityReply, Error> {
    let bytes = ctx.query(identity, &encode_query(query)).await?;
    decode_reply(&bytes).map_err(|sentence| Error::Module {
        reason: refusal::UNEXPECTED_REPLY.into(),
        sentence,
    })
}

fn unexpected(reply: IdentityReply) -> Error {
    let kind = match reply {
        IdentityReply::Accounts(_) => "accounts",
        IdentityReply::Account(_) => "an account",
        IdentityReply::Resolved(_) => "resolutions",
        IdentityReply::Gen(_) => "a generation",
    };
    Error::Module {
        reason: refusal::UNEXPECTED_REPLY.into(),
        sentence: format!("identity answered another question with {kind}"),
    }
}
