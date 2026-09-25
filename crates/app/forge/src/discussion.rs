// Chat owns conversations: forge opens a change's channel, posts its system lines into it, and asks chat about the replies a review drew.

use abi::{Refusal, reason};
use chat::{Block, MsgRow, Op, Party, PostPolicy, Query, Reply};
use store::{Reads, Writes};

use crate::Change;
use crate::state::storage;

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

/// Queues one system line into the change's channel.
pub fn post(store: &mut impl Writes, change: &Change, message_id: String, text: String) {
    emit(
        store,
        Op::PostMessage {
            channel_id: change.channel.clone(),
            message_id,
            blocks: vec![Block::paragraph(text)],
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
    match ask(store, &query)? {
        Reply::Message(row) => Ok(row),
        _ => Err(unexpected("chat must answer MessageById with Message")),
    }
}

/// The newest thread in `channel` that `key` started and someone answered.
pub fn attention(store: &impl Reads, channel: &str, key: &[u8]) -> Result<Option<MsgRow>, Refusal> {
    let query = Query::ThreadAttention {
        channel_id: channel.into(),
        author: Party::Key(key.to_vec()),
    };
    match ask(store, &query)? {
        Reply::Attention(row) => Ok(row),
        _ => Err(unexpected(
            "chat must answer ThreadAttention with Attention",
        )),
    }
}

fn ask(store: &impl Reads, query: &Query) -> Result<Reply, Refusal> {
    let bytes = store.query(chat::PROGRAM, abi::encode(query))?;
    abi::decode(&bytes).map_err(|e| storage(e.sentence))
}

fn unexpected(sentence: &str) -> Refusal {
    Refusal::new(reason::UNEXPECTED_REPLY, sentence)
}
