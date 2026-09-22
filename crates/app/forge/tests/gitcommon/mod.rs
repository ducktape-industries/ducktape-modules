// Shared helpers for the integration tests: fixture loading and small object builders.

#![allow(dead_code, unused_macros, unused_imports)]

use forge::git::{
    Commit, Hash, Kind, MemoryObjects, Mode, Objects, Oid, Signature, Tree, TreeEntry,
};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $name
        )) as &[u8]
    };
}
pub(crate) use fixture;

pub fn oid_text(text: &[u8], hash: Hash) -> Oid {
    let trimmed = String::from_utf8_lossy(text);
    Oid::from_hex(hash, trimmed.trim()).expect("fixture oid")
}

pub fn oid_list(text: &[u8], hash: Hash) -> Vec<Oid> {
    String::from_utf8_lossy(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Oid::from_hex(hash, line.trim()).expect("fixture oid"))
        .collect()
}

pub fn sha1(hex: &str) -> Oid {
    Oid::from_hex(Hash::Sha1, hex).expect("hex")
}

pub fn signature(time: i64) -> Signature {
    Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time,
        offset_minutes: 0,
    }
}

pub fn blob(store: &mut MemoryObjects, content: &[u8]) -> Oid {
    store.put(Kind::Blob, content).expect("put blob")
}

pub fn tree(store: &mut MemoryObjects, entries: &[(&str, Mode, Oid)]) -> Oid {
    let tree = Tree {
        entries: entries
            .iter()
            .map(|(name, mode, id)| TreeEntry {
                mode: *mode,
                name: name.as_bytes().to_vec(),
                id: *id,
            })
            .collect(),
    };
    store.put(Kind::Tree, &tree.serialize()).expect("put tree")
}

pub fn commit(
    store: &mut MemoryObjects,
    tree: Oid,
    parents: &[Oid],
    time: i64,
    message: &str,
) -> Oid {
    let commit = Commit {
        tree,
        parents: parents.to_vec(),
        author: signature(time),
        committer: signature(time),
        extra: Vec::new(),
        message: message.as_bytes().to_vec(),
    };
    store
        .put(Kind::Commit, &commit.serialize())
        .expect("put commit")
}

pub fn file_tree(store: &mut MemoryObjects, files: &[(&str, &str)]) -> Oid {
    let entries: Vec<(&str, Mode, Oid)> = files
        .iter()
        .map(|(name, content)| (*name, Mode::Regular, blob(store, content.as_bytes())))
        .collect();
    tree(store, &entries)
}
