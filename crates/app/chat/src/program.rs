// The wasm32 program over the rules: guest contexts as the store, the origin resolved through identity, the huddle node proof checked, then `execute`/`query`.

use crate::{
    AccountRow, ChatMsg, ChatViewQuery, ChatViewReply, Frame, HUDDLE_JOIN_NS, MsgRow, Party,
};
use abi::{Entry, Env, Origin, Refusal, Scan, Scheme, reason};
use guest::{Execute, Program, Query, Reads};
use module_registry::Page;

struct Reader<'a>(&'a Query);

impl crate::Read for Reader<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key)
    }
    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.0.scan(scan)
    }
}

struct Writer<'a>(&'a mut Execute);

impl crate::Read for Writer<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.get(key)
    }
    fn scan(&self, scan: Scan) -> Vec<Entry> {
        self.0.scan(scan)
    }
}

impl crate::Write for Writer<'_> {
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.0.set(key, value)
    }
    fn delete(&mut self, key: &[u8]) {
        self.0.delete(key.to_vec())
    }
}

fn bad(e: impl ToString) -> Refusal {
    Refusal::new(reason::INVALID_INPUT, e.to_string())
}

fn party_of(ctx: &impl Reads, origin: &Origin) -> Result<Party, Refusal> {
    Ok(match origin {
        Origin::External(key) if key.is_empty() => {
            return Err(bad("an external origin carries a key"));
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

fn node_joins(ctx: &impl Reads, env: &Env, msg: &ChatMsg) -> Result<(), Refusal> {
    let ChatMsg::JoinHuddle {
        channel_id,
        node,
        node_proof,
    } = msg
    else {
        return Ok(());
    };
    let Origin::External(key) = &env.origin else {
        return Err(Refusal::new(
            reason::UNAUTHORIZED,
            "only a key joins a huddle",
        ));
    };
    let node_consents = ctx.verify(
        Scheme::Ed25519,
        node.clone(),
        HUDDLE_JOIN_NS,
        [channel_id.as_bytes(), key].concat(),
        node_proof.clone(),
    )?;
    if !node_consents {
        return Err(bad("the node proof does not verify"));
    }
    Ok(())
}

/// Identity's roster as the view reads it: one door, chat's.
fn accounts(ctx: &impl Reads, limit: Option<usize>) -> Result<ChatViewReply, Refusal> {
    // The chat roster asks for one bounded page.
    let page = Page {
        after: None,
        limit: Some(crate::page(limit) as u64),
    };
    let identity::Reply::Accounts(accounts) = ctx.ask::<identity::Query, identity::Reply>(
        identity::PROGRAM,
        &identity::Query::List { page },
    )?
    else {
        return Err(Refusal::new(
            reason::UNEXPECTED_REPLY,
            "identity answered List with something else",
        ));
    };
    Ok(ChatViewReply::Accounts(
        accounts
            .items
            .into_iter()
            .map(|a| AccountRow {
                number: a.number,
                program: matches!(a.control, identity::Control::Program { .. }),
                keys: a.keys().iter().map(|k| crate::hex(&k.key)).collect(),
                name: a.name,
            })
            .collect(),
    ))
}

struct Chat;

impl Program for Chat {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        let msg: ChatMsg = serde_json::from_slice(payload).map_err(bad)?;
        node_joins(ctx, env, &msg)?;
        let frame = Frame {
            party: party_of(ctx, &env.origin)?,
            height: env.height,
            time: env.time,
        };
        crate::execute(&mut Writer(ctx), &frame, msg)
    }

    fn query(ctx: &mut Query, _env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let q: ChatViewQuery = serde_json::from_slice(request).map_err(bad)?;
        let reply = match q {
            ChatViewQuery::Accounts { limit } => accounts(ctx, limit)?,
            ChatViewQuery::ThreadAttention {
                channel_id,
                author: Party::Key(key),
            } => {
                // A key may have posted before or after acquiring an account.
                let resolved = party_of(ctx, &Origin::External(key.clone()))?;
                let mut newest = None;
                for author in [Party::Key(key), resolved] {
                    let reply = crate::query(
                        &Reader(ctx),
                        ChatViewQuery::ThreadAttention {
                            channel_id: channel_id.clone(),
                            author,
                        },
                    )?;
                    if let ChatViewReply::Attention(Some(row)) = reply
                        && newest
                            .as_ref()
                            .is_none_or(|old: &MsgRow| old.last_reply_seq < row.last_reply_seq)
                    {
                        newest = Some(row);
                    }
                }
                ChatViewReply::Attention(newest)
            }
            q => crate::query(&Reader(ctx), q)?,
        };
        ctx.respond(serde_json::to_vec(&reply).expect("a reply serializes"));
        Ok(())
    }
}

guest::program!(Chat);
