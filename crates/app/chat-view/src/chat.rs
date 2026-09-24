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

/// [`abi::unhex`], where the empty string is not a key either.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    abi::unhex(text).filter(|bytes| !bytes.is_empty())
}

// ---------- duck links ----------

/// `duck://<chain>/chat/<channel>[/<seq>]`: chat's own tail, the channel
/// written as [`route_segment`] spells it; "" without a chain.
pub fn channel_link(chain: &str, channel: &str, seq: Option<u64>) -> String {
    let channel = route_segment(channel);
    let seq = seq.map(|seq| seq.to_string());
    let mut tail = vec![channel.as_str()];
    tail.extend(seq.as_deref());
    ducklink::mint(chain, ::chat::PROGRAM, &tail).unwrap_or_default()
}

/// A channel id as one route segment. The app hands a view only routes of
/// `[A-Za-z0-9._-]`, and a channel id may hold any byte but `/` (a forge
/// room is `forge:<repo>:<n>`), so every other byte, and the escape `.`
/// itself, is written `.XX` in uppercase hex. An id of `[A-Za-z0-9_-]`
/// reads as itself.
pub fn route_segment(channel: &str) -> String {
    let mut spelled = String::with_capacity(channel.len());
    for byte in channel.bytes() {
        match byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
            true => spelled.push(byte as char),
            false => spelled.push_str(&format!(".{byte:02X}")),
        }
    }
    spelled
}

/// [`route_segment`] read back; a `.` not followed by two hex digits is kept
/// as written.
pub fn channel_of_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut read = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'.')
            .then(|| segment.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                read.push(byte);
                at += 3;
            }
            None => {
                read.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8(read).unwrap_or_else(|_| segment.to_owned())
}

/// The room and message a route handed to this view names:
/// `<channel>[/<seq>]`, the seq 0 when there is none.
pub fn route_target(route: &str) -> Option<(String, u64)> {
    let mut parts = route.splitn(2, '/');
    let channel = channel_of_segment(parts.next()?);
    let seq = parts.next().and_then(|seq| seq.parse().ok()).unwrap_or(0);
    (!channel.is_empty()).then_some((channel, seq))
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

    /// A room whose id the app's route charset refuses (a forge room's `:`)
    /// still lands: the link spells it in `[A-Za-z0-9._-]`, and the view
    /// reads the route the app hands it back to the same id and message.
    #[test]
    fn a_channel_link_round_trips_through_the_apps_route() {
        let route_ok = |route: &str| {
            route.len() <= 256
                && route.split('/').all(|segment| {
                    !segment.is_empty()
                        && segment != "."
                        && segment != ".."
                        && segment.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                        })
                })
        };
        let long = format!("forge:{}:{}", "r".repeat(37), u64::MAX);
        for channel in [
            "general",
            "dm-3-5",
            "forge:big-history:3",
            "forge:my.lib:12",
            "a.2E",
            ".",
            "..",
            "보고서 #1",
            long.as_str(),
        ] {
            let link = channel_link("testnet#0a1b2c3d", channel, Some(42));
            // what the app does with a chain link: the tail, joined
            let route = ducklink::Link::parse(&link).unwrap().tail.join("/");
            assert!(route_ok(&route), "{channel} → {route}");
            assert_eq!(route_target(&route), Some((channel.to_owned(), 42)));
        }
        assert_eq!(route_segment("forge:web:3"), "forge.3Aweb.3A3");
        assert_eq!(route_target("design"), Some(("design".into(), 0)));
        assert_eq!(channel_of_segment("v1.x"), "v1.x", "a bare dot is kept");
    }
}
