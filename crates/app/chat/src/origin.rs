//! What the host and the identity role say about who is asking: a huddle
//! join's node proof checked, and the role's profiles as chat's views read
//! them. The sender itself is the host's, read first thing in
//! [`Chat::execute`](crate::Chat).
use abi::role::identity as role;
use guest::{Error, QueryCtx, invalid, unauthorized};
use guest::{Origin, Scheme, code};
use store::{PageRequest, PageResponse};

use crate::{HUDDLE_JOIN_NS, Profile};

/// A huddle seat names a node, and the node signed its consent to seat
/// this key in this channel.
pub(crate) fn node_consents(
    ctx: &QueryCtx,
    origin: &Origin,
    channel_id: &str,
    node: &[u8],
    proof: &[u8],
) -> Result<(), Error> {
    let Origin::Signed(key) = origin else {
        return Err(unauthorized("only a key joins a huddle"));
    };
    let message = [channel_id.as_bytes(), key].concat();
    let signed = ctx.verify(
        Scheme::Ed25519,
        node.to_vec(),
        HUDDLE_JOIN_NS,
        message,
        proof.to_vec(),
    )?;
    if !signed {
        return Err(invalid("the node proof does not verify"));
    }
    Ok(())
}

/// One page of the identity role's profiles (asked of the module genesis
/// bound to the role), so a view links one module. The cursor is the last
/// account number, big-endian.
pub(crate) fn accounts(ctx: &QueryCtx, page: PageRequest) -> Result<PageResponse<Profile>, Error> {
    let after = page
        .after
        .as_deref()
        .map(|bytes| <[u8; 8]>::try_from(bytes).map(u64::from_be_bytes))
        .transpose()
        .map_err(|_| invalid("an account cursor is an account number"))?;
    let profiles = role::Query::Profiles {
        after,
        limit: page.limit() as u32,
    };
    let role::Reply::Profiles { profiles, next } =
        ctx.query::<role::Query, role::Reply>(&ctx.env().roles.identity, &profiles)?
    else {
        return Err(Error::new(
            code::UNEXPECTED_REPLY,
            "identity answered Profiles with something else",
        ));
    };
    Ok(PageResponse {
        height: ctx.env().height,
        items: profiles,
        next: next.map(|number| number.to_be_bytes().to_vec()),
    })
}
