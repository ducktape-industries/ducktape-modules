//! Every op, once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is a [`Memory`] store and the
//! accounts that act on it.
use abi::reason;
use store::Memory;

use crate::{
    Op, Page, PageReply, PostPolicy, Principal, Query, Reply, execute, parse_message, query,
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

/// A chat store and a clock: every op runs one block later.
#[derive(Default)]
struct Chat {
    store: Memory,
    height: u64,
}

impl Chat {
    /// A store holding `#general`, owned by Ada, open or members-only.
    fn with_channel(post_policy: PostPolicy) -> Chat {
        let mut chat = Chat::default();
        chat.ok(&ADA, create("general", post_policy));
        chat
    }

    /// The frame of the next block.
    fn next(&mut self, who: &Principal) -> crate::Frame {
        self.height += 1;
        crate::Frame {
            principal: who.clone(),
            height: self.height,
            time: self.height * 1000,
        }
    }

    fn run(&mut self, who: &Principal, op: Op) -> Result<(), abi::Refusal> {
        let frame = self.next(who);
        execute(&mut self.store, &frame, op)
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
        let frame = self.next(who);
        self.store
            .refused(|store| execute(store, &frame, op))
            .reason
    }

    fn post(&mut self, who: &Principal, id: &str, text: &str, thread: Option<u64>) {
        self.ok(who, post("general", id, text, thread));
    }

    fn ask(&self, question: Query) -> Reply {
        query(&self.store, self.height, question).unwrap()
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
            page: Page::default(),
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
