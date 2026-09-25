//! Who acts on a module's state, as resolved: an [`guest::Origin`] is the
//! raw caller, a [`Principal`] who is acting. The one shape every author,
//! owner, member, reviewer and grantee takes, and the one rule that turns a
//! signer into it. Every module that names people (chat, forge) resolves
//! its signer through [`principal_of`], so the rule is written once.
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{Error, Origin, code};
use guest::{QueryCtx, invalid, unauthorized};
use serde::{Deserialize, Serialize};
use store::KeyCodec;

use crate::AccountNumber;

/// The module derives the acting principal from `Env.origin` at write time,
/// never from a payload. A person is an account, the identity her many keys
/// share: a key that holds none writes nothing (it only reads), so no row
/// ever names a bare key. A module that emitted the write is `Module`.
///
/// There is no default principal: "nobody" is `Option<Principal>::None`,
/// never the most trusted variant.
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
    Hash,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Principal {
    /// a person: the identity account her signing key resolved to.
    Account(AccountNumber),
    /// a module that emitted the write as a follow-up.
    Module(String),
    /// genesis and system-internal writes.
    Root,
}

impl Principal {
    /// The principal as a describe field shows it: an account or a module.
    pub fn value(&self) -> describe::Value {
        use describe::Value;
        match self {
            Principal::Account(number) => Value::Account(*number),
            Principal::Module(module) => Value::Module(module.clone()),
            Principal::Root => Value::text("system"),
        }
    }

    /// The account this principal is, if it is one.
    pub fn account(&self) -> Option<AccountNumber> {
        match self {
            Principal::Account(account) => Some(*account),
            Principal::Module(_) | Principal::Root => None,
        }
    }

    /// A person, as opposed to trusted code: huddles and the `:` id
    /// namespace draw the line here.
    pub fn is_person(&self) -> bool {
        matches!(self, Principal::Account(_))
    }

    /// The principal a reader writes as, from the account the host resolved her
    /// seated key to. None while the key holds no account: every view gates
    /// its writes on this, the way [`principal_of`] refuses them.
    pub fn writer(account: Option<AccountNumber>) -> Option<Principal> {
        account.map(Principal::Account)
    }

    /// A principal typed by a person: an account number, `acct:<n>` too.
    pub fn parse(text: &str) -> Option<Principal> {
        let text = text.trim();
        let number = text.strip_prefix("acct:").unwrap_or(text);
        number.parse().ok().map(Principal::Account)
    }
}

/// A principal in a table key: its borsh, encoded like any bytes.
impl KeyCodec for Principal {
    fn encode_key(&self, out: &mut Vec<u8>) {
        abi::encode(self).encode_key(out);
    }
    fn decode_key(bytes: &mut &[u8]) -> Option<Self> {
        abi::decode(&Vec::<u8>::decode_key(bytes)?).ok()
    }
}

/// The refusal a key that holds no account meets on every write.
pub const NO_ACCOUNT: &str = "a person writes through an account, and this key holds none";

/// Who an origin is: a key is the account identity says holds it; a key
/// that holds none (or any key while identity is not deployed) is refused,
/// since only an account writes as a person.
pub fn principal_of(ctx: &QueryCtx, origin: &Origin) -> Result<Principal, Error> {
    Ok(match origin {
        Origin::Signed(key) if key.is_empty() => {
            return Err(invalid("an external origin carries a key"));
        }
        Origin::Signed(key) => match crate::account_of(ctx, key) {
            Ok(Some(number)) => Principal::Account(number),
            Ok(None) => return Err(unauthorized(NO_ACCOUNT)),
            Err(r) if r.code == code::UNKNOWN_PROGRAM => return Err(unauthorized(NO_ACCOUNT)),
            Err(r) => return Err(r),
        },
        Origin::Module(id) => Principal::Module(id.clone()),
        Origin::Root => Principal::Root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_principal_round_trips_through_a_key_and_reads_back_from_input() {
        for principal in [
            Principal::Account(7),
            Principal::Module("forge".into()),
            Principal::Root,
        ] {
            let bytes = principal.key_bytes();
            let mut rest = bytes.as_slice();
            assert_eq!(Principal::decode_key(&mut rest), Some(principal));
            assert!(rest.is_empty());
        }
        assert_eq!(Principal::parse(" 7 "), Some(Principal::Account(7)));
        assert_eq!(Principal::parse("acct:7"), Some(Principal::Account(7)));
        for nothing in ["acct:x", "user:ab01", "ab01", "", "someone"] {
            assert_eq!(Principal::parse(nothing), None, "{nothing}");
        }
        assert_eq!(Principal::writer(Some(3)), Some(Principal::Account(3)));
        assert_eq!(Principal::writer(None), None);
    }
}
