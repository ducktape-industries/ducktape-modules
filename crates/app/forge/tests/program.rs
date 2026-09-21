// The forge program end to end over MemorySandbox: founding, access, pushes in steps, advertisement, fetch, merge, sha256.

use std::collections::{BTreeMap, BTreeSet};

use abi::{Cause, Env, HashKind, Origin, reason};
use forge::{Bounds, MemorySandbox, Op, Query, Reply, Sandbox, Service, Settings};
use gitcore::wire::pktline::{self, Pkt, Reader};
use gitcore::{
    Commit, Hash, Kind, Limits, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree,
    TreeEntry, pack,
};

const OWNER: &[u8] = b"owner-key";
const WRITER: &[u8] = b"writer-key";
const STRANGER: &[u8] = b"stranger-key";
const TIME: u64 = 1_700_000_000;

fn env(actor: &[u8]) -> Env {
    Env {
        network: b"net".to_vec(),
        height: 1,
        time: TIME,
        me: "forge".into(),
        origin: Origin::External(actor.to_vec()),
        cause: Cause::Direct,
    }
}

fn bounds() -> Bounds {
    Bounds {
        max_objects: 10_000,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
    }
}

fn founded() -> MemorySandbox {
    let sandbox = MemorySandbox::default();
    forge::init(&sandbox, &abi::encode(&bounds())).unwrap();
    sandbox
}

fn act(sandbox: &MemorySandbox, actor: &[u8], op: &Op) -> Result<Vec<u8>, abi::Refusal> {
    forge::execute(sandbox, &env(actor), &abi::encode(op))?;
    Ok(sandbox.take_output())
}

fn ask(sandbox: &MemorySandbox, query: &Query) -> Result<Vec<u8>, abi::Refusal> {
    forge::query(sandbox, &abi::encode(query))?;
    Ok(sandbox.take_response())
}

fn create(sandbox: &MemorySandbox, name: &str, hash: HashKind) {
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

fn signature(time: i64) -> Signature {
    Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time,
        offset_minutes: 0,
    }
}

fn blob(store: &mut MemoryObjects, content: &[u8]) -> Oid {
    store.put(Kind::Blob, content).unwrap()
}

fn tree(store: &mut MemoryObjects, entries: &[(&str, Oid)]) -> Oid {
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

fn commit(store: &mut MemoryObjects, tree: Oid, parents: &[Oid], time: i64, message: &str) -> Oid {
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

fn file_commit(
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

fn pack_of(store: &MemoryObjects, ids: &[Oid]) -> Vec<u8> {
    let objects: Vec<Object> = ids
        .iter()
        .map(|id| store.get(id).unwrap().unwrap())
        .collect();
    pack::write(objects, store.hash()).unwrap()
}

fn all_ids(store: &MemoryObjects) -> Vec<Oid> {
    store.ids().copied().collect()
}

fn push_request(commands: &[(Oid, Oid, &str)], pack_bytes: &[u8]) -> Vec<u8> {
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

fn push(
    sandbox: &MemorySandbox,
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

fn report_lines(report: &[u8]) -> Vec<String> {
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

fn refs_of(sandbox: &MemorySandbox, repo: &str) -> BTreeMap<String, String> {
    let reply: Reply =
        abi::decode(&ask(sandbox, &Query::Refs { repo: repo.into() }).unwrap()).unwrap();
    let Reply::Refs(refs) = reply else {
        panic!("refs reply");
    };
    refs.into_iter()
        .map(|r| (String::from_utf8(r.name).unwrap(), r.target))
        .collect()
}

fn v2_request(command: &str, hash: Hash, args: &[String]) -> Vec<u8> {
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

fn pkt_lines(bytes: &[u8]) -> Vec<String> {
    Reader::new(bytes)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            Pkt::Flush => "<flush>".into(),
            Pkt::Delim => "<delim>".into(),
            Pkt::ResponseEnd => "<end>".into(),
        })
        .collect()
}

fn fetched_pack(response: &[u8]) -> (Vec<String>, Vec<u8>) {
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

fn ids_in_pack(bytes: &[u8], hash: Hash) -> BTreeSet<Oid> {
    pack::read(bytes, hash, &Limits::generous(), |_| Ok(None))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn founding_requires_bounds_and_ops_require_a_signer() {
    let sandbox = MemorySandbox::default();
    let unfounded = forge::init(&sandbox, b"");
    assert_eq!(unfounded.unwrap_err().reason, reason::PROTOCOL);
    forge::init(&sandbox, &abi::encode(&bounds())).unwrap();

    let by_system = forge::execute(
        &sandbox,
        &Env {
            origin: Origin::System,
            ..env(OWNER)
        },
        &abi::encode(&Op::Create {
            repo: "r".into(),
            hash: HashKind::Sha1,
        }),
    );
    assert_eq!(by_system.unwrap_err().reason, reason::UNAUTHORIZED);
}

#[test]
fn create_names_an_owner_and_refuses_bad_or_taken_names() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let again = act(
        &sandbox,
        WRITER,
        &Op::Create {
            repo: "project".into(),
            hash: HashKind::Sha1,
        },
    );
    assert_eq!(again.unwrap_err().reason, reason::ALREADY_EXISTS);
    for bad in ["", "a/b", ".hidden", "x.git", "sp ace"] {
        let refused = act(
            &sandbox,
            OWNER,
            &Op::Create {
                repo: bad.into(),
                hash: HashKind::Sha1,
            },
        );
        assert_eq!(
            refused.unwrap_err().reason,
            reason::INVALID_INPUT,
            "{bad:?}"
        );
    }
    let listed: Reply = abi::decode(&ask(&sandbox, &Query::Repos).unwrap()).unwrap();
    let Reply::Repos(repos) = listed else {
        panic!();
    };
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "project");
    assert_eq!(repos[0].repo.owner, OWNER);
    assert_eq!(repos[0].repo.settings, Settings::default());
}

#[test]
fn a_push_stores_the_objects_moves_the_ref_and_reports() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("README", b"hello\n")]);
    let zero = Hash::Sha1.zero();

    let report = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, tip, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    assert_eq!(report, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(sandbox.blob_count(), 3);
    assert_eq!(
        refs_of(&sandbox, "project"),
        BTreeMap::from([("refs/heads/main".to_string(), tip.to_hex())])
    );
}

#[test]
fn only_the_owner_and_granted_writers_push() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let zero = Hash::Sha1.zero();
    let pack_bytes = pack_of(&source, &all_ids(&source));
    let commands = [(zero, tip, "refs/heads/main")];

    let stranger = push(&sandbox, STRANGER, "project", &commands, &pack_bytes);
    assert_eq!(stranger.unwrap_err().reason, reason::UNAUTHORIZED);
    assert_eq!(sandbox.blob_count(), 0);

    let grant_by_stranger = act(
        &sandbox,
        STRANGER,
        &Op::Grant {
            repo: "project".into(),
            key: WRITER.to_vec(),
        },
    );
    assert_eq!(grant_by_stranger.unwrap_err().reason, reason::UNAUTHORIZED);

    act(
        &sandbox,
        OWNER,
        &Op::Grant {
            repo: "project".into(),
            key: WRITER.to_vec(),
        },
    )
    .unwrap();
    let writer = push(&sandbox, WRITER, "project", &commands, &pack_bytes).unwrap();
    assert_eq!(writer, ["unpack ok", "ok refs/heads/main"]);

    act(
        &sandbox,
        OWNER,
        &Op::Revoke {
            repo: "project".into(),
            key: WRITER.to_vec(),
        },
    )
    .unwrap();
    let revoked = push(&sandbox, WRITER, "project", &commands, &pack_bytes);
    assert_eq!(revoked.unwrap_err().reason, reason::UNAUTHORIZED);
}

#[test]
fn a_non_fast_forward_is_reported_and_not_applied_unless_allowed() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let left = file_commit(&mut source, &[root], 2, &[("a", b"left")]);
    let right = file_commit(&mut source, &[root], 3, &[("a", b"right")]);
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));

    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, left, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let rewound = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, right, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        rewound,
        ["unpack ok", "ng refs/heads/main non-fast-forward"]
    );
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        left.to_hex()
    );

    let deleted = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, zero, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        deleted,
        ["unpack ok", "ng refs/heads/main deletion prohibited"]
    );

    act(
        &sandbox,
        OWNER,
        &Op::Configure {
            repo: "project".into(),
            settings: Settings {
                head: b"refs/heads/main".to_vec(),
                allow_force: true,
                allow_delete: true,
            },
        },
    )
    .unwrap();
    let forced = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, right, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(forced, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        right.to_hex()
    );
    let deleted = push(
        &sandbox,
        OWNER,
        "project",
        &[(right, zero, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(deleted, ["unpack ok", "ok refs/heads/main"]);
    assert!(refs_of(&sandbox, "project").is_empty());
}

#[test]
fn a_stale_old_value_is_reported() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let next = file_commit(&mut source, &[root], 2, &[("a", b"2")]);
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let stale = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, next, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(stale, ["unpack ok", "ng refs/heads/main stale info"]);
}

#[test]
fn an_import_arrives_as_fast_forward_steps_and_an_open_pack_is_refused_whole() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let first = file_commit(&mut source, &[], 1, &[("a", b"1"), ("b", b"1")]);
    let after_first: BTreeSet<Oid> = source.ids().copied().collect();
    let second = file_commit(&mut source, &[first], 2, &[("a", b"2"), ("b", b"1")]);
    let third = file_commit(&mut source, &[second], 3, &[("a", b"3"), ("b", b"1")]);
    let later: Vec<Oid> = source
        .ids()
        .copied()
        .filter(|id| !after_first.contains(id))
        .collect();
    let zero = Hash::Sha1.zero();

    let open = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, third, "refs/heads/main")],
        &pack_of(&source, &later),
    )
    .unwrap();
    let missing = open[0]
        .strip_prefix("unpack missing object ")
        .map(|hex| Oid::from_hex(Hash::Sha1, hex).unwrap())
        .unwrap();
    assert!(after_first.contains(&missing), "{}", open[0]);
    assert_eq!(open[1], "ng refs/heads/main n/a (unpacker error)");
    assert_eq!(sandbox.blob_count(), 0);
    assert!(refs_of(&sandbox, "project").is_empty());

    let step_one = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, first, "refs/heads/main")],
        &pack_of(&source, &after_first.iter().copied().collect::<Vec<_>>()),
    )
    .unwrap();
    assert_eq!(step_one, ["unpack ok", "ok refs/heads/main"]);
    let step_two = push(
        &sandbox,
        OWNER,
        "project",
        &[(first, third, "refs/heads/main")],
        &pack_of(&source, &later),
    )
    .unwrap();
    assert_eq!(step_two, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(sandbox.blob_count(), source.count());
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        third.to_hex()
    );
}

#[test]
fn a_walk_past_the_bound_asks_for_smaller_steps() {
    let sandbox = MemorySandbox::default();
    forge::init(
        &sandbox,
        &abi::encode(&Bounds {
            push_walk: 2,
            ..bounds()
        }),
    )
    .unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let mut tip = file_commit(&mut source, &[], 1, &[("a", b"0")]);
    let root = tip;
    for step in 1..5u8 {
        tip = file_commit(&mut source, &[tip], 1 + step as i64, &[("a", &[step])]);
    }
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let too_far = push(
        &sandbox,
        OWNER,
        "project",
        &[(root, tip, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        too_far,
        [
            "unpack ok",
            "ng refs/heads/main history too long to verify; push in smaller steps"
        ]
    );
}

#[test]
fn advertisement_lists_refs_for_receive_and_capabilities_for_upload() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let empty = ask(
        &sandbox,
        &Query::Advertise {
            repo: "project".into(),
            service: Service::ReceivePack,
        },
    )
    .unwrap();
    let lines = pkt_lines(&empty);
    assert_eq!(lines[0], "# service=git-receive-pack");
    assert_eq!(lines[1], "<flush>");
    assert!(lines[2].starts_with(&format!("{} capabilities^{{}}\0", Hash::Sha1.zero())));
    assert!(lines[2].contains("report-status"));
    assert!(lines[2].contains("object-format=sha1"));

    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    push(
        &sandbox,
        OWNER,
        "project",
        &[(Hash::Sha1.zero(), tip, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    let populated = ask(
        &sandbox,
        &Query::Advertise {
            repo: "project".into(),
            service: Service::ReceivePack,
        },
    )
    .unwrap();
    let lines = pkt_lines(&populated);
    assert!(lines[2].starts_with(&format!("{tip} refs/heads/main\0")));

    let upload = ask(
        &sandbox,
        &Query::Advertise {
            repo: "project".into(),
            service: Service::UploadPack,
        },
    )
    .unwrap();
    let lines = pkt_lines(&upload);
    assert_eq!(lines[0], "# service=git-upload-pack");
    assert_eq!(lines[1], "<flush>");
    assert_eq!(lines[2], "version 2");
    assert!(lines.contains(&"object-format=sha1".to_string()));

    let missing = ask(
        &sandbox,
        &Query::Advertise {
            repo: "nope".into(),
            service: Service::UploadPack,
        },
    );
    assert_eq!(missing.unwrap_err().reason, reason::NOT_FOUND);
}

#[test]
fn ls_refs_and_fetch_serve_what_was_pushed() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let tip = file_commit(&mut source, &[root], 2, &[("a", b"2")]);
    let zero = Hash::Sha1.zero();
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, tip, "refs/heads/main"), (zero, root, "refs/tags/v0")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();

    let listing = ask(
        &sandbox,
        &Query::Upload {
            repo: "project".into(),
            request: v2_request("ls-refs", Hash::Sha1, &["symrefs".into(), "peel".into()]),
        },
    )
    .unwrap();
    assert_eq!(
        pkt_lines(&listing),
        [
            format!("{tip} HEAD symref-target:refs/heads/main"),
            format!("{tip} refs/heads/main"),
            format!("{root} refs/tags/v0"),
            "<flush>".to_string(),
        ]
    );

    let clone = ask(
        &sandbox,
        &Query::Upload {
            repo: "project".into(),
            request: v2_request("fetch", Hash::Sha1, &[format!("want {tip}"), "done".into()]),
        },
    )
    .unwrap();
    let (sections, pack_bytes) = fetched_pack(&clone);
    assert_eq!(sections, ["packfile", "<flush>"]);
    assert_eq!(
        ids_in_pack(&pack_bytes, Hash::Sha1),
        source.ids().copied().collect()
    );

    let update = ask(
        &sandbox,
        &Query::Upload {
            repo: "project".into(),
            request: v2_request(
                "fetch",
                Hash::Sha1,
                &[format!("want {tip}"), format!("have {root}")],
            ),
        },
    )
    .unwrap();
    let (sections, pack_bytes) = fetched_pack(&update);
    assert_eq!(
        sections,
        [
            "acknowledgments".to_string(),
            format!("ACK {root}"),
            "ready".into(),
            "<delim>".into(),
            "packfile".into(),
            "<flush>".into(),
        ]
    );
    let served = ids_in_pack(&pack_bytes, Hash::Sha1);
    assert!(served.contains(&tip));
    assert!(!served.contains(&root));
}

#[test]
fn an_upload_session_ended_by_a_flush_answers_nothing() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    for request in [pktline::flush().to_vec(), Vec::new()] {
        let ended = ask(
            &sandbox,
            &Query::Upload {
                repo: "project".into(),
                request,
            },
        )
        .unwrap();
        assert!(ended.is_empty());
    }
}

#[test]
fn merge_fast_forwards_commits_or_names_the_conflicts() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n"), ("b", b"1\n")]);
    let feature = file_commit(&mut source, &[root], 2, &[("a", b"1\n"), ("b", b"2\n")]);
    let main = file_commit(&mut source, &[root], 3, &[("a", b"2\n"), ("b", b"1\n")]);
    let clash = file_commit(&mut source, &[root], 4, &[("a", b"3\n"), ("b", b"1\n")]);
    let zero = Hash::Sha1.zero();
    push(
        &sandbox,
        OWNER,
        "project",
        &[
            (zero, root, "refs/heads/main"),
            (zero, feature, "refs/heads/feature"),
            (zero, clash, "refs/heads/clash"),
        ],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();

    let merge = |from: &str| {
        act(
            &sandbox,
            OWNER,
            &Op::Merge {
                repo: "project".into(),
                into: b"refs/heads/main".to_vec(),
                from: from.as_bytes().to_vec(),
                message: b"Merge\n".to_vec(),
            },
        )
    };

    let forwarded = merge("refs/heads/feature").unwrap();
    assert_eq!(forwarded, feature.to_hex().into_bytes());
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        feature.to_hex()
    );

    let again = merge("refs/heads/feature");
    assert_eq!(again.unwrap_err().reason, reason::WRONG_STATE);

    let diverged = push(
        &sandbox,
        OWNER,
        "project",
        &[(feature, main, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(diverged[1], "ng refs/heads/main non-fast-forward");
    act(
        &sandbox,
        OWNER,
        &Op::Configure {
            repo: "project".into(),
            settings: Settings {
                allow_force: true,
                ..Settings::default()
            },
        },
    )
    .unwrap();
    push(
        &sandbox,
        OWNER,
        "project",
        &[(feature, main, "refs/heads/main")],
        b"",
    )
    .unwrap();

    let merged = merge("refs/heads/feature").unwrap();
    let merged_id = Oid::from_hex(Hash::Sha1, String::from_utf8(merged).unwrap()).unwrap();
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        merged_id.to_hex()
    );
    let commit_bytes = sandbox
        .blob_get(abi::BlobId::Sha1(match merged_id {
            Oid::Sha1(d) => d,
            Oid::Sha256(_) => unreachable!(),
        }))
        .unwrap();
    let merged_commit = Commit::parse(&commit_bytes.body, Hash::Sha1).unwrap();
    assert_eq!(merged_commit.parents, [main, feature]);
    assert_eq!(merged_commit.author.name, abi::hex(OWNER).into_bytes());
    assert_eq!(merged_commit.author.time, TIME as i64);

    let conflicted = merge("refs/heads/clash").unwrap_err();
    assert_eq!(conflicted.reason, reason::WRONG_STATE);
    assert!(conflicted.sentence.contains('a'), "{}", conflicted.sentence);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        merged_id.to_hex()
    );
}

#[test]
fn a_sha256_repository_keeps_its_own_ids() {
    let sandbox = founded();
    create(&sandbox, "modern", HashKind::Sha256);
    let mut source = MemoryObjects::new(Hash::Sha256);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let zero = Hash::Sha256.zero();
    let report = push(
        &sandbox,
        OWNER,
        "modern",
        &[(zero, tip, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    assert_eq!(report, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(refs_of(&sandbox, "modern")["refs/heads/main"], tip.to_hex());
    assert_eq!(tip.to_hex().len(), 64);

    let clone = ask(
        &sandbox,
        &Query::Upload {
            repo: "modern".into(),
            request: v2_request(
                "fetch",
                Hash::Sha256,
                &[format!("want {tip}"), "done".into()],
            ),
        },
    )
    .unwrap();
    let (_, pack_bytes) = fetched_pack(&clone);
    assert_eq!(
        ids_in_pack(&pack_bytes, Hash::Sha256),
        source.ids().copied().collect()
    );
}

#[test]
fn a_sha1_pack_is_refused_by_a_sha256_repository() {
    let sandbox = founded();
    create(&sandbox, "modern", HashKind::Sha256);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let request = push_request(
        &[(Hash::Sha1.zero(), tip, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    );
    let refused = act(
        &sandbox,
        OWNER,
        &Op::Push {
            repo: "modern".into(),
            request,
        },
    );
    assert_eq!(refused.unwrap_err().reason, reason::INVALID_INPUT);
}
