//! The `identity` program: accounts, and who acts as each. Everything that
//! acts is an account: a person (the keys she holds), an agent (the keys its
//! manager, a person, adds) and a module (the account the kernel registers
//! as it admits the module). The types, rules and [`Identity`] module are
//! always built; a view links them with `module` off. The `module` feature
//! adds its wasm exports. The `view` feature adds the ask a view makes of
//! identity directly (`view.rs`).
mod program;
mod rules;
#[cfg(test)]
mod tests;
#[cfg(feature = "view")]
pub mod view;

pub use abi::role::identity::{Category, Profile, Status};
pub use guest::AccountNumber;
pub use program::Identity;

use borsh::{BorshDeserialize, BorshSerialize};
use guest::{BlobId, ModuleId, Scheme};
pub use store::{PageRequest, PageResponse};

pub const MODULE: &str = "identity";
pub const CONSENT_NAMESPACE: &[u8] = b"ducktape:identity:consent";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Key {
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub label: Option<String>,
    pub added_at: u64,
}

/// One account. A person holds keys and has no manager; an agent is
/// managed by a person and holds the keys that person adds; a module's
/// account holds no keys and names its `module`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    pub number: AccountNumber,
    pub name: String,
    pub avatar: Option<BlobId>,
    pub bio: Option<String>,
    pub updated_at: u64,
    /// who acts as this account
    pub keys: Vec<Key>,
    /// set for a module's account
    pub module: Option<ModuleId>,
    /// structure the chain enforces: one level deep, a person
    pub manager: Option<AccountNumber>,
    pub status: Status,
    /// a label the manager answers for; only a managed account has one
    pub category: Option<Category>,
}

impl Account {
    pub fn holds(&self, key: &[u8]) -> bool {
        self.keys.iter().any(|held| held.key == key)
    }

    /// A person: neither managed nor a module's.
    pub fn is_person(&self) -> bool {
        self.manager.is_none() && self.module.is_none()
    }

    /// How others show this account.
    pub fn profile(&self) -> Profile {
        Profile {
            number: self.number,
            name: self.name.clone(),
            category: self.category,
            manager: self.manager,
            module: self.module.clone(),
            status: self.status,
        }
    }
}

/// A key's consent to a new key joining an account: `proof` is `key`'s
/// signature over the [`Admission`]. For a person, `key` is one already on
/// the account and the new key signs the frame; for an agent, `key` is the
/// new key itself and the manager signs the frame.
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
    /// The identity role's op (`abi::role::identity::Op`), first: the
    /// system gives an admitted module its account.
    RegisterModule { module: ModuleId },
    /// A person's account, holding the signing key.
    Create { name: String, scheme: Scheme },
    /// An agent the signer's account manages; it holds no key yet.
    CreateAgent { name: String },
    AddKey {
        scheme: Scheme,
        label: Option<String>,
        consent: Consent,
    },
    RemoveKey {
        account: AccountNumber,
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
    SetStatus {
        account: AccountNumber,
        status: Status,
    },
    TransferManager {
        account: AccountNumber,
        to: AccountNumber,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reference {
    Account(AccountNumber),
    Key(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Query {
    /// The identity role's queries (`abi::role::identity::Query`), first
    /// and in its order: the account a key acts as (refused while it is not
    /// live), every account's profile, and a module's account.
    OfKey {
        key: Vec<u8>,
    },
    Profiles {
        after: Option<AccountNumber>,
        limit: u32,
    },
    OfModule {
        module: ModuleId,
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
    Managed {
        by: AccountNumber,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    /// The identity role's replies (`abi::role::identity::Reply`).
    Number(Option<AccountNumber>),
    Profiles {
        profiles: Vec<Profile>,
        next: Option<AccountNumber>,
    },
    Account(Option<Account>),
    Generation(u64),
    Resolved(Vec<Option<AccountNumber>>),
    Accounts(PageResponse<Account>),
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
    let status = |status: &Status| {
        Value::text(match status {
            Status::Active => "active",
            Status::Suspended => "suspended",
            Status::Revoked => "revoked",
        })
    };
    let (title, fields) = match op {
        Op::RegisterModule { module } => (
            format!("Register module · {module}"),
            vec![field("module", Value::Module(module.clone()))],
        ),
        Op::Create { name, scheme: s } => (
            format!("Create · {name}"),
            vec![field("name", Value::text(name)), scheme(s)],
        ),
        Op::CreateAgent { name } => (
            format!("Create agent · {name}"),
            vec![field("name", Value::text(name))],
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
        Op::RemoveKey {
            account: number,
            key,
        } => (
            "Remove key".into(),
            vec![account(number), field("key", Value::Key(key.clone()))],
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
        Op::SetStatus {
            account: number,
            status: to,
        } => (
            "Set status".into(),
            vec![account(number), field("status", status(to))],
        ),
        Op::TransferManager {
            account: number,
            to,
        } => (
            "Transfer manager".into(),
            vec![account(number), field("to", Value::Account(*to))],
        ),
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
            "RegisterModule",
            "Create",
            "CreateAgent",
            "AddKey",
            "RemoveKey",
            "SetName",
            "SetProfile",
            "SetStatus",
            "TransferManager",
        ]
    );
}
