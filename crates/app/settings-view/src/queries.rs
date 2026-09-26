//! Typed reads: who the seated key is (identity), and whether each of its
//! keys validates (valset). Rows keep what the programs said; they are
//! worded only when drawn.
use ducktape_view_guest::Host;
use ducktape_view_guest::host::{Error, malformed, pages, wrong_reply};
use ducktape_view_guest::methods::Query;
use identity::{Control, Kind, PageRequest, Standing};
use serde::{Deserialize, Serialize};

use crate::api::{Identity, Valset};

/// How many pages of a person's agents one read follows.
const AGENT_PAGES: usize = 64;

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    /// `None`: the seated key holds no account yet
    pub number: Option<u64>,
    /// the account's name; empty while it has no account
    pub name: String,
    pub keys: Vec<Key>,
    /// a person's account manages agents; an agent's or a module's none
    pub manages: bool,
    pub agents: Vec<Agent>,
    /// the agent read stopped at [`AGENT_PAGES`]: more agents exist
    pub more_agents: bool,
}

/// An agent the account manages.
#[derive(Clone, Serialize, Deserialize)]
pub struct Agent {
    pub number: u64,
    pub name: String,
    pub keys: usize,
    /// what identity says it is: always `Kind::Managed` (checked on read)
    #[serde(with = "ducktape_view_guest::borsh_bytes")]
    pub kind: Kind,
}

impl Agent {
    pub fn standing(&self) -> Standing {
        match self.kind {
            Kind::Managed { standing, .. } => standing,
            Kind::Person | Kind::Module(_) => Standing::Active,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Key {
    pub label: Option<String>,
    pub key: Vec<u8>,
    pub validator: bool,
}

/// Who the seated `signer` (hex) is: its account as the host resolved it
/// (`number`), with every key on it, or the key alone while it holds none.
pub(crate) async fn account(
    host: Host,
    signer: String,
    number: Option<u64>,
) -> Result<Option<Account>, Error> {
    if signer.is_empty() {
        return Ok(None);
    }
    let key = abi::unhex(&signer)
        .ok_or_else(|| malformed("the session's signer is not hexadecimal".into()))?;
    let Some(number) = number else {
        return Ok(Some(Account {
            number: None,
            name: String::new(),
            keys: vec![read_key(&host, key, None).await?],
            manages: false,
            agents: Vec::new(),
            more_agents: false,
        }));
    };
    let account = match host
        .ask::<Query<Identity>>(identity::Query::Get { number })
        .await?
    {
        identity::Reply::Account(account) => account,
        _ => return Err(wrong_reply()),
    };
    let Some(account) = account else {
        return Ok(None);
    };
    let mut keys = Vec::new();
    for key in account.keys() {
        keys.push(read_key(&host, key.key.clone(), key.label.clone()).await?);
    }
    let manages = matches!(account.control, Control::Person { .. });
    let (agents, more_agents) = if manages {
        agents(&host, number).await?
    } else {
        (Vec::new(), false)
    };
    Ok(Some(Account {
        number: Some(number),
        name: account.card.name,
        keys,
        manages,
        agents,
        more_agents,
    }))
}

/// The agents `manager` manages, up to [`AGENT_PAGES`] pages, and whether
/// more follow.
async fn agents(host: &Host, manager: u64) -> Result<(Vec<Agent>, bool), Error> {
    let (listed, next) = pages(None, AGENT_PAGES, |after| {
        let ask = host.ask::<Query<Identity>>(identity::Query::Managed {
            by: manager,
            page: PageRequest { after, limit: None },
        });
        async move {
            match ask.await? {
                identity::Reply::Accounts(reply) => Ok((reply.items, reply.next)),
                _ => Err(wrong_reply()),
            }
        }
    })
    .await?;
    let agents = listed
        .into_iter()
        .map(|agent| match agent.kind() {
            kind @ Kind::Managed { .. } => Ok(Agent {
                number: agent.number,
                keys: agent.keys().len(),
                name: agent.card.name,
                kind,
            }),
            Kind::Person | Kind::Module(_) => Err(malformed(format!(
                "identity lists account {} as managed, and it is not",
                agent.number
            ))),
        })
        .collect::<Result<_, _>>()?;
    Ok((agents, next.is_some()))
}

async fn read_key(host: &Host, key: Vec<u8>, label: Option<String>) -> Result<Key, Error> {
    let membership = match host
        .ask::<Query<Valset>>(valset::Query::Membership { key: key.clone() })
        .await?
    {
        valset::Reply::Membership(membership) => membership,
        _ => return Err(wrong_reply()),
    };
    Ok(Key {
        label,
        key,
        validator: membership.is_some_and(|m| m.role == valset::Role::Validator),
    })
}
