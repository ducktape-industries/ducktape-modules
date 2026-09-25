//! Who acts on a program's state: the one shape every author, owner,
//! member, reviewer and grantee takes, and the one rule that turns a
//! signer into it. Every program that names people (chat, forge) resolves
//! its signer through [`party_of`], so the rule is written once.
use abi::{Origin, Refusal, reason};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use store::{KeyCodec, Reads, invalid, unauthorized};

use crate::AccountNumber;

/// The program derives the acting party from `Env.origin` at write time,
/// never from a payload. A person is an account, the identity her many keys
/// share: a key that holds none writes nothing (it only reads), so no row
/// ever names a bare key. A program that emitted the write is a module.
///
/// There is no default party: "nobody" is `Option<Party>::None`, never the
/// most trusted variant.
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
pub enum Party {
    /// a person: the identity account her signing key resolved to.
    Account(AccountNumber),
    /// a module that emitted the write as a follow-up.
    Module(String),
    /// genesis and system-internal writes.
    System,
}

impl Party {
    /// The party as a describe field shows it: an account or a program.
    pub fn value(&self) -> describe::Value {
        use describe::Value;
        match self {
            Party::Account(number) => Value::Account(*number),
            Party::Module(module) => Value::Program(module.clone()),
            Party::System => Value::text("system"),
        }
    }

    /// The account this party is, if it is one.
    pub fn account(&self) -> Option<AccountNumber> {
        match self {
            Party::Account(account) => Some(*account),
            Party::Module(_) | Party::System => None,
        }
    }

    /// A person, as opposed to trusted code: huddles and the `:` id
    /// namespace draw the line here.
    pub fn is_person(&self) -> bool {
        matches!(self, Party::Account(_))
    }

    /// The party a reader writes as, from the account the host resolved her
    /// seated key to. None while the key holds no account: every view gates
    /// its writes on this, the way [`party_of`] refuses them.
    pub fn writer(account: Option<AccountNumber>) -> Option<Party> {
        account.map(Party::Account)
    }

    /// A party typed by a person: an account number, `acct:<n>` too.
    pub fn parse(text: &str) -> Option<Party> {
        let text = text.trim();
        let number = text.strip_prefix("acct:").unwrap_or(text);
        number.parse().ok().map(Party::Account)
    }
}

/// A party in a table key: its borsh, encoded like any bytes.
impl KeyCodec for Party {
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
pub fn party_of(store: &impl Reads, origin: &Origin) -> Result<Party, Refusal> {
    Ok(match origin {
        Origin::External(key) if key.is_empty() => {
            return Err(invalid("an external origin carries a key"));
        }
        Origin::External(key) => match crate::account_of(store, key) {
            Ok(Some(number)) => Party::Account(number),
            Ok(None) => return Err(unauthorized(NO_ACCOUNT)),
            Err(r) if r.reason == reason::UNKNOWN_PROGRAM => return Err(unauthorized(NO_ACCOUNT)),
            Err(r) => return Err(r),
        },
        Origin::Program(id) => Party::Module(id.clone()),
        Origin::System => Party::System,
    })
}

/// The frame a typed `execute` runs in: who acts, at what height and time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub party: Party,
    pub height: u64,
    pub time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_party_round_trips_through_a_key_and_reads_back_from_input() {
        for party in [
            Party::Account(7),
            Party::Module("forge".into()),
            Party::System,
        ] {
            let bytes = party.key_bytes();
            let mut rest = bytes.as_slice();
            assert_eq!(Party::decode_key(&mut rest), Some(party));
            assert!(rest.is_empty());
        }
        assert_eq!(Party::parse(" 7 "), Some(Party::Account(7)));
        assert_eq!(Party::parse("acct:7"), Some(Party::Account(7)));
        for nothing in ["acct:x", "user:ab01", "ab01", "", "someone"] {
            assert_eq!(Party::parse(nothing), None, "{nothing}");
        }
        assert_eq!(Party::writer(Some(3)), Some(Party::Account(3)));
        assert_eq!(Party::writer(None), None);
    }
}
