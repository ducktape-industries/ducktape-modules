//! The errors a module hands back: a [`code`](crate::code) each, with one
//! sentence.

use borsh::BorshDeserialize;

use crate::{Error, code};

pub fn invalid(message: impl Into<String>) -> Error {
    Error::new(code::INVALID_INPUT, message)
}

pub fn not_found(message: impl Into<String>) -> Error {
    Error::new(code::NOT_FOUND, message)
}

pub fn already_exists(message: impl Into<String>) -> Error {
    Error::new(code::ALREADY_EXISTS, message)
}

pub fn wrong_state(message: impl Into<String>) -> Error {
    Error::new(code::WRONG_STATE, message)
}

pub fn unauthorized(message: impl Into<String>) -> Error {
    Error::new(code::UNAUTHORIZED, message)
}

pub fn capacity(message: impl Into<String>) -> Error {
    Error::new(code::CAPACITY, message)
}

pub fn stale(message: impl Into<String>) -> Error {
    Error::new(code::STALE, message)
}

/// Stored state that does not decode: an operator's problem, never a panic.
pub fn corrupt(table: &str, key: &[u8], what: impl std::fmt::Display) -> Error {
    Error::new(code::CORRUPT, format!("{table}[{}]: {what}", abi::hex(key)))
}

/// A module's `Op`/`Query` that does not decode, refused as invalid input
/// naming the module and the shape.
pub fn decoded<T: BorshDeserialize>(module: &str, shape: &str, bytes: &[u8]) -> Result<T, Error> {
    abi::decode(bytes).map_err(|fault| {
        invalid(format!(
            "{module}: {shape} did not decode: {}",
            fault.sentence
        ))
    })
}
