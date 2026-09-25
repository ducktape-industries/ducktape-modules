use crate::api::{Identity, Valset};
use ducktape_view_guest::{
    Host,
    host::{Refusal, malformed},
    methods::Query,
};

use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub number: Option<u64>,
    pub name: String,
    pub keys: Vec<Key>,
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
) -> Result<Option<Account>, Refusal> {
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
    Ok(Some(Account {
        number: Some(number),
        name: account.name,
        keys,
    }))
}

async fn read_key(host: &Host, key: &[u8], label: String) -> Result<Key, Refusal> {
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
