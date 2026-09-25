//! Chat owns conversations: forge opens a change's channel, posts its
//! system lines into it, and asks chat about the replies a review drew.

use chat::{Block, MsgRow, Op, PostPolicy, Query, Reply};
use guest::{Error, code};
use guest::{ExecCtx, QueryCtx};
use identity::Principal;

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
pub fn create(ctx: &ExecCtx, repo: &str, change: &Change) {
    emit(
        ctx,
        Op::CreateChannel {
            channel_id: change.channel.clone(),
            name: format!("{repo}#{}", change.n),
            post_policy: PostPolicy::Open,
        },
    );
}

/// Queues one system line into the change's channel: the event's code in
/// a `forge` block, no name and no sentence.
pub fn post(ctx: &ExecCtx, change: &Change, message_id: String, event: Event) {
    emit(
        ctx,
        Op::PostMessage {
            channel_id: change.channel.clone(),
            message_id,
            blocks: vec![Block::Code {
                lang: Some(crate::MODULE.into()),
                text: event.code(),
            }],
            thread: None,
        },
    );
}

fn emit(ctx: &ExecCtx, message: Op) {
    ctx.send(chat::MODULE, abi::encode(&message));
}

/// The chat root a review posted, by its message id.
pub fn message(ctx: &QueryCtx, id: &str) -> Result<Option<MsgRow>, Error> {
    let query = Query::MessageById {
        message_id: id.into(),
    };
    match ctx.ask::<Query, Reply>(chat::MODULE, &query)? {
        Reply::Message(row) => Ok(row),
        other => Err(unexpected("MessageById", &other)),
    }
}

/// The newest thread in `channel` that `principal` started and someone answered.
pub fn attention(
    ctx: &QueryCtx,
    channel: &str,
    principal: &Principal,
) -> Result<Option<MsgRow>, Error> {
    let query = Query::ThreadAttention {
        channel_id: channel.into(),
        author: principal.clone(),
    };
    match ctx.ask::<Query, Reply>(chat::MODULE, &query)? {
        Reply::Attention(row) => Ok(row),
        other => Err(unexpected("ThreadAttention", &other)),
    }
}

fn unexpected(asked: &str, reply: &Reply) -> Error {
    Error::new(
        code::UNEXPECTED_REPLY,
        format!("chat answered {asked} with {reply:?}"),
    )
}
