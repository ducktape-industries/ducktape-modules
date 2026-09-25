//! The `identity` program: accounts, the keys and programs that control them,
//! and the consent by which a key joins an account. The types and rules are
//! always built; a view links them with `program` off. The `program` feature
//! adds the wasm32 program over the host (`program.rs`). The `view` feature
//! adds the ask a view makes of identity directly (`view.rs`).
#[cfg(feature = "program")]
mod program;
mod party;
mod rules;
#[cfg(test)]
mod tests;
#[cfg(feature = "view")]
pub mod view;

pub use party::{Frame, NO_ACCOUNT, Party, party_of};
pub use rules::{execute, query};

use abi::{BlobId, ProgramId, Scheme};
use borsh::{BorshDeserialize, BorshSerialize};
use module_registry::{Page, PageReply};

pub type AccountNumber = u64;

pub const PROGRAM: &str = "identity";
pub const CONSENT_NAMESPACE: &[u8] = b"ducktape:identity:consent";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Key {
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub label: Option<String>,
    pub added_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Standing {
    Active,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Control {
    Keys(Vec<Key>),
    Program {
        executor: ProgramId,
        controller: AccountNumber,
        standing: Standing,
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
            Control::Program { standing, .. } => *standing == Standing::Active,
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
    SetStanding {
        account: AccountNumber,
        standing: Standing,
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
    Get { number: AccountNumber },
    OfKey { key: Vec<u8> },
    Generation { key: Vec<u8> },
    Resolve { references: Vec<Reference> },
    List { page: Page },
    Controlled { by: AccountNumber, page: Page },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Account(Option<Account>),
    Number(Option<AccountNumber>),
    Generation(u64),
    Resolved(Vec<Option<AccountNumber>>),
    Accounts(PageReply<Account>),
}

pub fn principal(number: AccountNumber) -> Vec<u8> {
    number.to_le_bytes().to_vec()
}

pub fn account_of_principal(bytes: &[u8]) -> Option<AccountNumber> {
    <[u8; 8]>::try_from(bytes).ok().map(u64::from_le_bytes)
}

/// The asks another program makes of identity.
pub fn account_of(
    ctx: &impl store::Reads,
    key: &[u8],
) -> Result<Option<AccountNumber>, abi::Refusal> {
    match ctx.ask::<Query, Reply>(PROGRAM, &Query::OfKey { key: key.to_vec() })? {
        Reply::Number(number) => Ok(number),
        other => Err(abi::Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            format!("identity answered OfKey with {other:?}"),
        )),
    }
}

pub fn account(
    ctx: &impl store::Reads,
    number: AccountNumber,
) -> Result<Option<Account>, abi::Refusal> {
    match ctx.ask::<Query, Reply>(PROGRAM, &Query::Get { number })? {
        Reply::Account(account) => Ok(account),
        other => Err(abi::Refusal::new(
            abi::reason::UNEXPECTED_REPLY,
            format!("identity answered Get with {other:?}"),
        )),
    }
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
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
        Op::SetStanding {
            account: number,
            standing,
        } => (
            "Set standing".into(),
            vec![
                account(number),
                field(
                    "standing",
                    Value::text(match standing {
                        Standing::Active => "active",
                        Standing::Suspended => "suspended",
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
            "SetStanding",
            "TransferControl",
            "Revoke",
        ]
    );
}
