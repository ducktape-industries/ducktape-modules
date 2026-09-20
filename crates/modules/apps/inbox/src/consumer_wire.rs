pub mod attribution {
    use borsh::{BorshDeserialize, BorshSerialize};
    use sdk::{AccountNumber, Cause};
    use serde::{Deserialize, Serialize};

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(deny_unknown_fields)]
    pub struct Source {
        pub module: String,
        pub kind: String,
        pub object: String,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Actor {
        Account(AccountNumber),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(
        Serialize,
        Deserialize,
        BorshSerialize,
        BorshDeserialize,
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
    )]
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

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChangeKind {
        Added,
        Withdrawn,
        TransferredIn { from: AccountNumber },
        TransferredOut { to: AccountNumber },
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(deny_unknown_fields)]
    pub struct Change {
        pub seq: u64,
        pub source: Source,
        pub revision: u64,
        pub recipient: AccountNumber,
        pub reason: Reason,
        pub kind: ChangeKind,
        pub detail: Vec<u8>,
        pub actor: Actor,
        pub cause: Cause,
        pub height: u64,
    }

    impl Change {
        pub fn reference(&self) -> ChangeRef {
            ChangeRef {
                seq: self.seq,
                source: self.source.clone(),
                revision: self.revision,
                recipient: self.recipient,
                reason: self.reason.clone(),
                kind: self.kind.clone(),
                actor: self.actor.clone(),
                cause: self.cause.clone(),
                height: self.height,
            }
        }
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(deny_unknown_fields)]
    pub struct ChangeRef {
        pub seq: u64,
        pub source: Source,
        pub revision: u64,
        pub recipient: AccountNumber,
        pub reason: Reason,
        pub kind: ChangeKind,
        pub actor: Actor,
        pub cause: Cause,
        pub height: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionEvent {
        Changed(Change),
    }

    pub fn encode_event(value: &AttributionEvent) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_event(bytes: &[u8]) -> Result<AttributionEvent, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod identity {
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum KeyScheme {
        Ed25519,
        Secp256k1,
        Secp256r1,
    }

    #[cfg(test)]
    #[test]
    fn key_scheme_matches_producer_tags() {
        for (scheme, tag) in [
            (KeyScheme::Ed25519, "ed25519"),
            (KeyScheme::Secp256k1, "secp256k1"),
            (KeyScheme::Secp256r1, "secp256r1"),
        ] {
            let bytes = format!("\"{tag}\"").into_bytes();
            assert_eq!(sdk::wire::encode(&scheme), bytes);
            assert_eq!(sdk::wire::decode::<KeyScheme>(&bytes).unwrap(), scheme);
        }
    }
    use sdk::{AccountNumber, ModuleId};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
            controller: AccountNumber,
            executor: ModuleId,
            generation: u64,
            standing: ProgramStanding,
        },
        Revoked {
            controller: AccountNumber,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct KeyView {
        pub scheme: KeyScheme,
        pub pubkey: Vec<u8>,
        pub label: Option<String>,
        pub added_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AccountView {
        pub number: AccountNumber,
        pub name: String,
        pub control: Control,
        pub keys: Vec<KeyView>,
        pub avatar: Option<String>,
        pub bio: Option<String>,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        All {
            from: u64,
            limit: u64,
        },
        Get {
            number: AccountNumber,
        },
        OfKey {
            key: Vec<u8>,
        },
        Resolve {
            references: Vec<AccountRef>,
        },
        KeyGen {
            key: Vec<u8>,
        },
        Controlled {
            by: AccountNumber,
            from: u64,
            limit: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AccountRef {
        Account(AccountNumber),
        Key(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityReply {
        Accounts(Vec<AccountView>),
        Account(Option<AccountView>),
        Resolved(Vec<Option<AccountNumber>>),
        Gen(u64),
    }

    pub fn encode_query(value: &IdentityQuery) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_query(bytes: &[u8]) -> Result<IdentityQuery, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_reply(value: &IdentityReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        sdk::wire::decode(bytes)
    }
}
