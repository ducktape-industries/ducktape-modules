//! Who is asking, and what identity says about them: an origin resolved to
//! a [`Party`], a huddle join's node proof checked, and identity's roster
//! as chat's views read it. The wasm32 program is glue over
//! [`execute_from`] and [`query`](crate::query); both run natively over
//! [`store::Memory`] with an identity sibling and a verifier.
use abi::{Env, Origin, Refusal, Scheme, reason};
use store::{Page, PageReply, Reads, Writes, invalid, unauthorized};

use crate::{AccountRow, Frame, HUDDLE_JOIN_NS, Op, Party};

/// An op as it arrives: from an origin at a height. A huddle join's node
/// proof is checked first, then the origin acts as its party.
pub fn execute_from(store: &mut impl Writes, env: &Env, op: Op) -> Result<(), Refusal> {
    if let Op::JoinHuddle {
        channel_id,
        node,
        node_proof,
    } = &op
    {
        node_consents(store, &env.origin, channel_id, node, node_proof)?;
    }
    let frame = Frame {
        party: party_of(store, &env.origin)?,
        height: env.height,
        time: env.time,
    };
    crate::execute(store, &frame, op)
}

/// Who an origin is: a key is the account identity says holds it, or
/// itself while it holds none (or identity is not deployed). Every program
/// that names people by [`Party`] (chat, forge) resolves its signer here.
pub fn party_of(store: &impl Reads, origin: &Origin) -> Result<Party, Refusal> {
    Ok(match origin {
        Origin::External(key) if key.is_empty() => {
            return Err(invalid("an external origin carries a key"));
        }
        Origin::External(key) => match identity::account_of(store, key) {
            Ok(Some(number)) => Party::Account(number),
            Ok(None) => Party::Key(key.clone()),
            Err(r) if r.reason == reason::UNKNOWN_PROGRAM => Party::Key(key.clone()),
            Err(r) => return Err(r),
        },
        Origin::Program(id) => Party::Module(id.clone()),
        Origin::System => Party::System,
    })
}

/// A huddle seat names a node, and the node signed its consent to seat
/// this key in this channel.
fn node_consents(
    store: &impl Reads,
    origin: &Origin,
    channel_id: &str,
    node: &[u8],
    proof: &[u8],
) -> Result<(), Refusal> {
    let Origin::External(key) = origin else {
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
/// program.
pub(crate) fn accounts(store: &impl Reads, page: Page) -> Result<PageReply<AccountRow>, Refusal> {
    let list = identity::Query::List { page };
    let identity::Reply::Accounts(accounts) =
        store.ask::<identity::Query, identity::Reply>(identity::PROGRAM, &list)?
    else {
        return Err(Refusal::new(
            reason::UNEXPECTED_REPLY,
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
