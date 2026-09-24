// Object model tests: hashing, byte-exact parse/serialize of real git objects, tree ordering.

#[path = "common/mod.rs"]
mod common;

use common::{fixture, oid_text, sha1};
use gitcore::{
    Commit, Error, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tag, Tree,
    TreeEntry, oid_of,
};

#[test]
fn blob_oid_matches_git() {
    let id = oid_of(Hash::Sha1, Kind::Blob, b"hello\n").unwrap();
    assert_eq!(id.to_hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
    let empty = oid_of(Hash::Sha1, Kind::Blob, b"").unwrap();
    assert_eq!(empty.to_hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    let sha256 = oid_of(Hash::Sha256, Kind::Blob, b"").unwrap();
    assert_eq!(
        sha256.to_hex(),
        "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813"
    );
}

#[test]
fn frame_is_header_plus_body() {
    let object = Object::new(Kind::Blob, b"abc".to_vec());
    assert_eq!(object.frame(), b"blob 3\0abc");
}

#[test]
fn oid_hex_round_trip_and_ordering() {
    let hex = "0123456789abcdef0123456789abcdef01234567";
    let id = Oid::from_hex(Hash::Sha1, hex).unwrap();
    assert_eq!(id.to_hex(), hex);
    assert_eq!(format!("{id}"), hex);
    assert_eq!(id.hash(), Hash::Sha1);
    assert_eq!(Oid::from_hex(Hash::Sha1, "zz"), Err(Error::BadOidLength));
    assert_eq!(
        Oid::from_hex(Hash::Sha1, "zz23456789abcdef0123456789abcdef01234567"),
        Err(Error::BadHex)
    );
    assert!(Hash::Sha1.zero().is_zero());
    assert!(!id.is_zero());
    assert!(Hash::Sha1.zero() < id);
    assert!(id < Hash::Sha256.zero());
}

#[test]
fn commit_with_gpgsig_round_trips_byte_exact() {
    let bytes = fixture!("commit.bin");
    let expected = oid_text(fixture!("commit.oid"), Hash::Sha1);
    assert_eq!(oid_of(Hash::Sha1, Kind::Commit, bytes).unwrap(), expected);
    let commit = Commit::parse(bytes, Hash::Sha1).unwrap();
    assert_eq!(commit.parents.len(), 2);
    assert_eq!(commit.author.name, b"Ada Lovelace");
    assert_eq!(commit.author.offset_minutes, 540);
    assert_eq!(commit.committer.time, 1700000300);
    assert_eq!(commit.committer.offset_minutes, -300);
    assert_eq!(commit.extra.len(), 2);
    assert_eq!(commit.extra[0].0, b"encoding");
    assert_eq!(commit.extra[1].0, b"gpgsig");
    assert!(
        commit.extra[1]
            .1
            .starts_with(b"-----BEGIN PGP SIGNATURE-----\n\niQEz")
    );
    assert_eq!(commit.message, b"Merge with a signature\n\nBody line.\n");
    assert_eq!(commit.serialize(), bytes);
}

#[test]
fn commit_without_tree_is_rejected() {
    let bytes = b"author A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\n\nmsg\n";
    assert_eq!(Commit::parse(bytes, Hash::Sha1), Err(Error::BadCommit));
}

#[test]
fn signature_formats_like_git() {
    let sig = Signature::parse(b"Name Here <n@h> 1700000000 -0530").unwrap();
    assert_eq!(sig.name, b"Name Here");
    assert_eq!(sig.email, b"n@h");
    assert_eq!(sig.time, 1700000000);
    assert_eq!(sig.offset_minutes, -330);
    assert_eq!(sig.serialize(), b"Name Here <n@h> 1700000000 -0530");
    let empty_name = Signature::parse(b" <n@h> 5 +0000").unwrap();
    assert_eq!(empty_name.name, b"");
    assert_eq!(empty_name.serialize(), b" <n@h> 5 +0000");
    assert_eq!(Signature::parse(b"no brackets"), Err(Error::BadSignature));
    assert_eq!(Signature::parse(b"A <a> x +0000"), Err(Error::BadSignature));
}

#[test]
fn tree_from_git_round_trips_and_orders_directories_with_slash() {
    let bytes = fixture!("mixed-tree.bin");
    let expected = oid_text(fixture!("mixed-tree.oid"), Hash::Sha1);
    assert_eq!(oid_of(Hash::Sha1, Kind::Tree, bytes).unwrap(), expected);
    let tree = Tree::parse(bytes, Hash::Sha1).unwrap();
    let names: Vec<&[u8]> = tree.entries.iter().map(|e| e.name.as_slice()).collect();
    assert_eq!(
        names,
        [&b"a.sh"[..], b"b-dir", b"b.txt", b"b", b"link", b"module"]
    );
    let modes: Vec<Mode> = tree.entries.iter().map(|e| e.mode).collect();
    assert_eq!(
        modes,
        [
            Mode::Executable,
            Mode::Directory,
            Mode::Regular,
            Mode::Directory,
            Mode::Symlink,
            Mode::Gitlink
        ]
    );
    assert_eq!(tree.serialize(), bytes);

    let mut shuffled = tree.clone();
    shuffled.entries.reverse();
    assert_eq!(shuffled.serialize(), bytes);
    let reversed_pair = raw_tree(&[tree.entries[1].clone(), tree.entries[0].clone()]);
    assert_eq!(
        Tree::parse(&reversed_pair, Hash::Sha1),
        Err(Error::UnsortedTree)
    );
    let duplicate = raw_tree(&[tree.entries[0].clone(), tree.entries[0].clone()]);
    assert_eq!(
        Tree::parse(&duplicate, Hash::Sha1),
        Err(Error::UnsortedTree)
    );
}

fn raw_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut raw = Vec::new();
    for entry in entries {
        raw.extend_from_slice(entry.mode.as_bytes());
        raw.push(b' ');
        raw.extend_from_slice(&entry.name);
        raw.push(0);
        raw.extend_from_slice(entry.id.as_bytes());
    }
    raw
}

#[test]
fn plain_tree_fixture_matches_git() {
    let bytes = fixture!("tree.bin");
    let expected = oid_text(fixture!("tree.oid"), Hash::Sha1);
    let tree = Tree::parse(bytes, Hash::Sha1).unwrap();
    assert_eq!(tree.serialize(), bytes);
    assert_eq!(
        oid_of(Hash::Sha1, Kind::Tree, &tree.serialize()).unwrap(),
        expected
    );
    assert!(tree.find(b"hello.txt").is_some());
    assert_eq!(tree.find(b"dir").unwrap().mode, Mode::Directory);
}

#[test]
fn tree_rejects_zero_padded_directory_mode() {
    let entry = TreeEntry {
        mode: Mode::Directory,
        name: b"d".to_vec(),
        id: sha1("31a8084f03ba7f5c5c98b41a4d7b67e2e8b2e82d"),
    };
    let mut raw = b"040000 d\0".to_vec();
    raw.extend_from_slice(entry.id.as_bytes());
    assert_eq!(Tree::parse(&raw, Hash::Sha1), Err(Error::BadMode));
    let mut truncated = b"100644 f\0".to_vec();
    truncated.extend_from_slice(&entry.id.as_bytes()[..10]);
    assert_eq!(Tree::parse(&truncated, Hash::Sha1), Err(Error::Truncated));
}

#[test]
fn tag_round_trips_byte_exact() {
    let bytes = fixture!("tag.bin");
    let expected = oid_text(fixture!("tag.oid"), Hash::Sha1);
    assert_eq!(oid_of(Hash::Sha1, Kind::Tag, bytes).unwrap(), expected);
    let tag = Tag::parse(bytes, Hash::Sha1).unwrap();
    assert_eq!(tag.kind, Kind::Commit);
    assert_eq!(tag.name, b"v1");
    assert_eq!(tag.tagger.as_ref().unwrap().name, b"Charles Babbage");
    assert_eq!(tag.message, b"first release\n");
    assert_eq!(tag.serialize(), bytes);
    let untagged = Tag {
        tagger: None,
        ..tag.clone()
    };
    assert_eq!(
        Tag::parse(&untagged.serialize(), Hash::Sha1).unwrap(),
        untagged
    );
}

#[test]
fn memory_store_hashes_on_put() {
    let mut store = MemoryObjects::new(Hash::Sha256);
    let id = store.put(Kind::Blob, b"").unwrap();
    assert_eq!(id.hash(), Hash::Sha256);
    assert!(store.has(&id).unwrap());
    assert_eq!(store.get(&id).unwrap().unwrap().body, b"");
    assert!(!store.has(&Hash::Sha1.zero()).unwrap());
    assert_eq!(store.count(), 1);
    assert_eq!(store.ids().next(), Some(&id));
}

#[test]
fn kind_tokens() {
    for kind in [Kind::Blob, Kind::Tree, Kind::Commit, Kind::Tag] {
        assert_eq!(Kind::parse(kind.as_str().as_bytes()).unwrap(), kind);
    }
    assert_eq!(Kind::parse(b"bloc"), Err(Error::UnknownObjectKind));
}
