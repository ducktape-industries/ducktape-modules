// Object identity: the hash algorithm, the object id, and git's object hashing.

use crate::git::error::{Error, Result};
use crate::git::object::Kind;
use sha1_checked::Digest as _;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hash {
    Sha1,
    Sha256,
}

impl Hash {
    pub fn size(self) -> usize {
        match self {
            Hash::Sha1 => 20,
            Hash::Sha256 => 32,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Hash::Sha1 => "sha1",
            Hash::Sha256 => "sha256",
        }
    }

    pub fn from_name(name: &[u8]) -> Option<Hash> {
        match name {
            b"sha1" => Some(Hash::Sha1),
            b"sha256" => Some(Hash::Sha256),
            _ => None,
        }
    }

    pub fn zero(self) -> Oid {
        match self {
            Hash::Sha1 => Oid::Sha1([0; 20]),
            Hash::Sha256 => Oid::Sha256([0; 32]),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Oid {
    Sha1([u8; 20]),
    Sha256([u8; 32]),
}

impl Oid {
    pub fn hash(&self) -> Hash {
        match self {
            Oid::Sha1(_) => Hash::Sha1,
            Oid::Sha256(_) => Hash::Sha256,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Oid::Sha1(bytes) => bytes,
            Oid::Sha256(bytes) => bytes,
        }
    }

    pub fn from_bytes(hash: Hash, bytes: &[u8]) -> Result<Oid> {
        let right_length = bytes.len() == hash.size();
        if !right_length {
            return Err(Error::BadOidLength);
        }
        match hash {
            Hash::Sha1 => {
                let mut out = [0; 20];
                out.copy_from_slice(bytes);
                Ok(Oid::Sha1(out))
            }
            Hash::Sha256 => {
                let mut out = [0; 32];
                out.copy_from_slice(bytes);
                Ok(Oid::Sha256(out))
            }
        }
    }

    pub fn from_hex(hash: Hash, hex: impl AsRef<[u8]>) -> Result<Oid> {
        let hex = hex.as_ref();
        let right_length = hex.len() == hash.size() * 2;
        if !right_length {
            return Err(Error::BadOidLength);
        }
        let mut raw = Vec::with_capacity(hash.size());
        for pair in hex.chunks(2) {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            raw.push((high << 4) | low);
        }
        Oid::from_bytes(hash, &raw)
    }

    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(self.as_bytes().len() * 2);
        for byte in self.as_bytes() {
            out.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
            out.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
        }
        out
    }

    pub fn is_zero(&self) -> bool {
        self.as_bytes().iter().all(|byte| *byte == 0)
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::BadHex),
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.hash().name(), self.to_hex())
    }
}

pub(crate) enum Hasher {
    Sha1(Box<sha1_checked::Sha1>),
    Sha256(sha2::Sha256),
}

impl Hasher {
    pub(crate) fn new(hash: Hash) -> Hasher {
        match hash {
            Hash::Sha1 => Hasher::Sha1(Box::new(sha1_checked::Sha1::new())),
            Hash::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
        }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        match self {
            Hasher::Sha1(inner) => inner.update(bytes),
            Hasher::Sha256(inner) => inner.update(bytes),
        }
    }

    pub(crate) fn finish(self) -> Result<Oid> {
        match self {
            Hasher::Sha1(inner) => {
                let result = (*inner).try_finalize();
                if result.has_collision() {
                    return Err(Error::Collision);
                }
                let mut out = [0; 20];
                out.copy_from_slice(result.hash());
                Ok(Oid::Sha1(out))
            }
            Hasher::Sha256(inner) => {
                let mut out = [0; 32];
                out.copy_from_slice(&inner.finalize());
                Ok(Oid::Sha256(out))
            }
        }
    }
}

pub(crate) fn object_header(kind: Kind, len: usize) -> Vec<u8> {
    let mut header = Vec::with_capacity(16);
    header.extend_from_slice(kind.as_str().as_bytes());
    header.push(b' ');
    push_decimal(&mut header, len as u64);
    header.push(0);
    header
}

pub(crate) fn push_decimal(out: &mut Vec<u8>, value: u64) {
    let mut digits = [0u8; 20];
    let mut cursor = digits.len();
    let mut rest = value;
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + (rest % 10) as u8;
        rest /= 10;
        let done = rest == 0;
        if done {
            break;
        }
    }
    out.extend_from_slice(&digits[cursor..]);
}

pub fn oid_of(hash: Hash, kind: Kind, body: &[u8]) -> Result<Oid> {
    let mut hasher = Hasher::new(hash);
    hasher.update(&object_header(kind, body.len()));
    hasher.update(body);
    hasher.finish()
}
