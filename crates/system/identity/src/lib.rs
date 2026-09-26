//! The `identity` program: accounts, the keys and programs that control them,
//! and the consent by which a key joins an account. The types, rules and
//! [`Identity`] module are always built; a view links them with `module`
//! off. The `module` feature adds its wasm exports. The `view` feature
//! adds the ask a view makes of identity directly (`view.rs`).
mod program;
mod rules;
#[cfg(test)]
mod tests;
#[cfg(feature = "view")]
pub mod view;

pub use guest::AccountNumber;
pub use program::Identity;

use borsh::{BorshDeserialize, BorshSerialize};
use guest::{BlobId, ModuleId, Scheme};
use module_registry::{PageRequest, PageResponse};

pub const MODULE: &str = "identity";
pub const CONSENT_NAMESPACE: &[u8] = b"ducktape:identity:consent";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Key {
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub label: Option<String>,
    pub added_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Status {
    Active,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Control {
    Keys(Vec<Key>),
    Program {
        executor: ModuleId,
        controller: AccountNumber,
        status: Status,
    },
    Revoked {
        controller: AccountNumber,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    pub number: AccountNumber,
    pub name: String,
    pub control: Control,
    pub avatar: Option<BlobId>,
    pub bio: Option<String>,
    pub updated_at: u64,
}

impl Account {
    pub fn keys(&self) -> &[Key] {
        match &self.control {
            Control::Keys(keys) => keys,
            Control::Program { .. } | Control::Revoked { .. } => &[],
        }
    }

    pub fn holds(&self, key: &[u8]) -> bool {
        self.keys().iter().any(|held| held.key == key)
    }

    pub fn live(&self) -> bool {
        match &self.control {
            Control::Keys(_) => true,
            Control::Program { status, .. } => *status == Status::Active,
            Control::Revoked { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Consent {
    pub key: Vec<u8>,
    pub account: AccountNumber,
    pub expires_at: u64,
    pub proof: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Admission {
    pub network: Vec<u8>,
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub generation: u64,
    pub account: AccountNumber,
    pub expires_at: u64,
}

impl Admission {
    pub fn preimage(&self) -> Vec<u8> {
        abi::encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Create {
        name: String,
        scheme: Scheme,
    },
    AddKey {
        scheme: Scheme,
        label: Option<String>,
        consent: Consent,
    },
    RemoveKey {
        key: Vec<u8>,
    },
    SetName {
        account: AccountNumber,
        name: String,
    },
    SetProfile {
        account: AccountNumber,
        avatar: Option<BlobId>,
        bio: Option<String>,
    },
    CreateProgram {
        name: String,
        controller: AccountNumber,
    },
    SetStatus {
        account: AccountNumber,
        status: Status,
    },
    TransferControl {
        account: AccountNumber,
        to: AccountNumber,
    },
    Revoke {
        account: AccountNumber,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reference {
    Account(AccountNumber),
    Key(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    /// The identity role's query (`abi::role::identity::Query::Account`),
    /// first and in its order: the account that holds a key.
    OfKey {
        key: Vec<u8>,
    },
    Get {
        number: AccountNumber,
    },
    Generation {
        key: Vec<u8>,
    },
    Resolve {
        references: Vec<Reference>,
    },
    List {
        page: PageRequest,
    },
    Controlled {
        by: AccountNumber,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    /// The identity role's reply (`abi::role::identity::Reply::Account`).
    Number(Option<AccountNumber>),
    Account(Option<Account>),
    Generation(u64),
    Resolved(Vec<Option<AccountNumber>>),
    Accounts(PageResponse<Account>),
}

/// The asks another module makes of identity.
pub fn account_of(
    ctx: &guest::QueryCtx,
    key: &[u8],
) -> Result<Option<AccountNumber>, guest::Error> {
    match ctx.ask::<Query, Reply>(MODULE, &Query::OfKey { key: key.to_vec() })? {
        Reply::Number(number) => Ok(number),
        other => Err(guest::Error::new(
            guest::code::UNEXPECTED_REPLY,
            format!("identity answered OfKey with {other:?}"),
        )),
    }
}

pub fn account(
    ctx: &guest::QueryCtx,
    number: AccountNumber,
) -> Result<Option<Account>, guest::Error> {
    match ctx.ask::<Query, Reply>(MODULE, &Query::Get { number })? {
        Reply::Account(account) => Ok(account),
        other => Err(guest::Error::new(
            guest::code::UNEXPECTED_REPLY,
            format!("identity answered Get with {other:?}"),
        )),
    }
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this module ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let account = |number: &AccountNumber| field("account", Value::Account(*number));
    let optional = |text: &Option<String>| Value::Text(text.clone().unwrap_or_else(|| "—".into()));
    let scheme = |scheme: &abi::Scheme| {
        field(
            "scheme",
            Value::text(match scheme {
                abi::Scheme::Ed25519 => "Ed25519",
                abi::Scheme::Secp256k1 => "secp256k1",
                abi::Scheme::Secp256r1 => "secp256r1",
                abi::Scheme::Bls12381 => "BLS12-381",
            }),
        )
    };
    let (title, fields) = match op {
        Op::Create { name, scheme: s } => (
            format!("Create · {name}"),
            vec![field("name", Value::text(name)), scheme(s)],
        ),
        Op::AddKey {
            scheme: s,
            label,
            consent,
        } => (
            "Add key".into(),
            vec![
                scheme(s),
                field("label", optional(label)),
                field("key", Value::Key(consent.key.clone())),
                account(&consent.account),
                field("expires at", Value::Time(consent.expires_at)),
            ],
        ),
        Op::RemoveKey { key } => (
            "Remove key".into(),
            vec![field("key", Value::Key(key.clone()))],
        ),
        Op::SetName {
            account: number,
            name,
        } => (
            format!("Set name · {name}"),
            vec![account(number), field("name", Value::text(name))],
        ),
        Op::SetProfile {
            account: number,
            avatar,
            bio,
        } => (
            "Set profile".into(),
            vec![
                account(number),
                field(
                    "avatar",
                    avatar.map_or_else(
                        || Value::text("—"),
                        |blob| Value::Hash(blob.digest().to_vec()),
                    ),
                ),
                field("bio", optional(bio)),
            ],
        ),
        Op::CreateProgram { name, controller } => (
            format!("Create program · {name}"),
            vec![
                field("name", Value::text(name)),
                field("controller", Value::Account(*controller)),
            ],
        ),
        Op::SetStatus {
            account: number,
            status,
        } => (
            "Set status".into(),
            vec![
                account(number),
                field(
                    "status",
                    Value::text(match status {
                        Status::Active => "active",
                        Status::Suspended => "suspended",
                    }),
                ),
            ],
        ),
        Op::TransferControl {
            account: number,
            to,
        } => (
            "Transfer control".into(),
            vec![account(number), field("to", Value::Account(*to))],
        ),
        Op::Revoke { account: number } => ("Revoke".into(), vec![account(number)]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Create",
            "AddKey",
            "RemoveKey",
            "SetName",
            "SetProfile",
            "CreateProgram",
            "SetStatus",
            "TransferControl",
            "Revoke",
        ]
    );
}
