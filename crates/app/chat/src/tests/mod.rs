//! Every op, once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is the [`crate::Chat`] module over a
//! [`MockHost`], and the principals that act on it, each the sender of the
//! env it signs.
use guest::{Cause, Env, Origin, code};
use guest::{ExecCtx, MockHost, Module, QueryCtx};

use crate::{Op, PageRequest, PostPolicy, Principal, Query, Reply, parse_message};

mod channels;
mod messages;
mod origin;
mod properties;
mod reactions;
mod rooms;

const ADA: Principal = Principal::Account(1);
const BO: Principal = Principal::Account(2);
const CY: Principal = Principal::Account(3);
/// forge's account; `for` and `bot` are modules too ([`module_of`]).
const FORGE: Principal = Principal::Account(900);

/// The module an account is the account of, as identity registered it.
fn module_of(who: &Principal) -> Option<&'static str> {
    match who {
        Principal::Account(900) => Some("forge"),
        Principal::Account(901) => Some("for"),
        Principal::Account(902) => Some("bot"),
        _ => None,
    }
}

/// A chat store and a clock: every op runs one block later.
struct Chat {
    store: MockHost,
    height: u64,
}

impl Default for Chat {
    /// An empty store and a verifier that takes every node proof
    /// (`origin.rs` checks the proof itself).
    fn default() -> Chat {
        let store = MockHost::default();
        store.borrow_mut().verifier = Some(Box::new(|_, _, _, _, _| true));
        Chat { store, height: 0 }
    }
}

/// The origin that acts as `who`: a module's own frame, an account's key
/// (account `n` holds `n.to_be_bytes()`), the system.
fn signer(who: &Principal) -> Origin {
    if let Some(module) = module_of(who) {
        return Origin::Module(module.into());
    }
    match who {
        Principal::Account(number) => Origin::Signed(number.to_be_bytes().to_vec()),
        Principal::Root => Origin::Root,
    }
}

impl Chat {
    /// A store holding `#general`, owned by Ada, open or members-only.
    fn with_channel(post_policy: PostPolicy) -> Chat {
        let mut chat = Chat::default();
        chat.ok(&ADA, create("general", post_policy));
        chat
    }

    fn env(&self, origin: Origin, sender: Option<Principal>) -> Env {
        Env {
            chain_id: vec![],
            height: self.height,
            time: self.height * 1000,
            module: crate::MODULE.into(),
            origin,
            sender,
            roles: guest::MockHost::roles(),
            cause: Cause::Direct,
        }
    }

    /// The context of the next block, signed by `who`.
    fn next(&mut self, who: &Principal) -> ExecCtx {
        self.height += 1;
        self.store.exec(self.env(signer(who), Some(who.clone())))
    }

    /// A read at the current block.
    fn reads(&self) -> QueryCtx {
        self.store.query(self.env(Origin::Root, None))
    }

    fn run(&mut self, who: &Principal, op: Op) -> Result<(), guest::Error> {
        let ctx = self.next(who);
        crate::Chat::execute(&ctx, op)
    }

    #[track_caller]
    fn ok(&mut self, who: &Principal, op: Op) {
        if let Err(refusal) = self.run(who, op) {
            panic!("refused: {refusal}");
        }
    }

    /// The refusal's reason; the store is untouched by it.
    #[track_caller]
    fn refused(&mut self, who: &Principal, op: Op) -> String {
        let ctx = self.next(who);
        self.store.refused(|| crate::Chat::execute(&ctx, op)).code
    }

    fn post(&mut self, who: &Principal, id: &str, text: &str, thread: Option<u64>) {
        self.ok(who, post("general", id, text, thread));
    }

    fn ask(&self, question: Query) -> Reply {
        crate::Chat::query(&self.reads(), question).unwrap()
    }

    fn message(&self, seq: u64) -> crate::MsgRow {
        crate::state::message(&self.reads(), "general", seq).unwrap()
    }

    fn channel(&self) -> crate::ChannelRow {
        crate::state::channel(&self.reads(), "general").unwrap()
    }

    /// The seqs a search for `text` finds, newest first.
    fn search(&self, text: &str) -> Vec<u64> {
        let Reply::Hits(hits) = self.ask(Query::Search {
            text: text.into(),
            viewer: vec![],
            channel_id: None,
            page: PageRequest::default(),
        }) else {
            panic!("a search answers hits");
        };
        hits.hits.iter().map(|row| row.seq).collect()
    }
}

fn create(id: &str, post_policy: PostPolicy) -> Op {
    Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy,
    }
}

fn post(channel: &str, id: &str, text: &str, thread: Option<u64>) -> Op {
    Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: parse_message(text),
        thread,
    }
}

fn react(seq: u64, emoji: &str, on: bool) -> Op {
    let (channel_id, emoji) = ("general".to_string(), emoji.to_string());
    if on {
        Op::AddReaction {
            channel_id,
            seq,
            emoji,
        }
    } else {
        Op::RemoveReaction {
            channel_id,
            seq,
            emoji,
        }
    }
}

fn delete(seq: u64) -> Op {
    Op::DeleteMessage {
        channel_id: "general".into(),
        seq,
    }
}
