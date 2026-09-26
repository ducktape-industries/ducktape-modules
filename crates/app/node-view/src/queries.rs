//! Typed reads of valset.
use ducktape_view_guest::Host;
use ducktape_view_guest::borsh_bytes;
use ducktape_view_guest::host::{Error, pages, wrong_reply};
use ducktape_view_guest::methods::Query;
use serde::{Deserialize, Serialize};
use valset::view::Valset;
use valset::{Membership, PageRequest, Query as Ask, Reply};

/// How many membership pages one read follows.
const MEMBER_PAGES: usize = 64;

/// What the screen shows: the consensus set as valset answers it, and the
/// memberships behind it, as valset's own rows.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Set {
    /// the validator keys, in the order the program answers them
    pub validators: Vec<Vec<u8>>,
    #[serde(with = "borsh_bytes")]
    pub members: Vec<Membership>,
    /// the read stopped at [`MEMBER_PAGES`]: more memberships exist
    pub more: bool,
}

/// The set, read twice: the consensus keys the program answers, then every
/// membership behind them.
pub(crate) async fn set(host: Host) -> Result<Set, Error> {
    let validators = match host.ask::<Query<Valset>>(Ask::Validators).await? {
        Reply::Validators(keys) => keys,
        _ => return Err(wrong_reply()),
    };
    let (members, next) = pages(None, MEMBER_PAGES, |after| {
        let ask = host.ask::<Query<Valset>>(Ask::Memberships {
            page: PageRequest { after, limit: None },
        });
        async move {
            match ask.await? {
                Reply::Memberships(reply) => Ok((reply.items, reply.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    Ok(Set {
        validators,
        members,
        more: next.is_some(),
    })
}
