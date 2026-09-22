//! Chat owns conversations. Forge stores review anchors and queues deterministic system lines.
use crate::refuse::storage;
use crate::{Change, Sandbox};
use abi::Refusal;
use chat::{Block, ChatMsg, ChatViewQuery, ChatViewReply, PostPolicy};

pub const CHAT: &str = "chat";

pub fn create<S: Sandbox>(s: &S, repo: &str, change: &Change) {
    emit(
        s,
        ChatMsg::CreateChannel {
            channel_id: change.channel.clone(),
            name: format!("{repo}#{}", change.n),
            post_policy: PostPolicy::Open,
        },
    );
}
fn emit<S: Sandbox>(s: &S, message: ChatMsg) {
    // Chat's existing wire is JSON. Forge's contract and all forge records remain Borsh.
    s.emit(
        CHAT,
        borsh::to_vec(&message).expect("a chat message serializes"),
    );
}
pub fn message_id<S: Sandbox>(s: &S) -> Result<String, Refusal> {
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
pub fn post<S: Sandbox>(s: &S, change: &Change, message_id: String, text: String) {
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
pub fn message<S: Sandbox>(s: &S, id: &str) -> Result<Option<chat::MsgRow>, Refusal> {
    let request = borsh::to_vec(&ChatViewQuery::MessageById {
        message_id: id.into(),
    })
    .expect("a chat query serializes");
    let bytes = s.query(CHAT, request)?;
    match borsh::from_slice::<ChatViewReply>(&bytes).map_err(|e| storage(e.to_string()))? {
        ChatViewReply::Message(row) => Ok(row),
        _ => Err(Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            "chat must answer MessageById with Message",
        )),
    }
}

pub fn attention<S: Sandbox>(
    s: &S,
    channel: &str,
    key: &[u8],
) -> Result<Option<chat::MsgRow>, Refusal> {
    let request = borsh::to_vec(&ChatViewQuery::ThreadAttention {
        channel_id: channel.into(),
        author: chat::Party::Key(key.to_vec()),
    })
    .expect("chat query");
    let bytes = s.query(CHAT, request)?;
    match borsh::from_slice::<ChatViewReply>(&bytes).map_err(|e| storage(e.to_string()))? {
        ChatViewReply::Attention(row) => Ok(row),
        _ => Err(Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            "chat must answer ThreadAttention with Attention",
        )),
    }
}
