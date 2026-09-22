mod common;
use common::story::*;
use common::*;
use forge::*;

#[test]
fn real_push_answers_every_object_query_without_mutating_state() {
    assert!(!skipped(), "these contract tests require git");
    let remote = Remote::start(
        Bounds {
            blob_bytes: 64,
            ..forge_harness::default_bounds()
        },
        HashKind::Sha1,
    );
    let story = Story::pushed(&remote);
    let before = remote.snapshot();
    let Reply::Repos { height, page } = remote.query(&Query::Repos {
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(height, before.height());
    assert_eq!(page.items[0].repo.refs_count, 6);
    assert_eq!(page.items[0].repo.last_activity, height);
    let Reply::Repo {
        repo,
        bounds,
        writers,
        ..
    } = remote.query(&Query::Repo {
        repo: REPO.into(),
        cursor: None,
        limit: 1,
    })
    else {
        panic!();
    };
    assert_eq!(repo.repo.owner, b"tester");
    assert_eq!(bounds.blob_bytes, 64);
    assert!(writers.items.is_empty());
    let mut cursor = None;
    let mut refs = Vec::new();
    loop {
        let Reply::Refs { page, .. } = remote.query(&Query::Refs {
            repo: REPO.into(),
            cursor,
            limit: 2,
        }) else {
            panic!();
        };
        refs.extend(page.items);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(refs.len(), 6);
    assert!(
        refs.iter()
            .any(|r| r.name == b"refs/heads/feature" && r.target == story.feature)
    );
    let Reply::Log { page, tip, .. } = remote.query(&Query::Log {
        repo: REPO.into(),
        from: reference("feature"),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(tip, story.feature);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].parents, std::slice::from_ref(&story.root));
    assert_eq!(page.items[0].message, b"Feature\n\nReview these bytes.\n");
    assert_eq!(page.items[0].author.name, b"Ada");
    assert_eq!(page.items[0].author.time, 1_700_000_000);
    let Reply::Log { page, .. } = remote.query(&Query::Log {
        repo: REPO.into(),
        from: reference("feature"),
        cursor: page.next,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].oid, story.root);
    assert!(page.next.is_none());
    let Reply::Log { tip, .. } = remote.query(&Query::Log {
        repo: REPO.into(),
        from: Revision::Ref(b"refs/tags/v1".to_vec()),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(tip, story.root);
    let mut cursor = None;
    let mut entries = Vec::new();
    loop {
        let Reply::Tree { page, tree, .. } = remote.query(&Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: vec![],
            cursor,
            limit: 2,
        }) else {
            panic!();
        };
        assert_eq!(tree, story.oid("feature^{tree}"));
        entries.extend(page.items);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(entries.len(), 8);
    assert!(
        entries
            .iter()
            .any(|e| e.name == b"vendor" && e.kind == EntryKind::Gitlink)
    );
    assert!(
        entries
            .iter()
            .any(|e| e.name == b"mode.sh" && e.kind == EntryKind::Executable)
    );
    let Reply::Tree { page, .. } = remote.query(&Query::Tree {
        repo: REPO.into(),
        at: story.feature.clone(),
        path: b"src".to_vec(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].name, b"lib.rs");
    let oid = page.items[0].oid.clone();
    let Reply::Blob { blob, .. } = remote.query(&Query::Blob {
        repo: REPO.into(),
        oid: oid.clone(),
        range: Some(ByteRange { offset: 4, len: 6 }),
    }) else {
        panic!();
    };
    assert_eq!(blob.bytes, b"keep\n+");
    assert_eq!(blob.range, ByteRange { offset: 4, len: 6 });
    for (path, content, size) in [
        ("image.bin", Content::Binary, 4),
        ("large.txt", Content::Oversize, 258),
        ("empty.txt", Content::Text, 0),
    ] {
        let Reply::Blob { blob, .. } = remote.query(&Query::Blob {
            repo: REPO.into(),
            oid: story.oid(&format!("feature:{path}")),
            range: None,
        }) else {
            panic!();
        };
        assert_eq!(blob.content, content);
        assert_eq!(blob.size, size);
        assert!(blob.bytes.is_empty());
    }
    let mut cursor = None;
    let mut files = Vec::new();
    loop {
        let Reply::Diff {
            page, total_files, ..
        } = remote.query(&Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.feature.clone(),
            path: None,
            cursor,
            limit: 2,
        })
        else {
            panic!();
        };
        assert_eq!(total_files, 8);
        files.extend(page.items);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(files.len(), 8);
    let source = files
        .iter()
        .find(|f| f.new_path.as_deref() == Some(b"src/lib.rs"))
        .unwrap();
    assert_eq!((source.additions, source.deletions), (2, 1));
    let lines = &source.hunks[0].lines;
    assert!(
        lines
            .iter()
            .any(|l| l.kind == LineKind::Added && l.bytes == b"++ x\n" && l.new_line == Some(3))
    );
    assert!(
        lines
            .iter()
            .any(|l| l.kind == LineKind::Deleted && l.bytes == b"old\n" && l.old_line == Some(3))
    );
    assert_eq!(lines.last().unwrap().bytes, b"added");
    assert_eq!(source.hunks[0].old, LineRange { start: 1, count: 4 });
    assert_eq!(source.hunks[0].new, LineRange { start: 1, count: 5 });
    assert!(
        files
            .iter()
            .any(|f| f.old_path.as_deref() == Some(b"gone.txt")
                && f.new_path.is_none()
                && f.status == FileStatus::Deleted)
    );
    assert!(files.iter().any(|f| f.status == FileStatus::ModeChanged));
    for content in [Content::Binary, Content::Oversize, Content::Gitlink] {
        assert!(
            files
                .iter()
                .any(|f| f.content == content && f.hunks.is_empty())
        );
    }
    let Reply::Diff { page, .. } = remote.query(&Query::Diff {
        repo: REPO.into(),
        base: None,
        head: story.root.clone(),
        path: Some(b"README.md".to_vec()),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].hunks[0].old, LineRange { start: 0, count: 0 });
    assert_eq!(page.items[0].hunks[0].new, LineRange { start: 1, count: 1 });
    for (from, into, expected, ahead, behind) in [
        ("feature", "main", Mergeability::FastForward, 1, 0),
        ("main", "feature", Mergeability::UpToDate, 0, 1),
        ("feature", "clean", Mergeability::Clean, 1, 1),
        ("feature", "conflict", Mergeability::Conflicts, 1, 1),
        ("feature", "unrelated", Mergeability::Unrelated, 2, 1),
    ] {
        let Reply::Compare {
            comparison,
            conflicts,
            ..
        } = remote.query(&compare(from, into))
        else {
            panic!();
        };
        assert_eq!(comparison.mergeability, expected);
        assert_eq!((comparison.ahead, comparison.behind), (ahead, behind));
        if expected == Mergeability::Conflicts {
            assert_eq!(conflicts.items[0].path, b"src/lib.rs");
        }
    }
    let Reply::Activity {
        height,
        last_height,
    } = remote.query(&Query::Activity { repo: REPO.into() })
    else {
        panic!();
    };
    assert_eq!(height, last_height);
    let after = remote.snapshot();
    assert_eq!(before.state_snapshot(), after.state_snapshot());
    assert_eq!(before.blob_count(), after.blob_count());
}

#[test]
fn cursor_limit_path_and_work_failures_are_typed_at_the_answer_height() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    let Reply::Refs { page, .. } = remote.query(&Query::Refs {
        repo: REPO.into(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    let cursor = page.next.unwrap();
    let refused = remote.query(&Query::Tree {
        repo: REPO.into(),
        at: story.root.clone(),
        path: vec![],
        cursor: Some(cursor.clone()),
        limit: 1,
    });
    assert!(matches!(refused,Reply::Refused{reason,..} if reason==abi::reason::INVALID_INPUT));
    remote.advance();
    let refused = remote.query(&Query::Refs {
        repo: REPO.into(),
        cursor: Some(cursor),
        limit: 1,
    });
    assert!(
        matches!(refused,Reply::Refused{reason,height,..} if reason==abi::reason::STALE && height==remote.snapshot().height())
    );
    for q in [
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 0,
        },
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 129,
        },
        Query::Tree {
            repo: REPO.into(),
            at: story.root.clone(),
            path: b"../src".to_vec(),
            cursor: None,
            limit: 1,
        },
        Query::Blob {
            repo: REPO.into(),
            oid: story.oid("main:README.md"),
            range: Some(ByteRange {
                offset: u64::MAX,
                len: 1,
            }),
        },
    ] {
        assert!(
            matches!(remote.query(&q),Reply::Refused{reason,..} if reason==abi::reason::INVALID_INPUT)
        );
    }
    let narrow = Remote::start(
        Bounds {
            log_walk: 1,
            tree_walk: 1,
            ..forge_harness::default_bounds()
        },
        HashKind::Sha1,
    );
    let story = Story::pushed(&narrow);
    for q in [
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            cursor: None,
            limit: 1,
        },
        Query::Tree {
            repo: REPO.into(),
            at: story.root,
            path: vec![],
            cursor: None,
            limit: 1,
        },
    ] {
        assert!(
            matches!(narrow.query(&q),Reply::Refused{reason,..} if reason==abi::reason::CAPACITY)
        );
    }
}

#[test]
fn an_object_missing_on_the_serving_node_is_a_borsh_refusal() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    let oid = story.oid("feature:src/lib.rs");
    let mut snapshot = remote.snapshot();
    let mut digest = [0u8; 20];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&oid[i * 2..i * 2 + 2], 16).unwrap();
    }
    snapshot.remove_blob(&abi::BlobId::Sha1(digest));
    let sparse = Harness::from_snapshot(wasm(), snapshot).unwrap();
    for q in [
        Query::Blob {
            repo: REPO.into(),
            oid,
            range: None,
        },
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root),
            head: story.feature,
            path: Some(b"src/lib.rs".to_vec()),
            cursor: None,
            limit: 1,
        },
        compare("feature", "conflict"),
    ] {
        let bytes = remote.rt.block_on(sparse.query(&q)).unwrap();
        let reply: Reply = abi::decode(&bytes).unwrap();
        assert!(matches!(reply,Reply::Refused{reason,..} if reason==OBJECT_NOT_HELD));
    }
    let reply = remote.query(&Query::Blob {
        repo: REPO.into(),
        oid: "f".repeat(40),
        range: None,
    });
    assert!(matches!(reply,Reply::Refused{reason,..} if reason==OBJECT_NOT_HELD));
}

#[test]
fn access_pages_and_activity_order_follow_real_grants_pushes_and_revocations() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    for key in [b"writer-a", b"writer-b"] {
        remote
            .execute(&Op::Grant {
                repo: REPO.into(),
                key: key.to_vec(),
            })
            .unwrap();
    }
    let Reply::Repo { writers, .. } = remote.query(&Query::Repo {
        repo: REPO.into(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(writers.items, [b"writer-a".to_vec()]);
    let Reply::Repo { writers, .. } = remote.query(&Query::Repo {
        repo: REPO.into(),
        cursor: writers.next,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(writers.items, [b"writer-b".to_vec()]);
    assert!(writers.next.is_none());
    remote.actor(b"writer-a");
    git(
        story.source.path(),
        &["push", "-q", &remote.url, "feature:refs/heads/granted"],
    );
    remote.actor(b"tester");
    remote
        .execute(&Op::Revoke {
            repo: REPO.into(),
            key: b"writer-a".to_vec(),
        })
        .unwrap();
    remote.actor(b"writer-a");
    let error = git_fails(
        story.source.path(),
        &["push", "-q", &remote.url, "feature:refs/heads/refused"],
    );
    assert!(error.contains("403"), "{error}");
    remote.actor(b"tester");
    remote
        .execute(&Op::Create {
            repo: "other".into(),
            hash: HashKind::Sha1,
        })
        .unwrap();
    let Reply::Repos { page, .. } = remote.query(&Query::Repos {
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].name, "other");
    let Reply::Repos { page, .. } = remote.query(&Query::Repos {
        cursor: page.next,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].name, REPO);
    remote
        .execute(&Op::Configure {
            repo: REPO.into(),
            settings: Settings::default(),
        })
        .unwrap();
    let Reply::Repos { page, .. } = remote.query(&Query::Repos {
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].name, REPO);
}
