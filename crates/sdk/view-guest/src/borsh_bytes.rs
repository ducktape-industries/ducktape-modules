//! A borsh value in a view's serde snapshot, as its bytes:
//! `#[serde(with = "ducktape_view_guest::borsh_bytes")]` on a field whose
//! type is a module's (borsh-only) type, kept as it came.
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<T: BorshSerialize, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
    let bytes = borsh::to_vec(value).map_err(serde::ser::Error::custom)?;
    Serialize::serialize(&bytes, s)
}

pub fn deserialize<'de, T: BorshDeserialize, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
    let bytes = <Vec<u8> as Deserialize>::deserialize(d)?;
    borsh::from_slice(&bytes).map_err(serde::de::Error::custom)
}
