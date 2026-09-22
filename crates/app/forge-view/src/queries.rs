//! Typed reads. Every forge list is cursored, so one read follows `next`
//! until the program stops offering one (or the page budget runs out) and
//! hands the screen a single reply. A typed refusal becomes a `Refusal`, so
//! the four states of a `Loaded` slot stay honest.
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::ViewOf;

use crate::api::{Ask, ChatApi};
use crate::state::Names;
use forge::{Cursor, Page, Query, Reply};

/// What one page asks for. Every founded `Bounds.page_size` seen so far is
/// at least this, and a limit above it is refused rather than clamped.
pub(crate) const PAGE: u32 = 64;
/// How many pages one read follows. A history longer than this shows what
/// it read and says more follows, rather than walking a repository forever.
const MAX_PAGES: usize = 16;

/// The cache key of a read: the query is its own identity.
pub(crate) fn key(query: &Query) -> String {
    format!("{query:?}")
}

fn wrong_reply() -> Refusal {
    malformed("the module answered another question".into())
}

/// One read of forge, cursors followed.
pub(crate) async fn fetch(host: Host, query: Query) -> Result<Reply, Refusal> {
    let mut reply = ask(&host, query.clone()).await?;
    for _ in 1..MAX_PAGES {
        let Some(cursor) = next_cursor(&reply).cloned() else {
            break;
        };
        let Some(query) = with_cursor(&query, cursor) else {
            break;
        };
        let more = ask(&host, query).await?;
        extend(&mut reply, more);
    }
    Ok(reply)
}

async fn ask(host: &Host, query: Query) -> Result<Reply, Refusal> {
    match host.ask::<Ask>(query).await? {
        Reply::Refused {
            reason, sentence, ..
        } => Err(Refusal::new(&reason, &sentence)),
        reply => Ok(reply),
    }
}

fn next_cursor(reply: &Reply) -> Option<&Cursor> {
    match reply {
        Reply::Repos { page, .. } => page.next.as_ref(),
        Reply::Repo { writers, .. } => writers.next.as_ref(),
        Reply::Refs { page, .. } => page.next.as_ref(),
        Reply::Log { page, .. } => page.next.as_ref(),
        Reply::Tree { page, .. } => page.next.as_ref(),
        Reply::Diff { page, .. } => page.next.as_ref(),
        Reply::Changes { page, .. } => page.next.as_ref(),
        Reply::Change { reviews, .. } => reviews.next.as_ref(),
        Reply::Judgment { page, .. } => page.next.as_ref(),
        Reply::Compare { .. }
        | Reply::Blob { .. }
        | Reply::Activity { .. }
        | Reply::Refused { .. } => None,
    }
}

/// The same question, continued. An unpaged query has no continuation.
fn with_cursor(query: &Query, next: Cursor) -> Option<Query> {
    let mut query = query.clone();
    let slot = match &mut query {
        Query::Repos { cursor, .. }
        | Query::Repo { cursor, .. }
        | Query::Refs { cursor, .. }
        | Query::Log { cursor, .. }
        | Query::Tree { cursor, .. }
        | Query::Diff { cursor, .. }
        | Query::Changes { cursor, .. }
        | Query::Change { cursor, .. }
        | Query::Judgment { cursor, .. } => cursor,
        Query::Compare { .. }
        | Query::Blob { .. }
        | Query::Activity { .. }
        | Query::Advertise { .. }
        | Query::Upload { .. } => return None,
    };
    *slot = Some(next);
    Some(query)
}

fn absorb<T>(page: &mut Page<T>, more: Page<T>) {
    page.items.extend(more.items);
    page.next = more.next;
}

fn extend(into: &mut Reply, more: Reply) {
    match (into, more) {
        (Reply::Repos { page, .. }, Reply::Repos { page: more, .. }) => absorb(page, more),
        (Reply::Refs { page, .. }, Reply::Refs { page: more, .. }) => absorb(page, more),
        (Reply::Repo { writers, .. }, Reply::Repo { writers: more, .. }) => absorb(writers, more),
        (Reply::Log { page, .. }, Reply::Log { page: more, .. }) => absorb(page, more),
        (Reply::Tree { page, .. }, Reply::Tree { page: more, .. }) => absorb(page, more),
        (Reply::Diff { page, .. }, Reply::Diff { page: more, .. }) => absorb(page, more),
        (Reply::Changes { page, .. }, Reply::Changes { page: more, .. }) => absorb(page, more),
        (Reply::Change { reviews, .. }, Reply::Change { reviews: more, .. }) => {
            absorb(reviews, more)
        }
        (Reply::Judgment { page, .. }, Reply::Judgment { page: more, .. }) => absorb(page, more),
        _ => {}
    }
}

/// The identity roster, through chat — the one door that already joins
/// accounts, their names and the keys they hold.
pub(crate) async fn roster(host: Host) -> Result<Names, Refusal> {
    match host
        .ask::<ViewOf<ChatApi>>(chat::ChatViewQuery::Accounts { limit: Some(256) })
        .await?
    {
        chat::ChatViewReply::Accounts(rows) => Ok(Names::new(rows)),
        _ => Err(wrong_reply()),
    }
}

/// A change's hidden channel, oldest first.
pub(crate) async fn conversation(
    host: Host,
    channel_id: String,
    viewer: Vec<String>,
) -> Result<Vec<chat::MsgRow>, Refusal> {
    let mut all: Vec<chat::MsgRow> = Vec::new();
    let mut before_seq = None;
    for _ in 0..MAX_PAGES {
        let chat::ChatViewReply::Roots {
            roots,
            has_more,
            next_before_seq,
        } = host
            .ask::<ViewOf<ChatApi>>(chat::ChatViewQuery::Roots {
                channel_id: channel_id.clone(),
                viewer_handles: viewer.clone(),
                before_seq,
                limit: Some(PAGE as usize),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(roots);
        match next_before_seq {
            Some(next) if has_more => before_seq = Some(next),
            _ => break,
        }
    }
    all.sort_by_key(|row| row.seq);
    Ok(all)
}
