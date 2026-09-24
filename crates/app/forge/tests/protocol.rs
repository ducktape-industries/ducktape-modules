mod common;
use common::*;

#[test]
fn advertisement_lists_refs_for_receive_and_capabilities_for_upload() {
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
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
        &mut sandbox,
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
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let tip = file_commit(&mut source, &[root], 2, &[("a", b"2")]);
    let zero = Hash::Sha1.zero();
    push(
        &mut sandbox,
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
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
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
