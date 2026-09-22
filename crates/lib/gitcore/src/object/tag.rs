// Annotated tag objects: parse and serialize.

use super::{Kind, Signature, push_header, split_headers};
use crate::error::{Error, Result};
use crate::oid::{Hash, Oid};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub object: Oid,
    pub kind: Kind,
    pub name: Vec<u8>,
    pub tagger: Option<Signature>,
    pub message: Vec<u8>,
}

impl Tag {
    pub fn parse(bytes: &[u8], hash: Hash) -> Result<Tag> {
        let (headers, message) = split_headers(bytes)?;
        let mut object = None;
        let mut kind = None;
        let mut name = None;
        let mut tagger = None;
        for (key, value) in headers {
            match key.as_slice() {
                b"object" => object = Some(Oid::from_hex(hash, &value)?),
                b"type" => kind = Some(Kind::parse(&value)?),
                b"tag" => name = Some(value),
                b"tagger" => tagger = Some(Signature::parse(&value)?),
                _ => return Err(Error::BadTag),
            }
        }
        let (Some(object), Some(kind), Some(name)) = (object, kind, name) else {
            return Err(Error::BadTag);
        };
        Ok(Tag {
            object,
            kind,
            name,
            tagger,
            message: message.to_vec(),
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_header(&mut out, b"object", self.object.to_hex().as_bytes());
        push_header(&mut out, b"type", self.kind.as_str().as_bytes());
        push_header(&mut out, b"tag", &self.name);
        if let Some(tagger) = &self.tagger {
            push_header(&mut out, b"tagger", &tagger.serialize());
        }
        out.push(b'\n');
        out.extend_from_slice(&self.message);
        out
    }
}
