use serde::{Deserialize, Serialize};

pub(crate) mod kv {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub(crate) enum KvMsg {
        Set { key: Vec<u8>, value: Vec<u8> },
    }

    pub(crate) fn encode(msg: &KvMsg) -> Vec<u8> {
        sdk::wire::encode(msg)
    }
}
