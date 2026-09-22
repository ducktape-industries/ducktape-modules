//! Typed reads of the chat module.
use super::{
    ChannelInfo, ChatApi, ChatViewQuery, ChatViewReply, MemberRow, MessageHits, MsgRow,
    NameDirectory, PAGE, TagPage, WINDOW,
};
use ducktape_view_guest::doors::Query as ViewOf;
use ducktape_view_guest::host::{Refusal, malformed};

fn wrong_reply() -> Refusal {
    malformed("the chat module answered another question".into())
}

pub(crate) async fn channels(host: ducktape_view_guest::Host) -> Result<Vec<ChannelInfo>, Refusal> {
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let ChatViewReply::Channels {
            channels,
            has_more,
            next_after,
        } = host
            .ask::<ViewOf<ChatApi>>(ChatViewQuery::Channels {
                after,
                limit: Some(PAGE),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(channels);
        if !has_more || next_after.is_none() {
            return Ok(all);
        }
        after = next_after;
    }
}

/// One page of roots older than `before` (or the newest), oldest first,
/// with whether older ones remain.
pub(crate) async fn roots(
    host: ducktape_view_guest::Host,
    channel_id: String,
    viewer: Vec<String>,
    before: Option<u64>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Refusal> {
    let mut all = Vec::new();
    let mut before_seq = before;
    loop {
        let ChatViewReply::Roots {
            roots,
            has_more,
            next_before_seq,
        } = host
            .ask::<ViewOf<ChatApi>>(ChatViewQuery::Roots {
                channel_id: channel_id.clone(),
                viewer_handles: viewer.clone(),
                before_seq,
                limit: Some(PAGE),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(roots);
        if !has_more || next_before_seq.is_none() || all.len() >= limit {
            return Ok((sorted(all), has_more && next_before_seq.is_some()));
        }
        before_seq = next_before_seq;
    }
}

/// The rows around a landing seq, oldest first.
pub(crate) async fn around(
    host: ducktape_view_guest::Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<String>,
) -> Result<Vec<MsgRow>, Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::MessagesAround {
            channel_id,
            seq,
            viewer_handles: viewer,
            limit: Some(WINDOW / 2),
        })
        .await?
    {
        ChatViewReply::Messages(rows) => Ok(sorted(rows)),
        _ => Err(wrong_reply()),
    }
}

pub(crate) fn sorted(mut rows: Vec<MsgRow>) -> Vec<MsgRow> {
    rows.sort_by_key(|row| row.seq);
    rows
}

pub(crate) async fn members(
    host: ducktape_view_guest::Host,
    channel_id: String,
) -> Result<Vec<MemberRow>, Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::Members {
            channel_id,
            after: None,
            limit: Some(WINDOW),
        })
        .await?
    {
        ChatViewReply::Members { members, .. } => Ok(members),
        _ => Err(wrong_reply()),
    }
}

/// One page of a thread's replies after `after`, and how to page on.
pub(crate) async fn thread(
    host: ducktape_view_guest::Host,
    channel_id: String,
    root_seq: u64,
    viewer: Vec<String>,
    after: Option<u64>,
) -> Result<(Vec<MsgRow>, bool, Option<u64>), Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::Thread {
            channel_id,
            root_seq,
            viewer_handles: viewer,
            after_reply_seq: after,
            limit: Some(WINDOW),
        })
        .await?
    {
        ChatViewReply::Thread {
            replies,
            has_more,
            next_reply_seq,
            ..
        } => Ok((sorted(replies), has_more, next_reply_seq)),
        _ => Err(wrong_reply()),
    }
}

/// A search: `#tag` pages through the tag index, anything else is a
/// full-text search capped by the module.
pub(crate) async fn search_hits(
    host: ducktape_view_guest::Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<String>,
    after: Option<String>,
) -> Result<(Vec<MsgRow>, bool, bool, Option<String>), Refusal> {
    let query = match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => ChatViewQuery::TagSearch {
            tag: tag.to_owned(),
            viewer_handles: viewer,
            channel_id,
            after,
            limit: Some(PAGE),
        },
        _ => ChatViewQuery::Search {
            text,
            viewer_handles: viewer,
            channel_id,
            limit: Some(PAGE),
        },
    };
    match host.ask::<ViewOf<ChatApi>>(query).await? {
        ChatViewReply::Hits(MessageHits { hits, capped }) => Ok((hits, capped, false, None)),
        ChatViewReply::TagHits(TagPage {
            hits,
            has_more,
            next_after,
        }) => Ok((hits, false, has_more, next_after)),
        _ => Err(wrong_reply()),
    }
}

/// The identity roster, paged through chat, folded into the name directory.
pub(crate) async fn roster(host: ducktape_view_guest::Host) -> Result<NameDirectory, Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::Accounts { limit: Some(256) })
        .await?
    {
        ChatViewReply::Accounts(accounts) => Ok(NameDirectory::from_roster(accounts)),
        _ => Err(wrong_reply()),
    }
}

/// The account the seated key holds, straight from identity — chat-view and
/// forge-view share this resolution (`identity::view::account_of_key`).
pub(crate) async fn resolve_me(
    host: ducktape_view_guest::Host,
    key: String,
) -> Result<Option<u64>, Refusal> {
    identity::view::account_of_key(&host, &key).await
}
