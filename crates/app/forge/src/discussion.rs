//! Chat owns conversations: forge opens a change's channel, posts its
//! system lines into it, and asks chat about the replies a review drew.

use abi::{Refusal, reason};
use chat::{Block, MsgRow, Op, PostPolicy, Query, Reply};
use identity::Party;
use store::{Reads, Writes};

use crate::Change;

/// What a system line marks in a change's timeline. The line carries this
/// event's code and nothing else: who acted and what they concluded live
/// on forge's records, which the view words in the reader's language.
pub enum Event {
    Opened,
    Closed,
    Merged,
    /// The review of this id; its line is the root its discussion hangs on.
    Reviewed(u64),
}

impl Event {
    /// The code a `forge` block carries: `opened`, `closed`, `merged` or
    /// `review <id>`.
    fn code(&self) -> String {
        match self {
            Event::Opened => "opened".into(),
            Event::Closed => "closed".into(),
            Event::Merged => "merged".into(),
            Event::Reviewed(id) => format!("review {id}"),
        }
    }
}

/// Queues the change's channel. Chat creates it in the next block.
pub fn create(store: &mut impl Writes, repo: &str, change: &Change) {
    emit(
        store,
        Op::CreateChannel {
            channel_id: change.channel.clone(),
            name: format!("{repo}#{}", change.n),
            post_policy: PostPolicy::Open,
        },
    );
}

/// Queues one system line into the change's channel: the event's code in
/// a `forge` block, no name and no sentence.
pub fn post(store: &mut impl Writes, change: &Change, message_id: String, event: Event) {
    emit(
        store,
        Op::PostMessage {
            channel_id: change.channel.clone(),
            message_id,
            blocks: vec![Block::Code {
                lang: Some(crate::PROGRAM.into()),
                text: event.code(),
            }],
            thread: None,
        },
    );
}

fn emit(store: &mut impl Writes, message: Op) {
    store.emit(chat::PROGRAM, abi::encode(&message));
}

/// The chat root a review posted, by its message id.
pub fn message(store: &impl Reads, id: &str) -> Result<Option<MsgRow>, Refusal> {
    let query = Query::MessageById {
        message_id: id.into(),
    };
    match store.ask::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Message(row) => Ok(row),
        other => Err(unexpected("MessageById", &other)),
    }
}

/// The newest thread in `channel` that `party` started and someone answered.
pub fn attention(
    store: &impl Reads,
    channel: &str,
    party: &Party,
) -> Result<Option<MsgRow>, Refusal> {
    let query = Query::ThreadAttention {
        channel_id: channel.into(),
        author: party.clone(),
    };
    match store.ask::<Query, Reply>(chat::PROGRAM, &query)? {
        Reply::Attention(row) => Ok(row),
        other => Err(unexpected("ThreadAttention", &other)),
    }
}

fn unexpected(asked: &str, reply: &Reply) -> Refusal {
    Refusal::new(
        reason::UNEXPECTED_REPLY,
        format!("chat answered {asked} with {reply:?}"),
    )
}
