//! Who acts on chat state: the one shape every author, owner, member,
//! huddle seat, reactor and mention target takes.
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use store::KeyCodec;

pub type AccountNumber = u64;

/// The program derives the acting party from `Env.origin` at write time,
/// never from a payload: a key resolves through identity to the account
/// holding it, a key identity does not know stays a key, a program that
/// emitted the write is a module. An account is the identity a person's
/// many keys share, so it is what rows name whenever one exists.
#[derive(
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Party {
    /// an identity account: a resolved member key, or the program account the
    /// host ran the write as.
    Account(AccountNumber),
    /// an authenticated signing key that holds no account (non-empty).
    Key(Vec<u8>),
    /// a module that emitted the write as a follow-up.
    Module(String),
    /// genesis and system-internal writes; also a row no one authored yet.
    #[default]
    System,
}

impl Party {
    /// The account this party is, if it is one.
    pub fn account(&self) -> Option<AccountNumber> {
        match self {
            Party::Account(account) => Some(*account),
            Party::Key(_) | Party::Module(_) | Party::System => None,
        }
    }

    /// A person (an account or a key), as opposed to trusted code: huddles
    /// and the `:` id namespace draw the line here.
    pub fn is_person(&self) -> bool {
        matches!(self, Party::Account(_) | Party::Key(_))
    }

    /// The party a reader writes as: her account once identity has named
    /// one, else her seated key (`key_hex`); none with no key seated.
    pub fn reader(account: Option<AccountNumber>, key_hex: &str) -> Option<Party> {
        match account {
            Some(number) => Some(Party::Account(number)),
            None => abi::unhex(key_hex)
                .filter(|key| !key.is_empty())
                .map(Party::Key),
        }
    }

    /// A party typed by a person: an account number, or a key in hex.
    /// `acct:<n>` and `user:<hex>` are read too.
    pub fn parse(text: &str) -> Option<Party> {
        let text = text.trim();
        if let Some(number) = text.strip_prefix("acct:") {
            return number.parse().ok().map(Party::Account);
        }
        if let Ok(number) = text.parse() {
            return Some(Party::Account(number));
        }
        let key = text.strip_prefix("user:").unwrap_or(text);
        abi::unhex(key)
            .filter(|key| !key.is_empty())
            .map(Party::Key)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_party_round_trips_through_a_key_and_reads_back_from_input() {
        for party in [
            Party::Account(7),
            Party::Key(vec![0xab, 0x01]),
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
        assert_eq!(Party::parse("user:ab01"), Some(Party::Key(vec![0xab, 1])));
        assert_eq!(Party::parse("ab01"), Some(Party::Key(vec![0xab, 1])));
        for nothing in ["acct:x", "user:", "", "zz", "someone"] {
            assert_eq!(Party::parse(nothing), None, "{nothing}");
        }
        assert_eq!(Party::reader(Some(3), "ab"), Some(Party::Account(3)));
        assert_eq!(Party::reader(None, "ab"), Some(Party::Key(vec![0xab])));
        assert_eq!(Party::reader(None, ""), None);
    }
}
