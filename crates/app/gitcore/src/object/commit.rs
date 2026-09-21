// Commit objects and the author/committer signature line.

use super::{push_header, split_headers};
use crate::error::{Error, Result};
use crate::oid::{push_decimal, Hash, Oid};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub name: Vec<u8>,
    pub email: Vec<u8>,
    pub time: i64,
    pub offset_minutes: i16,
}

impl Signature {
    pub fn parse(bytes: &[u8]) -> Result<Signature> {
        let Some(open) = bytes.iter().position(|byte| *byte == b'<') else {
            return Err(Error::BadSignature);
        };
        let name = trim_spaces(&bytes[..open]).to_vec();
        let after_open = &bytes[open + 1..];
        let Some(close) = after_open.iter().position(|byte| *byte == b'>') else {
            return Err(Error::BadSignature);
        };
        let email = after_open[..close].to_vec();
        let rest = trim_spaces(&after_open[close + 1..]);
        let Some(space) = rest.iter().position(|byte| *byte == b' ') else {
            return Err(Error::BadSignature);
        };
        let time = parse_i64(&rest[..space])?;
        let offset_minutes = parse_offset(trim_spaces(&rest[space + 1..]))?;
        Ok(Signature {
            name,
            email,
            time,
            offset_minutes,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.name);
        out.extend_from_slice(b" <");
        out.extend_from_slice(&self.email);
        out.extend_from_slice(b"> ");
        push_i64(&mut out, self.time);
        out.push(b' ');
        let sign = if self.offset_minutes < 0 { b'-' } else { b'+' };
        out.push(sign);
        let magnitude = self.offset_minutes.unsigned_abs();
        push_two_digits(&mut out, magnitude / 60);
        push_two_digits(&mut out, magnitude % 60);
        out
    }
}

fn trim_spaces(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| *byte != b' ')
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |i| i + 1);
    let empty = start >= end;
    if empty {
        return &[];
    }
    &bytes[start..end]
}

fn parse_i64(digits: &[u8]) -> Result<i64> {
    let (negative, digits) = match digits.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, digits),
    };
    let mut value: i64 = 0;
    if digits.is_empty() {
        return Err(Error::BadSignature);
    }
    for digit in digits {
        let is_digit = digit.is_ascii_digit();
        if !is_digit {
            return Err(Error::BadSignature);
        }
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(i64::from(digit - b'0')))
            .ok_or(Error::BadSignature)?;
    }
    Ok(if negative { -value } else { value })
}

fn parse_offset(text: &[u8]) -> Result<i16> {
    let well_formed = text.len() == 5 && text[1..].iter().all(u8::is_ascii_digit);
    if !well_formed {
        return Err(Error::BadSignature);
    }
    let sign = match text[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return Err(Error::BadSignature),
    };
    let hours = i16::from(text[1] - b'0') * 10 + i16::from(text[2] - b'0');
    let minutes = i16::from(text[3] - b'0') * 10 + i16::from(text[4] - b'0');
    Ok(sign * (hours * 60 + minutes))
}

fn push_i64(out: &mut Vec<u8>, value: i64) {
    if value < 0 {
        out.push(b'-');
    }
    push_decimal(out, value.unsigned_abs());
}

fn push_two_digits(out: &mut Vec<u8>, value: u16) {
    out.push(b'0' + (value / 10 % 10) as u8);
    out.push(b'0' + (value % 10) as u8);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    pub author: Signature,
    pub committer: Signature,
    pub extra: Vec<(Vec<u8>, Vec<u8>)>,
    pub message: Vec<u8>,
}

impl Commit {
    pub fn parse(bytes: &[u8], hash: Hash) -> Result<Commit> {
        let (headers, message) = split_headers(bytes)?;
        let mut tree = None;
        let mut parents = Vec::new();
        let mut author = None;
        let mut committer = None;
        let mut extra = Vec::new();
        for (key, value) in headers {
            match key.as_slice() {
                b"tree" => {
                    let duplicate = tree.is_some();
                    if duplicate {
                        return Err(Error::BadCommit);
                    }
                    tree = Some(Oid::from_hex(hash, &value)?);
                }
                b"parent" => parents.push(Oid::from_hex(hash, &value)?),
                b"author" => author = Some(Signature::parse(&value)?),
                b"committer" => committer = Some(Signature::parse(&value)?),
                _ => extra.push((key, value)),
            }
        }
        let (Some(tree), Some(author), Some(committer)) = (tree, author, committer) else {
            return Err(Error::BadCommit);
        };
        Ok(Commit {
            tree,
            parents,
            author,
            committer,
            extra,
            message: message.to_vec(),
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_header(&mut out, b"tree", self.tree.to_hex().as_bytes());
        for parent in &self.parents {
            push_header(&mut out, b"parent", parent.to_hex().as_bytes());
        }
        push_header(&mut out, b"author", &self.author.serialize());
        push_header(&mut out, b"committer", &self.committer.serialize());
        for (key, value) in &self.extra {
            push_header(&mut out, key, value);
        }
        out.push(b'\n');
        out.extend_from_slice(&self.message);
        out
    }
}
