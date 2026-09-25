//! Typed reads of the chat module. Every list takes a `Page` and answers a
//! `PageReply`; `next` is the cursor of the page after it.
use chat::{ChannelInfo, MemberRow, MessageHits, MsgRow, Page, PageReply, Party, Query, Reply};
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Refusal, malformed};

use crate::api::{Ask, ChatApi};
use crate::names::NameDirectory;
use crate::{PAGE, WINDOW};

pub(crate) fn wrong_reply() -> Refusal {
    malformed("the chat module answered another question".into())
}

fn page(after: Option<Vec<u8>>, limit: usize) -> Page {
    Page {
        after,
        limit: Some(limit as u64),
    }
}

pub(crate) async fn channels(host: Host) -> Result<Vec<ChannelInfo>, Refusal> {
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let Reply::Channels(reply) = host
            .ask::<Ask<ChatApi>>(Query::Channels {
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
    host: Host,
    channel_id: String,
    viewer: Vec<Party>,
    below: Option<Vec<u8>>,
    limit: usize,
) -> Result<(Vec<MsgRow>, bool), Refusal> {
    let mut all = Vec::new();
    let mut after = below;
    loop {
        let Reply::Roots(reply) = host
            .ask::<Ask<ChatApi>>(Query::Roots {
                channel_id: channel_id.clone(),
                viewer: viewer.clone(),
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
    host: Host,
    channel_id: String,
    seq: u64,
    viewer: Vec<Party>,
) -> Result<Vec<MsgRow>, Refusal> {
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

pub(crate) async fn members(host: Host, channel_id: String) -> Result<Vec<MemberRow>, Refusal> {
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
    viewer: Vec<Party>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, Option<Vec<u8>>), Refusal> {
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
    host: Host,
    text: String,
    channel_id: Option<String>,
    viewer: Vec<Party>,
    after: Option<Vec<u8>>,
) -> Result<(Vec<MsgRow>, bool, Option<Vec<u8>>), Refusal> {
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
        Reply::TagHits(PageReply { items, next, .. }) => Ok((items, false, next)),
        _ => Err(wrong_reply()),
    }
}

/// The identity roster, paged through chat, folded into the name directory.
pub(crate) async fn roster(host: Host) -> Result<NameDirectory, Refusal> {
    match host
        .ask::<Ask<ChatApi>>(Query::Accounts {
            page: page(None, 256),
        })
        .await?
    {
        Reply::Accounts(accounts) => Ok(NameDirectory::from_roster(accounts)),
        _ => Err(wrong_reply()),
    }
}

/// The account the seated key holds, straight from identity — chat-view and
/// forge-view share this resolution (`identity::view::account_of_key`).
pub(crate) async fn resolve_me(host: Host, key: String) -> Result<Option<u64>, Refusal> {
    identity::view::account_of_key(&host, &key).await
}
