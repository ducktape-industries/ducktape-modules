//! The refusals a program hands back: abi's reason tokens, each with one sentence.

use abi::{Refusal, reason};
use borsh::BorshDeserialize;

pub fn invalid(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::INVALID_INPUT, sentence)
}

pub fn not_found(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::NOT_FOUND, sentence)
}

pub fn already_exists(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::ALREADY_EXISTS, sentence)
}

pub fn wrong_state(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::WRONG_STATE, sentence)
}

pub fn unauthorized(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::UNAUTHORIZED, sentence)
}

pub fn capacity(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::CAPACITY, sentence)
}

pub fn stale(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::STALE, sentence)
}

/// Stored state that does not decode: an operator's problem, never a panic.
pub fn corrupt(table: &str, key: &[u8], what: impl std::fmt::Display) -> Refusal {
    Refusal::new(
        reason::CORRUPT,
        format!("{table}[{}]: {what}", abi::hex(key)),
    )
}

/// A program's `Op`/`Query` that does not decode, refused as invalid input
/// naming the program and the shape.
pub fn decoded<T: BorshDeserialize>(program: &str, shape: &str, bytes: &[u8]) -> Result<T, Refusal> {
    abi::decode(bytes)
        .map_err(|fault| invalid(format!("{program}: {shape} did not decode: {}", fault.sentence)))
}
