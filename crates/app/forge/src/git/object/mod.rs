// Git object kinds, the raw object container, and the typed parsers for tree, commit and tag.

mod commit;
mod tag;
mod tree;

pub use commit::{Commit, Signature};
pub use tag::Tag;
pub use tree::{Mode, Tree, TreeEntry};

use crate::git::error::{Error, Result};
use crate::git::oid::{object_header, Hash, Oid};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Blob,
    Tree,
    Commit,
    Tag,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Blob => "blob",
            Kind::Tree => "tree",
            Kind::Commit => "commit",
            Kind::Tag => "tag",
        }
    }

    pub fn parse(token: &[u8]) -> Result<Kind> {
        match token {
            b"blob" => Ok(Kind::Blob),
            b"tree" => Ok(Kind::Tree),
            b"commit" => Ok(Kind::Commit),
            b"tag" => Ok(Kind::Tag),
            _ => Err(Error::UnknownObjectKind),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub kind: Kind,
    pub body: Vec<u8>,
}

impl Object {
    pub fn new(kind: Kind, body: Vec<u8>) -> Object {
        Object { kind, body }
    }

    pub fn frame(&self) -> Vec<u8> {
        let mut out = object_header(self.kind, self.body.len());
        out.extend_from_slice(&self.body);
        out
    }

    pub fn id(&self, hash: Hash) -> Result<Oid> {
        crate::git::oid::oid_of(hash, self.kind, &self.body)
    }
}

pub(crate) type Header = (Vec<u8>, Vec<u8>);

pub(crate) fn split_headers(bytes: &[u8]) -> Result<(Vec<Header>, &[u8])> {
    let mut headers: Vec<Header> = Vec::new();
    let mut rest = bytes;
    loop {
        let Some(newline) = rest.iter().position(|byte| *byte == b'\n') else {
            let unterminated = !rest.is_empty();
            if unterminated {
                return Err(Error::BadHeader);
            }
            return Ok((headers, rest));
        };
        let line = &rest[..newline];
        rest = &rest[newline + 1..];
        if line.is_empty() {
            return Ok((headers, rest));
        }
        let continuation = line[0] == b' ';
        if continuation {
            let Some(last) = headers.last_mut() else {
                return Err(Error::BadHeader);
            };
            last.1.push(b'\n');
            last.1.extend_from_slice(&line[1..]);
            continue;
        }
        let Some(space) = line.iter().position(|byte| *byte == b' ') else {
            return Err(Error::BadHeader);
        };
        headers.push((line[..space].to_vec(), line[space + 1..].to_vec()));
    }
}

pub(crate) fn push_header(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    out.extend_from_slice(key);
    out.push(b' ');
    for byte in value {
        out.push(*byte);
        let folded = *byte == b'\n';
        if folded {
            out.push(b' ');
        }
    }
    out.push(b'\n');
}
