//! Typed reads. Every forge list is cursored, so one read follows `next`
//! until the program stops offering one (or the page budget runs out) and
//! hands the screen a single reply. A typed refusal becomes a `Refusal`, so
//! the four states of a `Loaded` slot stay honest.
use ducktape_view_guest::Host;
use ducktape_view_guest::doors::Query as Ask;
use ducktape_view_guest::host::{Refusal, malformed};

use crate::api::{Ask as Forge, ChatApi};
use crate::state::Names;
use forge::{Page, PageReply, Query, Reply};

/// What one page asks for: 64 rows, from the start. A limit above the
/// program's `Bounds.page_size` is clamped to it.
pub(crate) const PAGE: Page = Page::first(64);
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

/// One read of forge, `next` followed.
pub(crate) async fn fetch(host: Host, query: Query) -> Result<Reply, Refusal> {
    let mut reply = host.ask::<Forge>(query.clone()).await?;
    for _ in 1..MAX_PAGES {
        let Some(after) = next_cursor(&reply).cloned() else {
            break;
        };
        let Some(query) = with_cursor(&query, after) else {
            break;
        };
        let more = host.ask::<Forge>(query).await?;
        extend(&mut reply, more);
    }
    Ok(reply)
}

fn next_cursor(reply: &Reply) -> Option<&Vec<u8>> {
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
        Reply::Compare { .. } | Reply::Blob { .. } | Reply::Activity { .. } => None,
    }
}

/// The same question, continued. An unpaged query has no continuation.
fn with_cursor(query: &Query, after: Vec<u8>) -> Option<Query> {
    let mut query = query.clone();
    let slot = match &mut query {
        Query::Repos { page, .. }
        | Query::Repo { page, .. }
        | Query::Refs { page, .. }
        | Query::Log { page, .. }
        | Query::Tree { page, .. }
        | Query::Diff { page, .. }
        | Query::Changes { page, .. }
        | Query::Change { page, .. }
        | Query::Judgment { page, .. } => page,
        Query::Compare { .. }
        | Query::Blob { .. }
        | Query::Activity { .. }
        | Query::Advertise { .. }
        | Query::Upload { .. } => return None,
    };
    slot.after = Some(after);
    Some(query)
}

fn absorb<T>(page: &mut PageReply<T>, more: PageReply<T>) {
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
        .ask::<Ask<ChatApi>>(chat::ChatViewQuery::Accounts {
            page: Page::first(256),
        })
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
    let mut page = PAGE;
    for _ in 0..MAX_PAGES {
        let chat::ChatViewReply::Roots(roots) = host
            .ask::<Ask<ChatApi>>(chat::ChatViewQuery::Roots {
                channel_id: channel_id.clone(),
                viewer_handles: viewer.clone(),
                page: page.clone(),
            })
            .await?
        else {
            return Err(wrong_reply());
        };
        all.extend(roots.items);
        match roots.next {
            Some(next) => page.after = Some(next),
            None => break,
        }
    }
    all.sort_by_key(|row| row.seq);
    Ok(all)
}
