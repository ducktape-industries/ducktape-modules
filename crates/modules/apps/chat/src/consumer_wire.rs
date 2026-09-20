use serde::{Deserialize, Serialize};

pub(crate) mod attribution {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ObjectRef {
        pub kind: String,
        pub object: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Actor {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Reason {
        Mention,
        Authorship,
        Ownership,
        Assignment,
        Credit,
        Result,
        Report,
        Defined(String),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Relation {
        pub recipient: u64,
        pub reason: Reason,
        pub detail: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Transfer {
        pub reason: Reason,
        pub from: u64,
        pub to: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AttributionUpdate {
        pub object: ObjectRef,
        pub revision: u64,
        pub actor: Actor,
        pub relations: Vec<Relation>,
        pub transfers: Vec<Transfer>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionMsg {
        Attribute {
            object: ObjectRef,
            revision: u64,
            actor: Actor,
            relations: Vec<Relation>,
            transfers: Vec<Transfer>,
        },
        AttributeBatch {
            updates: Vec<AttributionUpdate>,
        },
        Subscribe {},
    }

    pub fn encode_msg(message: &AttributionMsg) -> Vec<u8> {
        sdk::wire::encode(message)
    }
}

pub(crate) mod identity {
    use super::*;

    /// the closed signature-scheme set identity spells a key with — mirrored
    /// here as spelling only: chat verifies nothing, so it links no crypto.
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum KeyScheme {
        Ed25519,
        Secp256k1,
        Secp256r1,
    }

    pub const MAX_QUERY_LIMIT: u64 = 256;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct KeyView {
        pub scheme: KeyScheme,
        pub pubkey: Vec<u8>,
        pub label: Option<String>,
        pub added_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Control {
        Keys,
        Program {
            controller: u64,
            executor: String,
            generation: u64,
            standing: ProgramStanding,
        },
        Revoked {
            controller: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AccountView {
        pub number: u64,
        pub name: String,
        pub control: Control,
        pub keys: Vec<KeyView>,
        pub avatar: Option<String>,
        pub bio: Option<String>,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AccountRef {
        Account(u64),
        Key(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        All { from: u64, limit: u64 },
        Get { number: u64 },
        OfKey { key: Vec<u8> },
        Resolve { references: Vec<AccountRef> },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityReply {
        Accounts(Vec<AccountView>),
        Account(Option<AccountView>),
        Resolved(Vec<Option<u64>>),
        Gen(u64),
    }

    pub fn encode_query(query: &IdentityQuery) -> Vec<u8> {
        sdk::wire::encode(query)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }
}
