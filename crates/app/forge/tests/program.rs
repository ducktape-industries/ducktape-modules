mod common;
use common::*;

#[test]
fn founding_requires_bounds_and_ops_require_a_signer() {
    let sandbox = MemorySandbox::default();
    let unfounded = Forge::init(&sandbox.exec(1), b"").unwrap_err();
    assert_eq!(unfounded.code, code::INVALID_INPUT);
    assert!(
        unfounded
            .message
            .starts_with("forge: Bounds did not decode:"),
        "{unfounded}"
    );
    Forge::init(&sandbox.exec(1), &abi::encode(&bounds())).unwrap();

    let create = Op::Create {
        repo: "r".into(),
        hash: HashKind::Sha1,
    };
    for origin in [Origin::Root, Origin::Module("chat".into())] {
        let ctx = sandbox.forge.exec(sandbox.env_at(origin, 1, TIME));
        let refusal = sandbox
            .forge
            .refused(|| Forge::execute(&ctx, create.clone()));
        assert_eq!(refusal.code, code::UNAUTHORIZED);
    }
}

#[test]
fn create_names_an_owner_and_refuses_bad_or_taken_names() {
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let again = act(
        &mut sandbox,
        WRITER,
        &Op::Create {
            repo: "project".into(),
            hash: HashKind::Sha1,
        },
    );
    assert_eq!(again.unwrap_err().code, code::ALREADY_EXISTS);
    for bad in ["", "a/b", ".hidden", "x.git", "sp ace"] {
        let refused = act(
            &mut sandbox,
            OWNER,
            &Op::Create {
                repo: bad.into(),
                hash: HashKind::Sha1,
            },
        );
        assert_eq!(refused.unwrap_err().code, code::INVALID_INPUT, "{bad:?}");
    }
    let listed: Reply = abi::decode(
        &ask(
            &sandbox,
            &Query::Repos {
                page: PageRequest::first(128),
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
    assert_eq!(repos[0].repo.owner, person(OWNER));
    assert_eq!(repos[0].repo.settings, Settings::default());
}

#[test]
fn a_push_stores_the_objects_moves_the_ref_and_reports() {
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("README", b"hello\n")]);
    let zero = Hash::Sha1.zero();

    let report = push(
        &mut sandbox,
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
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let zero = Hash::Sha1.zero();
    let pack_bytes = pack_of(&source, &all_ids(&source));
    let commands = [(zero, tip, "refs/heads/main")];

    let stranger = push(&mut sandbox, STRANGER, "project", &commands, &pack_bytes);
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);
    assert_eq!(sandbox.blob_count(), 0);

    let grant_by_stranger = act(
        &mut sandbox,
        STRANGER,
        &Op::Grant {
            repo: "project".into(),
            principal: person(WRITER),
        },
    );
    assert_eq!(grant_by_stranger.unwrap_err().code, code::UNAUTHORIZED);

    act(
        &mut sandbox,
        OWNER,
        &Op::Grant {
            repo: "project".into(),
            principal: person(WRITER),
        },
    )
    .unwrap();
    let writer = push(&mut sandbox, WRITER, "project", &commands, &pack_bytes).unwrap();
    assert_eq!(writer, ["unpack ok", "ok refs/heads/main"]);

    act(
        &mut sandbox,
        OWNER,
        &Op::Revoke {
            repo: "project".into(),
            principal: person(WRITER),
        },
    )
    .unwrap();
    let revoked = push(&mut sandbox, WRITER, "project", &commands, &pack_bytes);
    assert_eq!(revoked.unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn a_non_fast_forward_is_reported_and_not_applied_unless_allowed() {
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let left = file_commit(&mut source, &[root], 2, &[("a", b"left")]);
    let right = file_commit(&mut source, &[root], 3, &[("a", b"right")]);
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));

    push(
        &mut sandbox,
        OWNER,
        "project",
        &[(zero, left, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let rewound = push(
        &mut sandbox,
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
        &mut sandbox,
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
        &mut sandbox,
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
        &mut sandbox,
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
        &mut sandbox,
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
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let next = file_commit(&mut source, &[root], 2, &[("a", b"2")]);
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));
    push(
        &mut sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let stale = push(
        &mut sandbox,
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
    let mut sandbox = founded();
    create(&mut sandbox, "project", HashKind::Sha1);
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
        &mut sandbox,
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
        &mut sandbox,
        OWNER,
        "project",
        &[(zero, first, "refs/heads/main")],
        &pack_of(&source, &after_first.iter().copied().collect::<Vec<_>>()),
    )
    .unwrap();
    assert_eq!(step_one, ["unpack ok", "ok refs/heads/main"]);
    let step_two = push(
        &mut sandbox,
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
    let mut sandbox = MemorySandbox::default();
    Forge::init(
        &sandbox.exec(1),
        &abi::encode(&Bounds {
            push_walk: 2,
            ..bounds()
        }),
    )
    .unwrap();
    create(&mut sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let mut tip = file_commit(&mut source, &[], 1, &[("a", b"0")]);
    let root = tip;
    for step in 1..5u8 {
        tip = file_commit(&mut source, &[tip], 1 + step as i64, &[("a", &[step])]);
    }
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));
    push(
        &mut sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let too_far = push(
        &mut sandbox,
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

/// A walk outlives the blocks that wrote nothing to forge, so a long log
/// finishes while the chain moves; forge writing mid-walk restarts it.
#[test]
fn a_cursor_outlives_empty_blocks_and_a_forge_write_restarts_it() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let log = |after| Query::Log {
        repo: REPO.into(),
        from: forge::Revision::Oid(story.feature.clone()),
        exclude: None,
        page: PageRequest {
            after,
            limit: Some(1),
        },
    };
    let page = |rig: &Rig, after| match abi::decode(&rig.query(&log(after)).unwrap()).unwrap() {
        Reply::Log { page, .. } => page,
        _ => panic!("a log"),
    };
    let first = page(&rig, None);
    assert_eq!(first.items[0].oid, story.feature);
    rig.advance();
    rig.advance();
    let second = page(&rig, first.next.clone());
    assert_eq!(second.items[0].oid, story.root);
    rig.sandbox.hold(b"ninth", 9);
    rig.execute(&Op::Grant {
        repo: REPO.into(),
        principal: Principal::Account(9),
    })
    .unwrap();
    assert_eq!(rig.query(&log(first.next)).unwrap_err().code, code::STALE);
}

/// A change's commits are its source's history less its target's.
#[test]
fn a_log_leaves_out_what_its_exclude_reaches() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let bytes = rig
        .query(&Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: Some(reference("main")),
            page: PageRequest::first(64),
        })
        .unwrap();
    let Reply::Log { page, .. } = abi::decode(&bytes).unwrap() else {
        panic!("a log");
    };
    let oids: Vec<_> = page.items.into_iter().map(|c| c.oid).collect();
    assert_eq!(oids, [story.feature]);
}

/// The app reads at the preconfirmed layer, whose height is the one its
/// next ops run at: a push in the block that answered the cursor rewrites
/// the log at the same height, and the cursor still tells.
#[test]
fn a_forge_write_at_the_answering_height_restarts_the_walk() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let log = |after| Query::Log {
        repo: REPO.into(),
        from: reference("feature"),
        exclude: None,
        page: PageRequest {
            after,
            limit: Some(1),
        },
    };
    let Reply::Log { page, .. } = abi::decode(&rig.query(&log(None)).unwrap()).unwrap() else {
        panic!("a log");
    };
    assert_eq!(page.items[0].oid, story.feature);
    // no advance: the same height the page was answered at
    rig.sandbox.hold(b"ninth", 9);
    signed_op(
        &rig.sandbox,
        &rig.actor,
        rig.height,
        &Op::Grant {
            repo: REPO.into(),
            principal: Principal::Account(9),
        },
    )
    .unwrap();
    assert_eq!(rig.query(&log(page.next)).unwrap_err().code, code::STALE);
}

/// What `exclude` reaches walks under its own budget: a target with more
/// history than the change's own commits does not refuse the listing.
#[test]
fn a_log_exclude_spends_its_own_walk_budget() {
    use common::story::*;
    let mut rig = Rig::start(
        forge::Bounds {
            log_walk: 2,
            ..bounds()
        },
        HashKind::Sha1,
    );
    let story = Story::pushed(&mut rig);
    let bytes = rig
        .query(&Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: Some(reference("unrelated")),
            page: PageRequest::first(64),
        })
        .unwrap();
    let Reply::Log { page, .. } = abi::decode(&bytes).unwrap() else {
        panic!("a log");
    };
    let oids: Vec<_> = page.items.into_iter().map(|c| c.oid).collect();
    assert_eq!(oids, [story.feature, story.root]);
}
