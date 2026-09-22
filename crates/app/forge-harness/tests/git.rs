mod common;
use common::*;

#[test]
fn push_then_clone() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);

    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);
    assert_eq!(log(&cloned), log(source.path()));
    for name in ["file-0", "file-1"] {
        let expected = std::fs::read(source.path().join(name)).expect("source file");
        let actual = std::fs::read(cloned.join(name)).expect("cloned file");
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn incremental_fetch() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);

    commit_file(source.path(), "file-2", "2\n");
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    git(&cloned, &["pull", "-q", "--ff-only"]);
    assert_eq!(head(&cloned), head(source.path()));
    assert_eq!(log(&cloned), log(source.path()));

    git(source.path(), &["branch", "side", "main~1"]);
    git(source.path(), &["tag", "-a", "v1", "-m", "v1"]);
    git(source.path(), &["push", "-q", &remote.url, "side", "v1"]);
    let listed = git(source.path(), &["ls-remote", &remote.url]);
    let side = git(source.path(), &["rev-parse", "side"]);
    let tag = git(source.path(), &["rev-parse", "v1"]);
    assert!(listed.contains(&format!("{}\trefs/heads/main", head(source.path()))));
    assert!(listed.contains(&format!("{}\trefs/heads/side", side.trim())));
    assert!(listed.contains(&format!("{}\trefs/tags/v1", tag.trim())));
    assert!(listed.contains(&format!("{}\trefs/tags/v1^{{}}", head(source.path()))));
}

#[test]
fn non_fast_forward_is_rejected() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(2);
    git(source.path(), &["push", "-q", &remote.url, "main"]);
    let pushed = head(source.path());

    git(source.path(), &["reset", "-q", "--hard", "HEAD~1"]);
    commit_file(source.path(), "rewritten", "x\n");
    let rewound = head(source.path());

    let refused = git_fails(source.path(), &["push", &remote.url, "main"]);
    assert!(refused.contains("non-fast-forward"), "{refused}");
    let forced = git_fails(source.path(), &["push", "--force", &remote.url, "main"]);
    assert!(forced.contains("[remote rejected]"), "{forced}");
    assert!(forced.contains("non-fast-forward"), "{forced}");
    let listed = git(
        source.path(),
        &["ls-remote", &remote.url, "refs/heads/main"],
    );
    assert!(listed.starts_with(&pushed), "{listed}");

    remote
        .execute(&Op::Configure {
            repo: REPO.into(),
            settings: Settings {
                allow_force: true,
                ..Settings::default()
            },
        })
        .expect("the owner configures");
    git(
        source.path(),
        &["push", "-q", "--force", &remote.url, "main"],
    );
    let listed = git(
        source.path(),
        &["ls-remote", &remote.url, "refs/heads/main"],
    );
    assert!(listed.starts_with(&rewound), "{listed}");
}

#[test]
fn import_in_steps() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(30);
    for step in ["main~20", "main~10", "main"] {
        let refspec = format!("{step}:refs/heads/main");
        git(source.path(), &["push", "-q", &remote.url, &refspec]);
    }
    let cloned = clone(&remote, &[]);
    assert_eq!(log(&clone_path(&cloned)), log(source.path()));

    let narrow = Remote::start(
        Bounds {
            push_walk: 5,
            ..forge_harness::default_bounds()
        },
        HashKind::Sha1,
    );
    git(
        source.path(),
        &["push", "-q", &narrow.url, "main~20:refs/heads/main"],
    );
    let refused = git_fails(
        source.path(),
        &["push", &narrow.url, "main:refs/heads/main"],
    );
    assert!(refused.contains("push in smaller steps"), "{refused}");
}

#[test]
fn sha256_repository() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha256);
    let source = tempfile::tempdir().expect("a temp dir");
    git(
        source.path(),
        &["init", "-q", "-b", "main", "--object-format=sha256"],
    );
    commit_file(source.path(), "file-0", "0\n");
    git(source.path(), &["push", "-q", &remote.url, "main"]);

    let cloned = clone(&remote, &[]);
    let cloned = clone_path(&cloned);
    let cloned_head = head(&cloned);
    let forge::Reply::Log { page, .. } = remote.query(&forge::Query::Log {
        repo: REPO.into(),
        from: forge::Revision::Ref(b"refs/heads/main".to_vec()),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].oid, cloned_head);
    let forge::Reply::Tree { page, .. } = remote.query(&forge::Query::Tree {
        repo: REPO.into(),
        at: cloned_head.clone(),
        path: Vec::new(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].oid.len(), 64);
    let forge::Reply::Blob { blob, .. } = remote.query(&forge::Query::Blob {
        repo: REPO.into(),
        oid: page.items[0].oid.clone(),
        range: None,
    }) else {
        panic!();
    };
    assert_eq!(blob.bytes, b"0\n");
    assert_eq!(cloned_head.len(), 64);
    assert_eq!(cloned_head, head(source.path()));
    assert_eq!(
        git(&cloned, &["rev-parse", "--show-object-format"]).trim(),
        "sha256"
    );
}

#[test]
fn gzip_request_bodies() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let source = source_with(1);
    for index in 0..30 {
        let branch = format!("branch-{index}");
        git(source.path(), &["checkout", "-q", "-b", &branch, "main"]);
        commit_file(source.path(), &branch, "x\n");
    }
    git(source.path(), &["checkout", "-q", "main"]);
    git(source.path(), &["push", "-q", &remote.url, "--all"]);

    let cloned = clone(&remote, &["--single-branch", "-b", "main"]);
    let cloned = clone_path(&cloned);
    let fetched = git_in(
        &cloned,
        &[
            "fetch",
            "-q",
            "origin",
            "refs/heads/*:refs/remotes/origin/*",
        ],
        true,
    );
    let trace = String::from_utf8_lossy(&fetched.stderr);
    assert!(fetched.status.success(), "{trace}");
    assert!(trace.contains("Content-Encoding: gzip"), "{trace}");
    let branches = git(
        &cloned,
        &[
            "for-each-ref",
            "--format=%(refname) %(symref)",
            "refs/remotes/origin",
        ],
    );
    let mut expected: Vec<String> = (0..30)
        .map(|n| format!("refs/remotes/origin/branch-{n} "))
        .collect();
    expected.extend([
        "refs/remotes/origin/main ".into(),
        "refs/remotes/origin/HEAD refs/remotes/origin/main".into(),
    ]);
    expected.sort();
    assert_eq!(
        branches.lines().collect::<Vec<_>>(),
        expected,
        "every fetched branch plus the clone's symbolic HEAD"
    );

    let mut request = Vec::new();
    for line in ["command=ls-refs\n", "object-format=sha1\n"] {
        pkt(&mut request, line);
    }
    request.extend_from_slice(b"0001");
    pkt(&mut request, "ref-prefix refs/heads/\n");
    request.extend_from_slice(b"0000");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&request).expect("gzip");
    let gzipped = encoder.finish().expect("gzip");
    let mut response = ureq::post(format!("{}/git-upload-pack", remote.url))
        .header("Content-Type", "application/x-git-upload-pack-request")
        .header("Content-Encoding", "gzip")
        .header("Git-Protocol", "version=2")
        .send(&gzipped[..])
        .expect("the harness answers");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.body_mut().read_to_vec().expect("a body");
    let listing = String::from_utf8_lossy(&body);
    assert!(listing.contains("refs/heads/main\n"), "{listing}");
    assert!(listing.contains("refs/heads/branch-29\n"), "{listing}");
}

#[test]
fn upload_advertisement_requires_protocol_v2() {
    if skipped() {
        return;
    }
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let response = ureq::get(format!("{}/info/refs?service=git-upload-pack", remote.url))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .expect("the harness answers");
    assert_eq!(response.status().as_u16(), 400);
    let mut with_header = ureq::get(format!(
        "{}.git/info/refs?service=git-upload-pack",
        remote.url
    ))
    .header("Git-Protocol", "version=2")
    .call()
    .expect("the harness answers");
    assert_eq!(with_header.status().as_u16(), 200);
    assert_eq!(
        with_header
            .headers()
            .get("content-type")
            .map(|v| v.as_bytes()),
        Some(b"application/x-git-upload-pack-advertisement".as_slice())
    );
    let body = with_header.body_mut().read_to_vec().expect("a body");
    assert!(body.starts_with(b"001e# service=git-upload-pack\n0000000eversion 2\n"));
    let missing = ureq::get(format!(
        "{}/nope/info/refs?service=git-receive-pack",
        remote.base
    ))
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("the harness answers");
    assert_eq!(missing.status().as_u16(), 404);
}
