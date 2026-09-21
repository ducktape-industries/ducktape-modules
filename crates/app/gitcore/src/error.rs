// The single error type every gitcore function returns.

use crate::object::Kind;
use crate::oid::{Hash, Oid};
use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadHeader,
    UnknownObjectKind,
    BadHex,
    BadOidLength,
    HashMismatch { expected: Oid, actual: Oid },
    WrongHash { expected: Hash, actual: Hash },
    Collision,
    MissingBase(Oid),
    MissingObject(Oid),
    WrongKind { id: Oid, expected: Kind },
    Cycle,
    BadPktLine,
    BadPackHeader,
    UnsupportedPackVersion(u32),
    BadDelta,
    Inflate,
    BadChecksum,
    UnsortedTree,
    BadMode,
    BadSignature,
    BadCommit,
    BadTag,
    CapReached,
    Unsupported,
    TooManyObjects,
    ObjectTooLarge,
    DeltaTooDeep,
    BadRequest,
    UnknownCommand,
    NotAdvertised(Oid),
    Storage,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => f.write_str("truncated input"),
            Error::BadHeader => f.write_str("bad object header"),
            Error::UnknownObjectKind => f.write_str("unknown object kind"),
            Error::BadHex => f.write_str("bad hex"),
            Error::BadOidLength => f.write_str("bad object id length"),
            Error::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch: expected {expected}, got {actual}")
            }
            Error::WrongHash { expected, actual } => write!(
                f,
                "wrong hash algorithm: expected {}, got {}",
                expected.name(),
                actual.name()
            ),
            Error::Collision => f.write_str("sha1 collision detected"),
            Error::MissingBase(id) => write!(f, "missing delta base {id}"),
            Error::MissingObject(id) => write!(f, "missing object {id}"),
            Error::WrongKind { id, expected } => write!(f, "{id} is not a {}", expected.as_str()),
            Error::Cycle => f.write_str("cycle"),
            Error::BadPktLine => f.write_str("bad pkt-line"),
            Error::BadPackHeader => f.write_str("bad pack header"),
            Error::UnsupportedPackVersion(v) => write!(f, "unsupported pack version {v}"),
            Error::BadDelta => f.write_str("bad delta"),
            Error::Inflate => f.write_str("inflate failed"),
            Error::BadChecksum => f.write_str("bad pack checksum"),
            Error::UnsortedTree => f.write_str("unsorted tree"),
            Error::BadMode => f.write_str("bad tree entry mode"),
            Error::BadSignature => f.write_str("bad signature line"),
            Error::BadCommit => f.write_str("bad commit"),
            Error::BadTag => f.write_str("bad tag"),
            Error::CapReached => f.write_str("walk cap reached"),
            Error::Unsupported => f.write_str("unsupported"),
            Error::TooManyObjects => f.write_str("too many objects"),
            Error::ObjectTooLarge => f.write_str("object too large"),
            Error::DeltaTooDeep => f.write_str("delta chain too deep"),
            Error::BadRequest => f.write_str("bad request"),
            Error::UnknownCommand => f.write_str("unknown command"),
            Error::NotAdvertised(id) => write!(f, "not our ref {id}"),
            Error::Storage => f.write_str("storage failure"),
        }
    }
}
