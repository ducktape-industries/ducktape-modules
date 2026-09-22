mod common;
use common::*;

#[test]
fn client_merge_fast_forwards_commits_or_names_the_conflicts() {
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
    let merge = |from: &str, old: Oid, expected: Oid, result: Oid| {
        act(
            &sandbox,
            OWNER,
            &Op::Merge {
                repo: "project".into(),
                into: b"refs/heads/main".to_vec(),
                from: forge::Revision::Ref(from.as_bytes().to_vec()),
                expected_into: old.to_hex(),
                expected_from: expected.to_hex(),
                result: result.to_hex(),
                change: None,
            },
        )
    };
    let compare = |from: &str| {
        let reply: Reply = abi::decode(
            &ask(
                &sandbox,
                &Query::Compare {
                    repo: "project".into(),
                    from: forge::Revision::Ref(from.as_bytes().to_vec()),
                    into: forge::Revision::Ref(b"refs/heads/main".to_vec()),
                    cursor: None,
                    limit: 128,
                },
            )
            .unwrap(),
        )
        .unwrap();
        let Reply::Compare {
            comparison,
            conflicts,
            ..
        } = reply
        else {
            panic!("{reply:?}");
        };
        (comparison.mergeability, conflicts.items)
    };
    assert_eq!(
        compare("refs/heads/feature").0,
        forge::Mergeability::FastForward
    );
    let forwarded: forge::OpReply =
        abi::decode(&merge("refs/heads/feature", root, feature, feature).unwrap()).unwrap();
    assert_eq!(
        forwarded,
        forge::OpReply::Merged {
            height: 1,
            oid: feature.to_hex(),
            change: None
        }
    );
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        feature.to_hex()
    );
    assert_eq!(
        merge("refs/heads/feature", feature, feature, feature)
            .unwrap_err()
            .reason,
        reason::WRONG_STATE
    );
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
    assert_eq!(compare("refs/heads/feature").0, forge::Mergeability::Clean);
    let tree_of = |id| {
        Commit::parse(&source.get(&id).unwrap().unwrap().body, Hash::Sha1)
            .unwrap()
            .tree
    };
    let (base, ours, theirs) = (tree_of(root), tree_of(main), tree_of(feature));
    let gitcore::merge::MergeOutcome::Clean(tree) =
        gitcore::merge::merge_trees(&mut source, Some(&base), &ours, &theirs, 100).unwrap()
    else {
        panic!("clean merge");
    };
    let author = Signature {
        name: abi::hex(OWNER).into_bytes(),
        email: Vec::new(),
        time: TIME as i64,
        offset_minutes: 0,
    };
    let merged_commit = Commit {
        tree,
        parents: vec![main, feature],
        author: author.clone(),
        committer: author,
        extra: Vec::new(),
        message: b"Merge\n".to_vec(),
    };
    let merged_id = source
        .put(Kind::Commit, &merged_commit.serialize())
        .unwrap();
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, merged_id, "refs/heads/merge-result")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    merge("refs/heads/feature", main, feature, merged_id).unwrap();
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        merged_id.to_hex()
    );
    let Oid::Sha1(digest) = merged_id else {
        panic!();
    };
    let actual = Commit::parse(
        &sandbox.blob_get(abi::BlobId::Sha1(digest)).unwrap().body,
        Hash::Sha1,
    )
    .unwrap();
    assert_eq!(actual.parents, [main, feature]);
    assert_eq!(actual.author.name, abi::hex(OWNER).into_bytes());
    assert_eq!(actual.author.time, TIME as i64);
    let conflicts = compare("refs/heads/clash");
    assert_eq!(conflicts.0, forge::Mergeability::Conflicts);
    assert!(conflicts.1.iter().any(|c| c.path == b"a"));
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
