//! Typed reads. Every forge list is cursored, so one read follows `next`
//! until the program stops offering one (or the page budget runs out) and
//! hands the screen a single reply. A typed refusal becomes a `Refusal`, so
//! the four states of a `Loaded` slot stay honest.
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Refusal, pages, wrong_reply};
use ducktape_view_guest::methods::Query as Ask;

use crate::api::{Ask as Forge, ChatApi};
use forge::{Page, PageReply, Query, Reply};

/// What one page asks for: 64 rows, from the start. A limit above the
/// program's `Bounds.page_size` is clamped to it.
pub(crate) const PAGE: Page = Page::first(64);
/// How many pages one read follows. A history longer than this shows what
/// it read and says more follows, rather than walking a repository forever.
const MAX_PAGES: usize = 16;

/// One read of forge, `next` followed: the pages after the first fold into
/// it.
pub(crate) async fn fetch(host: Host, query: Query) -> Result<Reply, Refusal> {
    let mut reply = host.ask::<Forge>(query.clone()).await?;
    let (more, _) = pages(next_cursor(&reply).cloned(), MAX_PAGES - 1, |after| {
        let ask = after
            .and_then(|after| with_cursor(&query, after))
            .map(|query| host.ask::<Forge>(query));
        async move {
            let Some(ask) = ask else {
                return Ok((Vec::new(), None));
            };
            let page = ask.await?;
            let next = next_cursor(&page).cloned();
            Ok((vec![page], next))
        }
    })
    .await?;
    for page in more {
        extend(&mut reply, page);
    }
    Ok(reply)
}

/// Whether a folded read stopped at the page budget with more to read:
/// [`fetch`] follows every cursor it can, so one left over means exactly
/// that.
pub(crate) fn cut_short(reply: &Reply) -> bool {
    next_cursor(reply).is_some()
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
    query.page_mut()?.after = Some(after);
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

/// A change's hidden channel, oldest first, and whether more follow past
/// the page budget.
pub(crate) async fn conversation(
    host: Host,
    channel_id: String,
    viewer: Vec<identity::Party>,
) -> Result<(Vec<chat::MsgRow>, bool), Refusal> {
    let (mut all, next) = pages(None, MAX_PAGES, |after| {
        let ask = host.ask::<Ask<ChatApi>>(chat::Query::Roots {
            channel_id: channel_id.clone(),
            viewer: viewer.clone(),
            page: Page { after, ..PAGE },
        });
        async move {
            match ask.await? {
                chat::Reply::Roots(roots) => Ok((roots.items, roots.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    all.sort_by_key(|row: &chat::MsgRow| row.seq);
    Ok((all, next.is_some()))
}
