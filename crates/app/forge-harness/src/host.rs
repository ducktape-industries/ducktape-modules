// MemoryHost: the ducktape host ops answered over in-memory maps, framing blobs the way the real host does. Used by Harness.

use std::collections::BTreeMap;

use abi::{
    Blob, BlobHeader, BlobId, Cause, Entry, Env, HashKind, HostOp, HostReply, Origin, Refusal,
    Scan, reason,
};
use sha1::Digest as _;

const NETWORK: &[u8] = b"harness";
const PROGRAM: &str = "forge";
const TIME: u64 = 1_700_000_000;

pub struct MemoryHost {
    actor: Vec<u8>,
    height: u64,
    state: BTreeMap<Vec<u8>, Vec<u8>>,
    blobs: BTreeMap<BlobId, Blob>,
    output: Vec<u8>,
    response: Vec<u8>,
}

impl MemoryHost {
    pub fn new(actor: Vec<u8>) -> MemoryHost {
        MemoryHost {
            actor,
            height: 0,
            state: BTreeMap::new(),
            blobs: BTreeMap::new(),
            output: Vec::new(),
            response: Vec::new(),
        }
    }

    pub fn advance_height(&mut self) {
        self.height += 1;
    }

    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub fn take_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.response)
    }

    pub fn env(&self) -> Env {
        Env {
            network: NETWORK.to_vec(),
            height: self.height,
            time: TIME,
            me: PROGRAM.into(),
            origin: Origin::External(self.actor.clone()),
            cause: Cause::Direct,
        }
    }

    fn scan(&self, scan: &Scan) -> Vec<Entry> {
        let admitted = self
            .state
            .iter()
            .filter(|(key, _)| scan.admits(key))
            .map(|(key, value)| Entry {
                key: key.clone(),
                value: value.clone(),
            });
        let ordered: Vec<Entry> = if scan.reverse {
            admitted.rev().collect()
        } else {
            admitted.collect()
        };
        match scan.limit {
            Some(limit) => ordered.into_iter().take(limit as usize).collect(),
            None => ordered,
        }
    }

    fn blob_put(&mut self, hash: HashKind, kind: String, body: Vec<u8>) -> HostReply {
        let framed = match frame(&kind, &body) {
            Ok(framed) => framed,
            Err(refusal) => return HostReply::Refused(refusal),
        };
        let id = id_of(hash, &framed);
        self.blobs.entry(id).or_insert(Blob { kind, body });
        HostReply::BlobId(id)
    }

    fn blob_read(&self, id: BlobId, offset: u64, len: u64) -> Option<Vec<u8>> {
        let blob = self.blobs.get(&id)?;
        let total = blob.body.len() as u64;
        let start = offset.min(total);
        let end = offset.saturating_add(len).min(total);
        Some(blob.body[start as usize..end as usize].to_vec())
    }
}

#[async_trait::async_trait]
impl runtime::Host for MemoryHost {
    async fn call(&mut self, op: HostOp) -> HostReply {
        match op {
            HostOp::Get(key) | HostOp::CommittedGet(key) => {
                HostReply::Value(self.state.get(&key).cloned())
            }
            HostOp::Set { key, value } => {
                self.state.insert(key, value);
                HostReply::Done
            }
            HostOp::Delete(key) => {
                self.state.remove(&key);
                HostReply::Done
            }
            HostOp::Scan(scan) | HostOp::CommittedScan(scan) => {
                HostReply::Entries(self.scan(&scan))
            }
            HostOp::BlobPut { hash, kind, body } => self.blob_put(hash, kind, body),
            HostOp::BlobGet(id) => HostReply::Blob(self.blobs.get(&id).cloned()),
            HostOp::BlobStat(id) => {
                HostReply::BlobHeader(self.blobs.get(&id).map(|blob| BlobHeader {
                    kind: blob.kind.clone(),
                    len: blob.body.len() as u64,
                }))
            }
            HostOp::BlobRead { id, offset, len } => {
                HostReply::Value(self.blob_read(id, offset, len))
            }
            HostOp::Output(bytes) => {
                self.output = bytes;
                HostReply::Done
            }
            HostOp::Respond(bytes) => {
                self.response.extend_from_slice(&bytes);
                HostReply::Done
            }
            HostOp::Root(_)
            | HostOp::Query { .. }
            | HostOp::Emit(_)
            | HostOp::Event(_)
            | HostOp::Crypto(_) => HostReply::Refused(Refusal::new(
                reason::UNSUPPORTED,
                "the harness host answers state, blobs, output and response only",
            )),
        }
    }
}

fn frame(kind: &str, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let kind_is_a_word = !kind.is_empty() && !kind.contains([' ', '\0']);
    if !kind_is_a_word {
        return Err(Refusal::new(
            reason::INVALID_INPUT,
            "a blob kind is one non-empty word without spaces or NUL",
        ));
    }
    let header = format!("{kind} {}\0", body.len());
    let mut framed = Vec::with_capacity(header.len() + body.len());
    framed.extend_from_slice(header.as_bytes());
    framed.extend_from_slice(body);
    Ok(framed)
}

fn id_of(hash: HashKind, framed: &[u8]) -> BlobId {
    match hash {
        HashKind::Sha256 => BlobId::Sha256(sha2::Sha256::digest(framed).into()),
        HashKind::Sha1 => BlobId::Sha1(sha1::Sha1::digest(framed).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::Host as _;

    #[tokio::test]
    async fn a_git_blob_gets_its_git_oid() {
        let mut host = MemoryHost::new(b"me".to_vec());
        let reply = host
            .call(HostOp::BlobPut {
                hash: HashKind::Sha1,
                kind: "blob".into(),
                body: b"hello\n".to_vec(),
            })
            .await;
        let HostReply::BlobId(id) = reply else {
            panic!("{reply:?}");
        };
        assert_eq!(
            abi::hex(id.digest()),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        let stat = host.call(HostOp::BlobStat(id)).await;
        assert_eq!(
            stat,
            HostReply::BlobHeader(Some(BlobHeader {
                kind: "blob".into(),
                len: 6
            }))
        );
        let window = host
            .call(HostOp::BlobRead {
                id,
                offset: 2,
                len: 100,
            })
            .await;
        assert_eq!(window, HostReply::Value(Some(b"llo\n".to_vec())));
    }

    #[tokio::test]
    async fn scan_honours_bounds_direction_and_limit() {
        let mut host = MemoryHost::new(b"me".to_vec());
        for key in ["a/1", "a/2", "a/3", "b/1"] {
            host.call(HostOp::Set {
                key: key.into(),
                value: Vec::new(),
            })
            .await;
        }
        let reply = host
            .call(HostOp::Scan(Scan::prefix(b"a/").reverse().limit(2)))
            .await;
        let HostReply::Entries(entries) = reply else {
            panic!("{reply:?}");
        };
        let keys: Vec<&[u8]> = entries.iter().map(|entry| entry.key.as_slice()).collect();
        assert_eq!(keys, [b"a/3", b"a/2"]);
    }

    #[tokio::test]
    async fn height_moves_with_the_harness_and_the_actor_signs() {
        let mut host = MemoryHost::new(b"me".to_vec());
        host.advance_height();
        let env = host.env();
        assert_eq!(env.height, 1);
        assert_eq!(env.origin, Origin::External(b"me".to_vec()));
        let refused = host.call(HostOp::Event(Vec::new())).await;
        assert!(matches!(refused, HostReply::Refused(_)));
    }
}
