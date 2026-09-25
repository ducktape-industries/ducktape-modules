//! Every op, once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is a [`MockHost`] store and the
//! accounts that act on it.
use store::code;
use store::testing::{MockHost, env};
use store::{Env, Origin};

use crate::{
    Op, PageRequest, PageResponse, PostPolicy, Principal, Query, Reply, execute, parse_message,
    query,
};

mod channels;
mod messages;
mod origin;
mod properties;
mod reactions;
mod rooms;

const ADA: Principal = Principal::Account(1);
const BO: Principal = Principal::Account(2);
const CY: Principal = Principal::Account(3);

/// A chat store and a clock: every op runs one block later, signed by a key
/// whose node proofs all verify (`origin.rs` checks the proof itself).
struct Chat {
    store: MockHost,
    height: u64,
}

impl Default for Chat {
    fn default() -> Chat {
        let store = MockHost {
            verifier: Some(Box::new(|_, _, _, _, _| true)),
            ..MockHost::default()
        };
        Chat { store, height: 0 }
    }
}

impl Chat {
    /// A store holding `#general`, owned by Ada, open or members-only.
    fn with_channel(post_policy: PostPolicy) -> Chat {
        let mut chat = Chat::default();
        chat.ok(&ADA, create("general", post_policy));
        chat
    }

    /// The env of the next block.
    fn next(&mut self) -> Env {
        self.height += 1;
        Env {
            height: self.height,
            time: self.height * 1000,
            ..env(Origin::Signed(b"key".to_vec()))
        }
    }

    fn run(&mut self, who: &Principal, op: Op) -> Result<(), store::Error> {
        let env = self.next();
        execute(&mut self.store, &env, who.clone(), op)
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
        let env = self.next();
        self.store
            .refused(|store| execute(store, &env, who.clone(), op))
            .code
    }

    /// The env of the current block, for a query.
    fn env(&self) -> Env {
        Env {
            height: self.height,
            ..env(Origin::Root)
        }
    }

    fn post(&mut self, who: &Principal, id: &str, text: &str, thread: Option<u64>) {
        self.ok(who, post("general", id, text, thread));
    }

    fn ask(&self, question: Query) -> Reply {
        query(&self.store, &self.env(), question).unwrap()
    }

    fn message(&self, seq: u64) -> crate::MsgRow {
        crate::state::message(&self.store, "general", seq).unwrap()
    }

    fn channel(&self) -> crate::ChannelRow {
        crate::state::channel(&self.store, "general").unwrap()
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
