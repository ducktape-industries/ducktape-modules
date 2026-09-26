use crate::api::{Identity, Valset};
use ducktape_view_guest::{
    Host,
    host::{Error, malformed},
    methods::Query,
};

use identity::{Control, Kind, PageRequest, Standing};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub number: Option<u64>,
    pub name: String,
    pub keys: Vec<Key>,
    /// a person's account manages agents; an agent's or a module's none
    pub manages: bool,
    pub agents: Vec<Agent>,
}
/// An agent the account manages, as its line reads.
#[derive(Clone, Serialize, Deserialize)]
pub struct Agent {
    pub number: u64,
    pub name: String,
    pub keys: usize,
    #[serde(with = "identity::view::borsh_bytes")]
    pub standing: Standing,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Key {
    pub label: String,
    pub key: String,
    pub validator: bool,
}
/// Who the seated `key` is: its account as the host resolved it (`number`),
/// with every key on it, or the key alone while it holds none.
pub async fn read_account(
    host: Host,
    key: String,
    number: Option<u64>,
) -> Result<Option<Account>, Error> {
    if key.is_empty() {
        return Ok(None);
    }
    if !key.len().is_multiple_of(2) || !key.is_ascii() {
        return Err(malformed("Host account key is not hexadecimal".into()));
    }
    let key = (0..key.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&key[i..i + 2], 16).map_err(|e| malformed(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(number) = number else {
        return Ok(Some(Account {
            number: None,
            name: "Unregistered key".into(),
            keys: vec![read_key(&host, &key, "Host key".into()).await?],
            manages: false,
            agents: Vec::new(),
        }));
    };
    let account = match host
        .ask::<Query<Identity>>(identity::Query::Get { number })
        .await?
    {
        identity::Reply::Account(a) => a,
        other => return Err(malformed(format!("identity answered Get with {other:?}"))),
    };
    let Some(account) = account else {
        return Ok(None);
    };
    let mut keys = Vec::new();
    for key in account.keys() {
        keys.push(
            read_key(
                &host,
                &key.key,
                key.label.clone().unwrap_or_else(|| "Key".into()),
            )
            .await?,
        );
    }
    let manages = matches!(account.control, Control::Person { .. });
    let agents = if manages {
        read_agents(&host, number).await?
    } else {
        Vec::new()
    };
    Ok(Some(Account {
        number: Some(number),
        name: account.card.name,
        keys,
        manages,
        agents,
    }))
}

/// Every agent `manager` manages, every page of them.
async fn read_agents(host: &Host, manager: u64) -> Result<Vec<Agent>, Error> {
    let mut agents = Vec::new();
    let mut after = None;
    loop {
        let page = PageRequest { after, limit: None };
        let asked = identity::Query::Managed { by: manager, page };
        let reply = match host.ask::<Query<Identity>>(asked).await? {
            identity::Reply::Accounts(reply) => reply,
            other => {
                return Err(malformed(format!(
                    "identity answered Managed with {other:?}"
                )));
            }
        };
        for agent in reply.items {
            let Kind::Managed { standing, .. } = agent.kind() else {
                return Err(malformed(format!(
                    "identity lists account {} as managed, and it is not",
                    agent.number
                )));
            };
            agents.push(Agent {
                number: agent.number,
                keys: agent.keys().len(),
                name: agent.card.name,
                standing,
            });
        }
        match reply.next {
            Some(next) => after = Some(next),
            None => return Ok(agents),
        }
    }
}

async fn read_key(host: &Host, key: &[u8], label: String) -> Result<Key, Error> {
    let membership = match host
        .ask::<Query<Valset>>(valset::Query::Membership { key: key.to_vec() })
        .await?
    {
        valset::Reply::Membership(m) => m,
        other => {
            return Err(malformed(format!(
                "valset answered Membership with {other:?}"
            )));
        }
    };
    Ok(Key {
        label,
        key: ducktape_view_guest::design::short_hex(&abi::hex(key)),
        validator: matches!(membership.map(|m| m.role), Some(valset::Role::Validator)),
    })
}
