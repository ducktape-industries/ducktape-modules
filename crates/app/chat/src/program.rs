// The wasm32 program over the rules: the origin resolved to a party through
// identity, a huddle join's node proof checked, then `execute`/`query`.

use abi::{Env, Origin, Refusal, Scheme, reason};
use guest::{Execute, Program, Query as QueryCtx};
use store::{Page, Reads, decoded, invalid, unauthorized};

use crate::{AccountRow, Frame, HUDDLE_JOIN_NS, MsgRow, Op, PROGRAM, Party, Query, Reply};

struct Chat;

impl Program for Chat {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let op = decoded::<Op>(PROGRAM, "Op", payload)?;
        if let Op::JoinHuddle {
            channel_id,
            node,
            node_proof,
        } = &op
        {
            node_consents(ctx, env, channel_id, node, node_proof)?;
        }
        let frame = Frame {
            party: party_of(ctx, &env.origin)?,
            height: env.height,
            time: env.time,
        };
        crate::execute(ctx, &frame, op)
    }

    fn query(ctx: &mut QueryCtx, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = match decoded::<Query>(PROGRAM, "Query", request)? {
            Query::Accounts { page } => accounts(ctx, page)?,
            Query::ThreadAttention {
                channel_id,
                author: Party::Key(key),
            } => attention_of_key(ctx, env, channel_id, key)?,
            query => crate::query(ctx, env.height, query)?,
        };
        ctx.reply(&reply);
        Ok(())
    }
}

guest::program!(Chat);

/// Who an origin is to chat: a key is the account identity says holds it,
/// or itself while it holds none (or identity is not deployed).
fn party_of(ctx: &impl Reads, origin: &Origin) -> Result<Party, Refusal> {
    Ok(match origin {
        Origin::External(key) if key.is_empty() => {
            return Err(invalid("an external origin carries a key"));
        }
        Origin::External(key) => match identity::account_of(ctx, key) {
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
    ctx: &impl Reads,
    env: &Env,
    channel_id: &str,
    node: &[u8],
    proof: &[u8],
) -> Result<(), Refusal> {
    let Origin::External(key) = &env.origin else {
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

/// Identity's roster as the view reads it, so a view links one program.
fn accounts(ctx: &impl Reads, page: Page) -> Result<Reply, Refusal> {
    let list = identity::Query::List { page };
    let identity::Reply::Accounts(accounts) =
        ctx.ask::<identity::Query, identity::Reply>(identity::PROGRAM, &list)?
    else {
        return Err(Refusal::new(
            reason::UNEXPECTED_REPLY,
            "identity answered List with something else",
        ));
    };
    let row = |account: identity::Account| AccountRow {
        number: account.number,
        program: matches!(account.control, identity::Control::Program { .. }),
        keys: account.keys().iter().map(|k| crate::hex(&k.key)).collect(),
        name: account.name,
    };
    Ok(Reply::Accounts(accounts.map(row)))
}

/// A key may have posted before or after it gained an account: the newer
/// answered thread of the two.
fn attention_of_key(
    ctx: &impl Reads,
    env: &Env,
    channel_id: String,
    key: Vec<u8>,
) -> Result<Reply, Refusal> {
    let account = party_of(ctx, &Origin::External(key.clone()))?;
    let mut newest: Option<MsgRow> = None;
    for author in [Party::Key(key), account] {
        let asked = Query::ThreadAttention {
            channel_id: channel_id.clone(),
            author,
        };
        let Reply::Attention(Some(row)) = crate::query(ctx, env.height, asked)? else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|old| old.last_reply_seq < row.last_reply_seq)
        {
            newest = Some(row);
        }
    }
    Ok(Reply::Attention(newest))
}
