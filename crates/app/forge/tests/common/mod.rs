#![allow(dead_code, unused_imports)]
// The forge program end to end over MemorySandbox: founding, access, pushes in steps, advertisement, fetch, merge, sha256.

pub use std::collections::{BTreeMap, BTreeSet};

pub use abi::{Cause, Env, HashKind, Origin, reason};
pub use forge::{Bounds, Op, Page, Query, Reply, Service, Settings};
pub use gitcore::wire::pktline::{self, Pkt, Reader};
pub use gitcore::{
    Commit, Hash, Kind, Limits, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree,
    TreeEntry, pack,
};
pub use store::{Memory, Reads, Writes};

pub mod sandbox;
pub mod story;
pub use sandbox::MemorySandbox;

pub const OWNER: &[u8] = b"owner-key";
pub const WRITER: &[u8] = b"writer-key";
pub const STRANGER: &[u8] = b"stranger-key";
pub const TIME: u64 = 1_700_000_000;

pub fn env(actor: &[u8]) -> Env {
    Env {
        network: b"net".to_vec(),
        height: 1,
        time: TIME,
        me: "forge".into(),
        origin: Origin::External(actor.to_vec()),
        cause: Cause::Direct,
    }
}

pub fn bounds() -> Bounds {
    Bounds {
        max_objects: 10_000,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
        page_size: 128,
        log_walk: 1000,
        tree_walk: 256,
        diff_bytes: 4 << 20,
        blob_bytes: 1 << 20,
        record_bytes: 64 << 10,
    }
}

pub fn founded() -> MemorySandbox {
    let mut sandbox = MemorySandbox::default();
    forge::init(&mut sandbox, &abi::encode(&bounds())).unwrap();
    sandbox
}

pub fn act(sandbox: &mut MemorySandbox, actor: &[u8], op: &Op) -> Result<Vec<u8>, abi::Refusal> {
    forge::execute(sandbox, &env(actor), &abi::encode(op))?;
    Ok(sandbox.forge.take_output())
}

pub fn ask(sandbox: &MemorySandbox, query: &Query) -> Result<Vec<u8>, abi::Refusal> {
    forge::query(sandbox, &env(OWNER), &abi::encode(query))
}

pub fn create(sandbox: &mut MemorySandbox, name: &str, hash: HashKind) {
    act(
        sandbox,
        OWNER,
        &Op::Create {
            repo: name.into(),
            hash,
        },
    )
    .unwrap();
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
    store.put(Kind::Blob, content).unwrap()
}

pub fn tree(store: &mut MemoryObjects, entries: &[(&str, Oid)]) -> Oid {
    let tree = Tree {
        entries: entries
            .iter()
            .map(|(name, id)| TreeEntry {
                mode: Mode::Regular,
                name: name.as_bytes().to_vec(),
                id: *id,
            })
            .collect(),
    };
    store.put(Kind::Tree, &tree.serialize()).unwrap()
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
        message: format!("{message}\n").into_bytes(),
    };
    store.put(Kind::Commit, &commit.serialize()).unwrap()
}

pub fn file_commit(
    store: &mut MemoryObjects,
    parents: &[Oid],
    time: i64,
    files: &[(&str, &[u8])],
) -> Oid {
    let mut entries = Vec::new();
    for (name, content) in files {
        let id = blob(store, content);
        entries.push((*name, id));
    }
    let tree = tree(store, &entries);
    commit(store, tree, parents, time, "step")
}

pub fn pack_of(store: &MemoryObjects, ids: &[Oid]) -> Vec<u8> {
    let objects: Vec<Object> = ids
        .iter()
        .map(|id| store.get(id).unwrap().unwrap())
        .collect();
    pack::write(objects, store.hash()).unwrap()
}

pub fn all_ids(store: &MemoryObjects) -> Vec<Oid> {
    store.ids().copied().collect()
}

pub fn push_request(commands: &[(Oid, Oid, &str)], pack_bytes: &[u8]) -> Vec<u8> {
    let mut request = Vec::new();
    for (index, (old, new, name)) in commands.iter().enumerate() {
        let mut line = format!("{old} {new} {name}").into_bytes();
        let first = index == 0;
        if first {
            line.extend_from_slice(b"\0report-status side-band-64k");
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    request.extend_from_slice(pack_bytes);
    request
}

pub fn push(
    sandbox: &mut MemorySandbox,
    actor: &[u8],
    repo: &str,
    commands: &[(Oid, Oid, &str)],
    pack_bytes: &[u8],
) -> Result<Vec<String>, abi::Refusal> {
    let report = act(
        sandbox,
        actor,
        &Op::Push {
            repo: repo.into(),
            request: push_request(commands, pack_bytes),
        },
    )?;
    Ok(report_lines(&report))
}

pub fn report_lines(report: &[u8]) -> Vec<String> {
    let mut reader = Reader::new(report);
    let Some(Ok(Pkt::Data(band))) = reader.next() else {
        panic!("a sideband report starts with a data packet");
    };
    assert_eq!(band[0], 1);
    Reader::new(&band[1..])
        .filter_map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => {
                Some(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            _ => None,
        })
        .collect()
}

pub fn refs_of(sandbox: &MemorySandbox, repo: &str) -> BTreeMap<String, String> {
    let reply: Reply = abi::decode(
        &ask(
            sandbox,
            &Query::Refs {
                repo: repo.into(),
                page: Page::first(128),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let Reply::Refs { page, .. } = reply else {
        panic!("refs reply");
    };
    page.items
        .into_iter()
        .map(|r| (String::from_utf8(r.name).unwrap(), r.target))
        .collect()
}

pub fn v2_request(command: &str, hash: Hash, args: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, format!("command={command}").as_bytes());
    pktline::push_line(&mut out, b"agent=git/2.55.0");
    pktline::push_line(
        &mut out,
        format!("object-format={}", hash.name()).as_bytes(),
    );
    out.extend_from_slice(pktline::delim());
    for arg in args {
        pktline::push_line(&mut out, arg.as_bytes());
    }
    out.extend_from_slice(pktline::flush());
    out
}

pub fn pkt_lines(bytes: &[u8]) -> Vec<String> {
    Reader::new(bytes)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            Pkt::Flush => "<flush>".into(),
            Pkt::Delim => "<delim>".into(),
            Pkt::ResponseEnd => "<end>".into(),
        })
        .collect()
}

pub fn fetched_pack(response: &[u8]) -> (Vec<String>, Vec<u8>) {
    let mut sections = Vec::new();
    let mut pack_bytes = Vec::new();
    for pkt in Reader::new(response) {
        match pkt.unwrap() {
            Pkt::Data(data) if sections.last().is_some_and(|s| s == "packfile") => {
                assert_eq!(data[0], 1);
                pack_bytes.extend_from_slice(&data[1..]);
            }
            Pkt::Data(data) => {
                sections.push(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            Pkt::Flush => sections.push("<flush>".into()),
            Pkt::Delim => sections.push("<delim>".into()),
            Pkt::ResponseEnd => sections.push("<end>".into()),
        }
    }
    (sections, pack_bytes)
}

pub fn ids_in_pack(bytes: &[u8], hash: Hash) -> BTreeSet<Oid> {
    pack::read(bytes, hash, &Limits::generous(), |_| Ok(None))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}
