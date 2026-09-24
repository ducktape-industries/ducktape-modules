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

// ---------- duck links ----------

/// `duck://<chain>/chat/<channel>[/<seq>]`: chat's own tail, as the module
/// reads it; "" without a chain.
pub fn channel_link(chain: &str, channel: &str, seq: Option<u64>) -> String {
    let seq = seq.map(|seq| seq.to_string());
    let mut tail = vec![channel];
    tail.extend(seq.as_deref());
    ducklink::mint(chain, ::chat::PROGRAM, &tail).unwrap_or_default()
}

/// A pressed mention (an account number) becomes `duck://<chain>/identity/<n>`,
/// the link the app opens; any other link is already one and passes through.
pub fn pressed_link(link: String, chain: &str) -> String {
    match link.parse::<u64>() {
        Ok(account) => {
            ducklink::mint(chain, identity::PROGRAM, &[&account.to_string()]).unwrap_or_default()
        }
        Err(_) => link,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_keep_their_shapes() {
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "general", Some(42)),
            "duck://testnet-0a1b2c3d/chat/general/42"
        );
        assert_eq!(channel_link("", "general", None), "");
        assert_eq!(
            pressed_link("7".into(), "testnet#0a1b2c3d"),
            "duck://testnet-0a1b2c3d/identity/7"
        );
    }
}
