// Protocol v2 fetch tests: capability advertisement, ls-refs, negotiation, and reading back the streamed pack.

mod common;

use common::{fixture, oid_list, oid_text, sha1};
use gitcore::server::admit_pack;
use gitcore::wire::pktline::{self, Pkt, Reader};
use gitcore::wire::upload::{
    acknowledgments, capability_advertisement, fetch, ls_refs_response, parse_command, Command,
    Fetch, LsRefs,
};
use gitcore::{pack, Error, Hash, Kind, Limits, MemoryObjects, Objects, Oid};
use std::collections::{BTreeMap, BTreeSet};

fn repo() -> (MemoryObjects, BTreeMap<Vec<u8>, Oid>, Oid, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    admit_pack(
        &mut store,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    admit_pack(
        &mut store,
        fixture!("thin.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    let tag = store.put(Kind::Tag, fixture!("tag.bin")).unwrap();
    assert_eq!(tag, oid_text(fixture!("tag.oid"), Hash::Sha1));
    let base = oid_text(fixture!("base.oid"), Hash::Sha1);
    let tip = oid_text(fixture!("tip.oid"), Hash::Sha1);
    let mut refs = BTreeMap::new();
    refs.insert(b"refs/heads/main".to_vec(), tip);
    refs.insert(b"refs/heads/old".to_vec(), base);
    refs.insert(b"refs/tags/v1".to_vec(), tag);
    (store, refs, base, tip, tag)
}

fn request(command: &str, args: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, format!("command={command}").as_bytes());
    pktline::push_line(&mut out, b"agent=git/2.55.0");
    pktline::push_line(&mut out, b"object-format=sha1");
    out.extend_from_slice(pktline::delim());
    for arg in args {
        pktline::push_line(&mut out, arg.as_bytes());
    }
    out.extend_from_slice(pktline::flush());
    out
}

fn lines(bytes: &[u8]) -> Vec<String> {
    Reader::new(bytes)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            Pkt::Flush => "<flush>".into(),
            Pkt::Delim => "<delim>".into(),
            Pkt::ResponseEnd => "<end>".into(),
        })
        .collect()
}

#[test]
fn capability_advertisement_golden() {
    let body = capability_advertisement(Hash::Sha1, b"ducktape/0.1");
    assert_eq!(
        lines(&body),
        [
            "version 2",
            "agent=ducktape/0.1",
            "ls-refs=unborn",
            "fetch=wait-for-done",
            "server-option",
            "object-format=sha1",
            "<flush>"
        ]
    );
    assert!(lines(&capability_advertisement(Hash::Sha256, b"x"))
        .contains(&"object-format=sha256".to_string()));
}

#[test]
fn parse_commands() {
    let ls = parse_command(
        &request(
            "ls-refs",
            &[
                "symrefs",
                "peel",
                "unborn",
                "ref-prefix HEAD",
                "ref-prefix refs/heads/",
            ],
        ),
        Hash::Sha1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        ls,
        Command::LsRefs(LsRefs {
            symrefs: true,
            peel: true,
            unborn: true,
            ref_prefixes: vec![b"HEAD".to_vec(), b"refs/heads/".to_vec()],
        })
    );
    let no_args = parse_command(b"0014command=ls-refs\n0000", Hash::Sha1)
        .unwrap()
        .unwrap();
    assert_eq!(no_args, Command::LsRefs(LsRefs::default()));

    let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let fetched = parse_command(
        &request(
            "fetch",
            &[
                "thin-pack",
                "no-progress",
                "ofs-delta",
                "include-tag",
                &format!("want {a}"),
                &format!("have {b}"),
                "done",
            ],
        ),
        Hash::Sha1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        fetched,
        Command::Fetch(Fetch {
            wants: vec![sha1(a)],
            haves: vec![sha1(b)],
            done: true,
            thin_pack: true,
            no_progress: true,
            include_tag: true,
            ofs_delta: true,
            sideband_all: false,
            wait_for_done: false,
        })
    );

    assert_eq!(
        parse_command(&request("fetch", &["deepen 1"]), Hash::Sha1),
        Err(Error::Unsupported)
    );
    assert_eq!(
        parse_command(&request("fetch", &["filter blob:none"]), Hash::Sha1),
        Err(Error::Unsupported)
    );
    assert_eq!(
        parse_command(&request("fetch", &["want-ref refs/heads/main"]), Hash::Sha1),
        Err(Error::Unsupported)
    );
    assert_eq!(
        parse_command(&request("fetch", &["bogus"]), Hash::Sha1),
        Err(Error::BadRequest)
    );
    assert_eq!(
        parse_command(&request("push", &[]), Hash::Sha1),
        Err(Error::UnknownCommand)
    );
    assert_eq!(parse_command(b"0000", Hash::Sha1), Ok(None));
    assert_eq!(parse_command(b"", Hash::Sha1), Ok(None));
    assert_eq!(
        parse_command(b"0015agent=git/2.55.0\n0000", Hash::Sha1),
        Err(Error::BadRequest)
    );
    assert_eq!(
        parse_command(&request("fetch", &[]), Hash::Sha256),
        Err(Error::WrongHash {
            expected: Hash::Sha256,
            actual: Hash::Sha1
        })
    );
}

#[test]
fn ls_refs_lists_head_symref_and_peeled_tags() {
    let (store, refs, base, tip, tag) = repo();
    let command = LsRefs {
        symrefs: true,
        peel: true,
        unborn: true,
        ref_prefixes: Vec::new(),
    };
    let body = ls_refs_response(&store, &refs, Some(b"refs/heads/main"), &command, 10).unwrap();
    assert_eq!(
        lines(&body),
        [
            format!("{tip} HEAD symref-target:refs/heads/main"),
            format!("{tip} refs/heads/main"),
            format!("{base} refs/heads/old"),
            format!("{tag} refs/tags/v1 peeled:{base}"),
            "<flush>".into(),
        ]
    );

    let unborn = ls_refs_response(&store, &refs, Some(b"refs/heads/nope"), &command, 10).unwrap();
    assert_eq!(
        lines(&unborn)[0],
        "unborn HEAD symref-target:refs/heads/nope"
    );
    let no_unborn = LsRefs {
        unborn: false,
        ..command.clone()
    };
    let hidden = ls_refs_response(&store, &refs, Some(b"refs/heads/nope"), &no_unborn, 10).unwrap();
    assert!(lines(&hidden)[0].ends_with("refs/heads/main"));

    let only_tags = LsRefs {
        ref_prefixes: vec![b"refs/tags/".to_vec()],
        ..LsRefs::default()
    };
    let body = ls_refs_response(&store, &refs, Some(b"refs/heads/main"), &only_tags, 10).unwrap();
    assert_eq!(
        lines(&body),
        [format!("{tag} refs/tags/v1"), "<flush>".into()]
    );
    assert_eq!(
        lines(&ls_refs_response(&store, &BTreeMap::new(), None, &command, 10).unwrap()),
        ["<flush>"]
    );
}

#[test]
fn acknowledgment_section_golden() {
    let a = sha1("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        lines(&acknowledgments(&[], false)),
        ["acknowledgments", "NAK"]
    );
    assert_eq!(
        lines(&acknowledgments(&[a], true)),
        ["acknowledgments", format!("ACK {a}").as_str(), "ready"]
    );
}

struct Response {
    sections: Vec<String>,
    pack: Vec<u8>,
}

fn run_fetch(store: &MemoryObjects, refs: &BTreeMap<Vec<u8>, Oid>, command: &Fetch) -> Response {
    let mut bytes = Vec::new();
    let mut sink = |chunk: &[u8]| bytes.extend_from_slice(chunk);
    fetch(store, refs, command, 1000, &mut sink).unwrap();
    let mut sections = Vec::new();
    let mut pack = Vec::new();
    for pkt in Reader::new(&bytes) {
        match pkt.unwrap() {
            Pkt::Data(data) if sections.last().is_some_and(|s| s == "packfile") => {
                assert_eq!(data[0], 1);
                pack.extend_from_slice(&data[1..]);
            }
            Pkt::Data(data) => {
                sections.push(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            Pkt::Flush => sections.push("<flush>".into()),
            Pkt::Delim => sections.push("<delim>".into()),
            Pkt::ResponseEnd => sections.push("<end>".into()),
        }
    }
    Response { sections, pack }
}

fn read_pack(bytes: &[u8]) -> BTreeSet<Oid> {
    pack::read(bytes, Hash::Sha1, &Limits::generous(), |_| Ok(None))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn full_fetch_flow_clone_then_incremental() {
    let (store, refs, base, tip, tag) = repo();
    let base_objects: BTreeSet<Oid> = oid_list(fixture!("base.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    let delta_objects: BTreeSet<Oid> = oid_list(fixture!("delta.oids"), Hash::Sha1)
        .into_iter()
        .collect();

    let clone = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(clone.sections, ["packfile", "<flush>"]);
    let got = read_pack(&clone.pack);
    let mut everything = base_objects.clone();
    everything.extend(delta_objects.iter().copied());
    assert_eq!(got, everything);

    let mut client = MemoryObjects::new(Hash::Sha1);
    admit_pack(&mut client, &clone.pack, Hash::Sha1, &Limits::generous()).unwrap();
    assert!(client.has(&tip).unwrap());

    let incremental = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            haves: vec![base, sha1("1234567812345678123456781234567812345678")],
            ..Fetch::default()
        },
    );
    assert_eq!(
        incremental.sections,
        [
            "acknowledgments",
            &format!("ACK {base}"),
            "ready",
            "<delim>",
            "packfile",
            "<flush>"
        ]
    );
    assert_eq!(read_pack(&incremental.pack), delta_objects);
    let mut client = MemoryObjects::new(Hash::Sha1);
    admit_pack(
        &mut client,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    admit_pack(
        &mut client,
        &incremental.pack,
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    assert!(client.has(&tip).unwrap());

    let nothing_common = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            haves: vec![sha1("1234567812345678123456781234567812345678")],
            ..Fetch::default()
        },
    );
    assert_eq!(
        nothing_common.sections,
        ["acknowledgments", "NAK", "<flush>"]
    );
    assert!(nothing_common.pack.is_empty());

    let waiting = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            haves: vec![base],
            wait_for_done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(
        waiting.sections,
        ["acknowledgments", &format!("ACK {base}"), "<flush>"]
    );

    let up_to_date = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            haves: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(up_to_date.sections, ["packfile", "<flush>"]);
    assert!(read_pack(&up_to_date.pack).is_empty());

    let tagged = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tag],
            done: true,
            ..Fetch::default()
        },
    );
    let got = read_pack(&tagged.pack);
    let mut expected = base_objects.clone();
    expected.insert(tag);
    assert_eq!(got, expected);

    let tag_over_base = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tag],
            haves: vec![base],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(read_pack(&tag_over_base.pack), [tag].into_iter().collect());
}

#[test]
fn fetch_refuses_unadvertised_wants_and_empty_requests() {
    let (store, refs, _, _, _) = repo();
    let mut bytes = Vec::new();
    let mut sink = |chunk: &[u8]| bytes.extend_from_slice(chunk);
    let stray = sha1("1234567812345678123456781234567812345678");
    assert_eq!(
        fetch(
            &store,
            &refs,
            &Fetch {
                wants: vec![stray],
                done: true,
                ..Fetch::default()
            },
            100,
            &mut sink
        ),
        Err(Error::NotAdvertised(stray))
    );
    assert_eq!(
        fetch(&store, &refs, &Fetch::default(), 100, &mut sink),
        Err(Error::BadRequest)
    );
    assert!(bytes.is_empty());
}

#[test]
fn fetch_pack_is_valid_for_git() {
    let Some(git) = git_binary() else {
        return;
    };
    let (store, refs, _, tip, _) = repo();
    let clone = run_fetch(
        &store,
        &refs,
        &Fetch {
            wants: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    let dir = std::env::temp_dir().join(format!("gitcore-fetch-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    assert!(std::process::Command::new(&git)
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap()
        .success());
    let mut child = std::process::Command::new(&git)
        .args(["index-pack", "--stdin", "--strict"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), &clone.pack).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let fsck = std::process::Command::new(&git)
        .args(["fsck", "--connectivity-only", &tip.to_hex()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(fsck.status.success(), "{fsck:?}");
    std::fs::remove_dir_all(&dir).ok();
}

fn git_binary() -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("which")
        .arg("git")
        .output()
        .ok()?;
    let found = output.status.success();
    if !found {
        return None;
    }
    Some(std::path::PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}
