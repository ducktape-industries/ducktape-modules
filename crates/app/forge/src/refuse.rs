// The refusals forge hands back: abi's reason tokens, each with one sentence.

use abi::{Refusal, reason};

pub fn invalid(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::INVALID_INPUT, sentence)
}

pub fn not_found(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::NOT_FOUND, sentence)
}

pub fn unauthorized(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::UNAUTHORIZED, sentence)
}

pub fn already_exists(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::ALREADY_EXISTS, sentence)
}

pub fn wrong_state(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::WRONG_STATE, sentence)
}

pub fn capacity(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::CAPACITY, sentence)
}

pub fn storage(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::PROTOCOL, sentence)
}

/// A different serving node or object replication can satisfy this query.
pub const OBJECT_NOT_HELD: &str = "object_not_held";
pub fn object_not_held(oid: impl std::fmt::Display) -> Refusal {
    Refusal::new(
        OBJECT_NOT_HELD,
        format!("object {oid} is not held by this node"),
    )
}
pub fn stale(sentence: impl Into<String>) -> Refusal {
    Refusal::new(reason::STALE, sentence)
}
