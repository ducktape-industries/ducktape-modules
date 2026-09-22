//! Chat owns conversations. Forge stores review anchors and queues deterministic system lines.
use crate::Change;
use crate::ops::storage;
use abi::Refusal;
use chat::{Block, ChatMsg, ChatViewQuery, ChatViewReply, PostPolicy};
use store::{Reads, Writes};

pub const CHAT: &str = "chat";

pub fn create<S: Writes>(s: &mut S, repo: &str, change: &Change) {
    emit(
        s,
        ChatMsg::CreateChannel {
            channel_id: change.channel.clone(),
            name: format!("{repo}#{}", change.n),
            post_policy: PostPolicy::Open,
        },
    );
}
fn emit<S: Writes>(s: &mut S, message: ChatMsg) {
    s.emit(CHAT, abi::encode(&message));
}
pub fn message_id<S: Writes>(s: &mut S) -> Result<String, Refusal> {
    let key = b"system-message-seq";
    let n: u64 = s
        .get(key)
        .map(|b| abi::decode(&b))
        .transpose()?
        .unwrap_or(0);
    let n = n
        .checked_add(1)
        .ok_or_else(|| Refusal::new(abi::reason::EXHAUSTED, "system message counter exhausted"))?;
    s.set(key.to_vec(), abi::encode(&n));
    Ok(format!("forge:{n:016x}"))
}
pub fn post<S: Writes>(s: &mut S, change: &Change, message_id: String, text: String) {
    emit(
        s,
        ChatMsg::PostMessage {
            channel_id: change.channel.clone(),
            message_id,
            blocks: vec![Block::paragraph(text)],
            thread: None,
        },
    );
}
pub fn message<S: Reads>(s: &S, id: &str) -> Result<Option<chat::MsgRow>, Refusal> {
    let request = abi::encode(&ChatViewQuery::MessageById {
        message_id: id.into(),
    });
    let bytes = s.query(CHAT, request)?;
    match abi::decode::<ChatViewReply>(&bytes).map_err(|e| storage(e.sentence))? {
        ChatViewReply::Message(row) => Ok(row),
        _ => Err(Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            "chat must answer MessageById with Message",
        )),
    }
}

pub fn attention<S: Reads>(
    s: &S,
    channel: &str,
    key: &[u8],
) -> Result<Option<chat::MsgRow>, Refusal> {
    let request = abi::encode(&ChatViewQuery::ThreadAttention {
        channel_id: channel.into(),
        author: chat::Party::Key(key.to_vec()),
    });
    let bytes = s.query(CHAT, request)?;
    match abi::decode::<ChatViewReply>(&bytes).map_err(|e| storage(e.sentence))? {
        ChatViewReply::Attention(row) => Ok(row),
        _ => Err(Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            "chat must answer ThreadAttention with Attention",
        )),
    }
}
