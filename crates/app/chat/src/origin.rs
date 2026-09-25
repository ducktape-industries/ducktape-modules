//! Who is asking, and what identity says about them: an origin resolved to
//! a [`Principal`](crate::Principal) by identity's one rule, a huddle join's node
//! proof checked, and identity's roster as chat's views read it. The wasm32 module is glue over
//! [`execute_from`] and [`query`](crate::query); both run natively over
//! [`store::testing::MockHost`] with an identity sibling and a verifier.
use store::{Error, Origin, PageRequest, PageResponse, Reads, Scheme, code, invalid, unauthorized};

use crate::{AccountRow, HUDDLE_JOIN_NS};

/// A huddle seat names a node, and the node signed its consent to seat
/// this key in this channel.
pub(crate) fn node_consents(
    store: &impl Reads,
    origin: &Origin,
    channel_id: &str,
    node: &[u8],
    proof: &[u8],
) -> Result<(), Error> {
    let Origin::Signed(key) = origin else {
        return Err(unauthorized("only a key joins a huddle"));
    };
    let message = [channel_id.as_bytes(), key].concat();
    let signed = store.verify(
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

/// One page of identity's roster as a view reads it, so a view links one
/// module.
pub(crate) fn accounts(
    store: &impl Reads,
    page: PageRequest,
) -> Result<PageResponse<AccountRow>, Error> {
    let list = identity::Query::List { page };
    let identity::Reply::Accounts(accounts) =
        store.ask::<identity::Query, identity::Reply>(identity::MODULE, &list)?
    else {
        return Err(Error::new(
            code::UNEXPECTED_REPLY,
            "identity answered List with something else",
        ));
    };
    Ok(accounts.map(|account| AccountRow {
        number: account.number,
        program: matches!(account.control, identity::Control::Program { .. }),
        keys: account.keys().iter().map(|k| crate::hex(&k.key)).collect(),
        name: account.name,
    }))
}
