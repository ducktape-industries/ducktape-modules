//! Typed reads of the chat module. Every list takes a `Page` and answers a
//! `PageReply`; `next` is the cursor of the page after it.
use super::{
    ChannelInfo, ChatApi, ChatViewQuery, ChatViewReply, MemberRow, MessageHits, MsgRow,
    NameDirectory, PAGE, WINDOW,
};
use chat::{Page, PageReply};
use ducktape_view_guest::doors::Query as ViewOf;
use ducktape_view_guest::host::{Refusal, malformed};

fn wrong_reply() -> Refusal {
    malformed("the chat module answered another question".into())
}

fn page(after: Option<Vec<u8>>, limit: usize) -> Page {
    Page {
        after,
        limit: Some(limit as u64),
    }
}

pub(crate) async fn channels(host: ducktape_view_guest::Host) -> Result<Vec<ChannelInfo>, Refusal> {
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let ChatViewReply::Channels(reply) = host
            .ask::<ViewOf<ChatApi>>(ChatViewQuery::Channels {
                page: page(after, PAGE),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(reply.items);
        match reply.next {
            Some(next) => after = Some(next),
            None => return Ok(all),
        }
    }
}

/// One page of roots below `below` (or the newest), oldest first, with
/// whether older ones remain.
pub(crate) async fn roots(
    host: ducktape_view_guest::Host,
    channel_id: String,
    viewer: Vec<String>,
    below: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Refusal> {
    let mut all = Vec::new();
    let mut after = below;
    loop {
        let ChatViewReply::Roots(reply) = host
            .ask::<ViewOf<ChatApi>>(ChatViewQuery::Roots {
                channel_id: channel_id.clone(),
                viewer_handles: viewer.clone(),
                page: page(after, PAGE),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(reply.items);
        match reply.next {
            Some(next) if all.len() < limit => after = Some(next),
            next => return Ok((sorted(all), next.is_some())),
        }
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
            page: page(None, WINDOW / 2),
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
            page: page(None, WINDOW),
        })
        .await?
    {
        ChatViewReply::Members(reply) => Ok(reply.items),
        _ => Err(wrong_reply()),
    }
}

/// One page of a thread's replies after `after`, and the cursor to page on.
pub(crate) async fn thread(
    host: ducktape_view_guest::Host,
    channel_id: String,
    root_seq: u64,
    viewer: Vec<String>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::Thread {
            channel_id,
            root_seq,
            viewer_handles: viewer,
            page: page(after, WINDOW),
        })
        .await?
    {
        ChatViewReply::Thread {
            replies: PageReply { items, next, .. },
            ..
        } => Ok((sorted(items), next)),
        _ => Err(wrong_reply()),
    }
}

/// A search: `#tag` pages through the tag index, anything else is a
/// full-text search capped by the module. Returns the hits, whether the
/// search was capped, and the cursor of the next tag page.
pub(crate) async fn search_hits(
    host: ducktape_view_guest::Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<String>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, bool, Option<Vec<u8>>), Refusal> {
    let query = match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => ChatViewQuery::TagSearch {
            tag: tag.to_owned(),
            viewer_handles: viewer,
            channel_id,
            page: page(after, PAGE),
        },
        _ => ChatViewQuery::Search {
            text,
            viewer_handles: viewer,
            channel_id,
            page: page(None, PAGE),
        },
    };
    match host.ask::<ViewOf<ChatApi>>(query).await? {
        ChatViewReply::Hits(MessageHits { hits, capped }) => Ok((hits, capped, None)),
        ChatViewReply::TagHits(PageReply { items, next, .. }) => Ok((items, false, next)),
        _ => Err(wrong_reply()),
    }
}

/// The identity roster, paged through chat, folded into the name directory.
pub(crate) async fn roster(host: ducktape_view_guest::Host) -> Result<NameDirectory, Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(ChatViewQuery::Accounts {
            page: page(None, 256),
        })
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
