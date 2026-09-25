//! The boundary to the SDK's rich composer: what a draft is for (a
//! [`Target`]), the key it is kept and focused under, and the chat op a
//! committed draft becomes. Everything else about editing is the SDK's.
use chat::{MsgRow, Op, Principal, parse_message};
pub use ducktape_view_guest::composer::*;
use ducktape_view_guest::host::Error;
use serde::{Deserialize, Serialize};

/// The most a composer sends, well under the program's message cap.
const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Post {
        channel: String,
        thread: Option<u64>,
    },
    Edit {
        channel: String,
        seq: u64,
        base_rev: u32,
    },
}

impl Target {
    pub fn channel(&self) -> &str {
        match self {
            Target::Post { channel, .. } | Target::Edit { channel, .. } => channel,
        }
    }

    /// The key the draft is kept under; its editor is `<key>/editor`.
    pub fn key(&self) -> String {
        match self {
            Target::Post {
                channel,
                thread: None,
            } => format!("draft-{channel}"),
            Target::Post {
                channel,
                thread: Some(root),
            } => format!("draft-{channel}-{root}"),
            Target::Edit { channel, seq, .. } => format!("edit-{channel}-{seq}"),
        }
    }
}

/// The op a committed draft becomes, `id` naming a new message.
pub fn op(id: String, send: &Send, target: &Target) -> Result<Op, Error> {
    let body = &send.body;
    if body.is_empty() || body.len() > MAX_BODY_BYTES {
        return Err(Error::new(
            "invalid_body",
            "Message must contain between 1 byte and 16 KiB",
        ));
    }
    let blocks = parse_message(body);
    Ok(match target.clone() {
        Target::Post { channel, thread } => Op::PostMessage {
            channel_id: channel,
            message_id: id,
            blocks,
            thread,
        },
        Target::Edit {
            channel,
            seq,
            base_rev,
        } => Op::EditMessage {
            channel_id: channel,
            seq,
            blocks,
            base_rev: Some(base_rev),
        },
    })
}

/// The row a just-accepted post by `author` shows as until the program
/// serves it: seq 0. An edit shows nothing early.
pub fn pending_row(op: &Op, author: Principal) -> Option<MsgRow> {
    let Op::PostMessage {
        message_id,
        blocks,
        thread,
        ..
    } = op
    else {
        return None;
    };
    Some(MsgRow {
        message_id: message_id.clone(),
        blocks: blocks.clone(),
        thread: *thread,
        ..MsgRow::by(author)
    })
}
