//! Typed reads of the system programs: accounts (identity), validators
//! (valset) and programs (module-registry). The window's blocks are the
//! host's (`load.rs`).
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Error, pages, wrong_reply};
use ducktape_view_guest::methods::Query;
use identity::view::Identity;
use module_registry as registry;
use module_registry::view::Registry;
use valset::view::Valset;

use crate::state::{Accounts, Network};

/// How many pages of accounts, or of scheduled changes, one read follows.
const LIST_PAGES: usize = 64;

/// Every account, up to [`LIST_PAGES`] pages of them.
pub(crate) async fn accounts(host: Host) -> Result<Accounts, Error> {
    let (list, next) = pages(None, LIST_PAGES, |after| {
        let ask = host.ask::<Query<Identity>>(identity::Query::List {
            page: identity::PageRequest { after, limit: None },
        });
        async move {
            match ask.await? {
                identity::Reply::Accounts(reply) => Ok((reply.items, reply.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    Ok(Accounts {
        list,
        more: next.is_some(),
    })
}

pub(crate) async fn validators(host: Host) -> Result<Vec<Vec<u8>>, Error> {
    match host.ask::<Query<Valset>>(valset::Query::Validators).await? {
        valset::Reply::Validators(keys) => Ok(keys),
        _ => Err(wrong_reply()),
    }
}

/// What the registry runs and lists, and what it will.
///
/// `At(0)` is the folded set: the registry applies the changes due at or
/// before the height asked, and it answers no height of its own, so there is
/// no "as of now" to ask for — the scheduled list is what is still to come.
pub(crate) async fn network(host: Host) -> Result<Network, Error> {
    let programs = match host.ask::<Query<Registry>>(registry::Query::At(0)).await? {
        registry::Reply::Programs(programs) => programs,
        _ => return Err(wrong_reply()),
    };
    let views = match host
        .ask::<Query<Registry>>(registry::Query::Views(0))
        .await?
    {
        registry::Reply::Views(views) => views,
        _ => return Err(wrong_reply()),
    };
    let (changes, next) = pages(None, LIST_PAGES, |after| {
        let ask = host.ask::<Query<Registry>>(registry::Query::Scheduled {
            page: registry::PageRequest { after, limit: None },
        });
        async move {
            match ask.await? {
                registry::Reply::Scheduled(reply) => Ok((reply.items, reply.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    Ok(Network {
        programs,
        views,
        changes,
        more: next.is_some(),
    })
}
