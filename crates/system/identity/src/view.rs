//! What a view asks identity for on its own: the seated key `host.props`
//! hands it (lowercase hex, no `acct:`/`user:` prefix — the host is
//! program-agnostic and does not know whether that key holds an account)
//! resolved to the account it holds, if any. chat-view and forge-view both
//! gate writes on this, so it lives once here instead of duplicated per
//! view — mirrors `settings-view`'s own `read_account`.
use ducktape_view_guest::Host;
use ducktape_view_guest::doors::{Program, Query};
use ducktape_view_guest::host::{Refusal, malformed};

use crate::{AccountNumber, Op, Query as IdentityQuery, Reply};

pub struct Identity;
impl Program for Identity {
    const NAME: &'static str = crate::PROGRAM;
    type Op = Op;
    type Query = IdentityQuery;
    type Reply = Reply;
}

/// The account `key` holds, or `None` for an empty or unregistered key. A
/// non-empty key that is not hexadecimal is a host contract violation, not
/// a quiet "no account".
pub async fn account_of_key(host: &Host, key: &str) -> Result<Option<AccountNumber>, Refusal> {
    if key.is_empty() {
        return Ok(None);
    }
    if !key.len().is_multiple_of(2) || !key.is_ascii() {
        return Err(malformed("session key is not hexadecimal".into()));
    }
    let key = (0..key.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&key[i..i + 2], 16).map_err(|e| malformed(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    match host
        .ask::<Query<Identity>>(IdentityQuery::OfKey { key })
        .await?
    {
        Reply::Number(number) => Ok(number),
        other => Err(malformed(format!("identity answered OfKey with {other:?}"))),
    }
}
