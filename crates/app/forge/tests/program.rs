mod common;
use common::*;

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
    let listed: Reply = abi::decode(
        &ask(
            &sandbox,
            &Query::Repos {
                cursor: None,
                limit: 128,
            },
        )
        .unwrap(),
    )
    .unwrap();
    let Reply::Repos { page, .. } = listed else {
        panic!();
    };
    let repos = page.items;
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
