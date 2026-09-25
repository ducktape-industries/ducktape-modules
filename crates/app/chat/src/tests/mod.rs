//! Every op, once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is the [`crate::Chat`] module over a
//! [`MockHost`] whose identity holds each account's key, and the accounts
//! that act on it.
use guest::{Cause, Env, Origin, code};
use guest::{ExecCtx, MockHost, Module, QueryCtx};

use crate::{Op, PageRequest, PageResponse, PostPolicy, Principal, Query, Reply, parse_message};

mod channels;
mod messages;
mod origin;
mod properties;
mod reactions;
mod rooms;

const ADA: Principal = Principal::Account(1);
const BO: Principal = Principal::Account(2);
const CY: Principal = Principal::Account(3);

/// A chat store and a clock: every op runs one block later.
struct Chat {
    store: MockHost,
    height: u64,
}

impl Default for Chat {
    /// An empty store beside an identity where account `n` holds the key
    /// `n.to_be_bytes()` ([`signer`]), and a verifier that takes every node
    /// proof (`origin.rs` checks the proof itself).
    fn default() -> Chat {
        let store = MockHost::default();
        store.borrow_mut().verifier = Some(Box::new(|_, _, _, _, _| true));
        store.borrow_mut().siblings.insert(
            identity::MODULE.into(),
            Box::new(|request| {
                let identity::Query::OfKey { key } = abi::decode(request)? else {
                    panic!("the harness answers identity's OfKey only");
                };
                let number = <[u8; 8]>::try_from(key.as_slice()).map(u64::from_be_bytes);
                Ok(abi::encode(&identity::Reply::Number(number.ok())))
            }),
        );
        Chat { store, height: 0 }
    }
}

/// The origin that acts as `who`: an account's key, the module, the system.
fn signer(who: &Principal) -> Origin {
    match who {
        Principal::Account(number) => Origin::Signed(number.to_be_bytes().to_vec()),
        Principal::Module(module) => Origin::Module(module.clone()),
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

    fn env(&self, origin: Origin) -> Env {
        Env {
            chain_id: vec![],
            height: self.height,
            time: self.height * 1000,
            module: crate::MODULE.into(),
            origin,
            cause: Cause::Direct,
        }
    }

    /// The context of the next block, signed by `who`.
    fn next(&mut self, who: &Principal) -> ExecCtx {
        self.height += 1;
        self.store.exec(self.env(signer(who)))
    }

    /// A read at the current block.
    fn reads(&self) -> QueryCtx {
        self.store.query(self.env(Origin::Root))
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
