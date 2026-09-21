//! The small, owned interfaces Agent consumes from sibling modules.
//!
//! These types intentionally do not link a sibling module or its wire crate.
//! Their field order, variant names, and envelope shape mirror the SDK wire
//! codecs at the pinned source revision.

pub mod attribution {
    use borsh::{BorshDeserialize, BorshSerialize};
    use sdk::{AccountNumber, Cause, DeliveryOutcome, ModuleId, Root};
    use serde::{Deserialize, Serialize};

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(deny_unknown_fields)]
    pub struct ObjectRef {
        pub kind: String,
        pub object: String,
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
    #[serde(deny_unknown_fields)]
    pub struct Relation {
        pub recipient: AccountNumber,
        pub reason: Reason,
        pub detail: Vec<u8>,
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
    #[serde(deny_unknown_fields)]
    pub struct Transfer {
        pub reason: Reason,
        pub from: AccountNumber,
        pub to: AccountNumber,
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

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChangeEntry {
        pub at: u64,
        pub change: Change,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionQuery {
        Relations {
            source: Source,
        },
        Changes {
            after: u64,
            limit: u64,
        },
        ChangesFor {
            recipient: AccountNumber,
            after: u64,
            limit: u64,
        },
        ChangesOf {
            source: Source,
            after: u64,
            limit: u64,
        },
        Subscribers,
        DeliveriesOf {
            subscriber: ModuleId,
            after: u64,
            limit: u64,
        },
        DeliveryOf {
            subscriber: ModuleId,
            seq: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionReply {
        Relations(Option<ObjectRelations>),
        Changes(Vec<ChangeEntry>),
        Subscribers(Vec<ModuleId>),
        Deliveries(Vec<DeliveryEntry>),
        Delivery(Option<Delivery>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ObjectRelations {
        pub source: Source,
        pub revision: u64,
        pub relations: Vec<Relation>,
        pub changes: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DeliveryState {
        Queued,
        Retired(DeliveryOutcome),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Delivery {
        pub item: u64,
        pub subscriber: ModuleId,
        pub seq: u64,
        pub root: Root,
        pub state: DeliveryState,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct DeliveryEntry {
        pub at: u64,
        pub delivery: Delivery,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionEvent {
        Changed(Change),
    }

    pub fn encode_msg(value: &AttributionMsg) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_msg(bytes: &[u8]) -> Result<AttributionMsg, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_query(value: &AttributionQuery) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_query(bytes: &[u8]) -> Result<AttributionQuery, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_reply(value: &AttributionReply) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<AttributionReply, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_event(value: &AttributionEvent) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_event(bytes: &[u8]) -> Result<AttributionEvent, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod dispatch {
    use borsh::{BorshDeserialize, BorshSerialize};
    use sdk::{AccountNumber, CallId, ModuleId};
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AdmissionPolicy {
        #[default]
        Queue,
        FailFast,
    }

    impl AdmissionPolicy {
        fn is_queue(&self) -> bool {
            *self == Self::Queue
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ResultEvent {
        pub dispatch_id: String,
        pub recipe_id: String,
        pub outcome: Result<Vec<u8>, String>,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Refusal {
        NotAProgram,
        Revoked,
        Suspended,
        StaleGeneration,
        WrongExecutor,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Attempt {
        Applied,
        Rejected,
        Refused,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum CallOutcome {
        Applied { output: Vec<u8>, assigned: Vec<u8> },
        Rejected { reason: String },
        Refused(Refusal),
        Unrepresentable { attempted: Attempt },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct CallCompleted {
        pub id: CallId,
        pub account: AccountNumber,
        pub outcome: CallOutcome,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Delivery {
        Result(ResultEvent),
        CallCompleted(CallCompleted),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DispatchMsg {
        Dispatch {
            dispatch_id: String,
            recipe_id: String,
            payload: Vec<u8>,
            demands: BTreeMap<String, u64>,
            #[serde(default, skip_serializing_if = "AdmissionPolicy::is_queue")]
            admission: AdmissionPolicy,
        },
        Call {
            invocation: String,
            step: u64,
            account: AccountNumber,
            target: ModuleId,
            payload: Vec<u8>,
        },
    }

    pub fn encode_msg(value: &DispatchMsg) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_msg(bytes: &[u8]) -> Result<DispatchMsg, String> {
        sdk::wire::decode(bytes)
    }

    pub fn encode_delivery(value: &Delivery) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_delivery(bytes: &[u8]) -> Result<Delivery, String> {
        sdk::wire::decode(bytes)
    }
}

pub mod identity {
    use borsh::{BorshDeserialize, BorshSerialize};
    use sdk::{AccountNumber, ModuleId, Origin};
    use serde::{Deserialize, Serialize};

    #[derive(
        Serialize,
        Deserialize,
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        BorshSerialize,
        BorshDeserialize,
    )]
    #[serde(rename_all = "snake_case")]
    pub enum KeyScheme {
        Ed25519,
        Secp256k1,
        Secp256r1,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct KeyView {
        pub scheme: KeyScheme,
        pub pubkey: Vec<u8>,
        pub label: Option<String>,
        pub added_at: u64,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq,
    )]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    #[derive(
        Serialize, Deserialize, BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq,
    )]
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
    pub enum IdentityMsg {
        Create {
            name: String,
            scheme: KeyScheme,
        },
        CreateProgram {
            name: String,
            controller: AccountNumber,
            request: u64,
        },
        SetProgramStanding {
            account: AccountNumber,
            standing: ProgramStanding,
        },
        TransferControl {
            account: AccountNumber,
            to: AccountNumber,
        },
        RevokeProgram {
            account: AccountNumber,
        },
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

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityEvent {
        ProgramCreated {
            request: u64,
            account: AccountNumber,
            controller: AccountNumber,
        },
    }

    pub fn authenticate_event(
        origin: &Origin,
        identity: &str,
        payload: &[u8],
    ) -> Result<IdentityEvent, String> {
        let emitted_by_identity = matches!(origin, Origin::Module(module) if module == identity);
        if !emitted_by_identity {
            return Err(format!(
                "identity events are authenticated by origin: expected Module({identity:?}), got {origin:?}"
            ));
        }
        decode_event(payload)
    }

    pub fn encode_msg(value: &IdentityMsg) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_msg(bytes: &[u8]) -> Result<IdentityMsg, String> {
        sdk::wire::decode(bytes)
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

    pub fn encode_event(value: &IdentityEvent) -> Vec<u8> {
        sdk::wire::encode(value)
    }

    pub fn decode_event(bytes: &[u8]) -> Result<IdentityEvent, String> {
        sdk::wire::decode(bytes)
    }
}
