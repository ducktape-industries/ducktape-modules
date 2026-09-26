//! Typed reads of the chat module. Every list takes a `PageRequest` and answers a
//! `PageResponse`; `next` is the cursor of the page after it.
use chat::{
    ChannelInfo, MemberRow, MessageHits, MsgRow, PageRequest, PageResponse, Principal, Query, Reply,
};
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Error, pages, wrong_reply};

use crate::api::{Ask, ChatApi};
use crate::{PAGE, WINDOW};

/// How many channel pages one read follows: 64 pages of 64 channels.
const CHANNEL_PAGES: usize = 64;

fn page(after: Option<Vec<u8>>, limit: usize) -> PageRequest {
    PageRequest {
        after,
        limit: Some(limit as u64),
    }
}

/// Every room, up to [`CHANNEL_PAGES`] pages, and whether more follow.
pub(crate) async fn channels(host: Host) -> Result<(Vec<ChannelInfo>, bool), Error> {
    let (all, next) = pages(None, CHANNEL_PAGES, |after| {
        let ask = host.ask::<Ask<ChatApi>>(Query::Channels {
            page: page(after, PAGE),
        });
        async move {
            match ask.await? {
                Reply::Channels(reply) => Ok((reply.items, reply.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    Ok((all, next.is_some()))
}

/// The `limit` roots below `below` (or the newest), oldest first, with
/// whether older ones remain.
pub(crate) async fn roots(
    host: Host,
    channel_id: String,
    viewer: Vec<Principal>,
    below: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Error> {
    ::chat::view::roots(
        host,
        channel_id,
        viewer,
        below,
        limit.div_ceil(PAGE),
        PAGE as u64,
    )
    .await
}

/// The rows around a landing seq, oldest first.
pub(crate) async fn around(
    host: Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<Principal>,
) -> Result<Vec<MsgRow>, Error> {
    match host
        .ask::<Ask<ChatApi>>(Query::MessagesAround {
            channel_id,
            seq,
            viewer,
            page: page(None, WINDOW / 2),
        })
        .await?
    {
        Reply::Messages(rows) => Ok(sorted(rows)),
        _ => Err(wrong_reply()),
    }
}

pub(crate) fn sorted(mut rows: Vec<MsgRow>) -> Vec<MsgRow> {
    rows.sort_by_key(|row| row.seq);
    rows
}

pub(crate) async fn members(host: Host, channel_id: String) -> Result<Vec<MemberRow>, Error> {
    match host
        .ask::<Ask<ChatApi>>(Query::Members {
            channel_id,
            page: page(None, WINDOW),
        })
        .await?
    {
        Reply::Members(reply) => Ok(reply.items),
        _ => Err(wrong_reply()),
    }
}

/// One page of a thread's replies after `after`, and the cursor to page on.
pub(crate) async fn thread(
    host: Host,
    channel_id: String,
    root_seq: u64,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Error> {
    match host
        .ask::<Ask<ChatApi>>(Query::Thread {
            channel_id,
            root_seq,
            viewer,
            page: page(after, WINDOW),
        })
        .await?
    {
        Reply::Thread {
            replies: PageResponse { items, next, .. },
            ..
        } => Ok((sorted(items), next)),
        _ => Err(wrong_reply()),
    }
}

/// A search: `#tag` pages through the tag index, anything else is a
/// full-text search capped by the module. Returns the hits, whether the
/// search was capped, and the cursor of the next tag page.
pub(crate) async fn search_hits(
    host: Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<Principal>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, bool, Option<Vec<u8>>), Error> {
    let query = match text.strip_prefix('#') {
        Some(tag) if !tag.is_empty() => Query::TagSearch {
            tag: tag.to_owned(),
            viewer,
            channel_id,
            page: page(after, PAGE),
        },
        _ => Query::Search {
            text,
            viewer,
            channel_id,
            page: page(None, PAGE),
        },
    };
    match host.ask::<Ask<ChatApi>>(query).await? {
        Reply::Hits(MessageHits { hits, capped }) => Ok((hits, capped, None)),
        Reply::TagHits(PageResponse { items, next, .. }) => Ok((items, false, next)),
        _ => Err(wrong_reply()),
    }
}
