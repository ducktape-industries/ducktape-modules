//! How a collection element's key becomes store bytes and comes back.
//! Integers are big-endian at their full width, so a scan's order is numeric
//! order; everything else is its borsh encoding; a tuple is the concatenation
//! of its parts, so a prefix scan over the first part works.

use abi::{Refusal, reason};
use borsh::{BorshDeserialize, BorshSerialize};

pub trait KeyCodec: Sized {
    fn encode_key(&self, out: &mut Vec<u8>);
    fn decode_key(bytes: &mut &[u8]) -> Result<Self, Refusal>;

    fn key_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_key(&mut out);
        out
    }
}

pub fn corrupt(what: impl Into<String>) -> Refusal {
    Refusal::new(reason::CORRUPT, what)
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], Refusal> {
    if bytes.len() < len {
        return Err(corrupt(format!(
            "a key ends after {} bytes where {len} were expected",
            bytes.len()
        )));
    }
    let (head, rest) = bytes.split_at(len);
    *bytes = rest;
    Ok(head)
}

macro_rules! integer {
    ($($t:ty),*) => {$(
        impl KeyCodec for $t {
            fn encode_key(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_be_bytes());
            }
            fn decode_key(bytes: &mut &[u8]) -> Result<Self, Refusal> {
                let width = core::mem::size_of::<$t>();
                Ok(<$t>::from_be_bytes(take(bytes, width)?.try_into().unwrap()))
            }
        }
    )*};
}

integer!(u8, u16, u32, u64, u128);

macro_rules! borsh {
    ($($t:ty),*) => {$(
        impl KeyCodec for $t {
            fn encode_key(&self, out: &mut Vec<u8>) {
                BorshSerialize::serialize(self, out).expect("an in-memory value encodes");
            }
            fn decode_key(bytes: &mut &[u8]) -> Result<Self, Refusal> {
                BorshDeserialize::deserialize(bytes)
                    .map_err(|e| corrupt(format!("a key does not decode: {e}")))
            }
        }
    )*};
}

borsh!((), Vec<u8>, String, [u8; 20], [u8; 32]);

macro_rules! tuple {
    ($($name:ident),+) => {
        impl<$($name: KeyCodec),+> KeyCodec for ($($name,)+) {
            fn encode_key(&self, out: &mut Vec<u8>) {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                $($name.encode_key(out);)+
            }
            fn decode_key(bytes: &mut &[u8]) -> Result<Self, Refusal> {
                Ok(($($name::decode_key(bytes)?,)+))
            }
        }
    };
}

tuple!(A, B);
tuple!(A, B, C);

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<K: KeyCodec + PartialEq + core::fmt::Debug>(key: K) -> Vec<u8> {
        let bytes = key.key_bytes();
        let mut rest = bytes.as_slice();
        assert_eq!(K::decode_key(&mut rest).unwrap(), key);
        assert!(rest.is_empty());
        bytes
    }

    #[test]
    fn integers_scan_in_numeric_order_and_tuples_concatenate() {
        assert!(round_trip(2u64) < round_trip(10u64));
        assert_eq!(round_trip(1u64), [0, 0, 0, 0, 0, 0, 0, 1]);
        let pair = round_trip((7u64, vec![9u8, 9]));
        assert!(pair.starts_with(&7u64.key_bytes()));
        round_trip((String::from("a"), 3u32, ()));
        assert!(u64::decode_key(&mut &[1u8][..]).is_err());
    }
}
