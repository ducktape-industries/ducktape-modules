//! The module's own types, linked: what the view sends (`ChatMsg`), what
//! it asks (`ChatViewQuery`) and the rows it draws. One definition site,
//! in `crates/app/chat`.
pub use chat::{
    AccountRow, Block, ChannelInfo, ChatMsg, ChatViewQuery, ChatViewReply, Mark, MemberRow,
    MessageHits, MsgRow, Party, PostPolicy, ReactionSummary, Span, dm_peers, hex, parse_message,
};

pub fn members_only(info: &ChannelInfo) -> bool {
    info.channel.post_policy == PostPolicy::MembersOnly
}

/// A handle or a bare key hex back to the party a membership write names.
pub fn party_of(text: &str) -> Option<Party> {
    let text = text.trim();
    if let Some(number) = text.strip_prefix("acct:") {
        return number.parse().ok().map(Party::Account);
    }
    if let Ok(number) = text.parse::<u64>() {
        return Some(Party::Account(number));
    }
    let key = text.strip_prefix("user:").unwrap_or(text);
    unhex(key).map(Party::Key)
}

/// An even-length all-hex string back to its bytes; anything else is not hex.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}
