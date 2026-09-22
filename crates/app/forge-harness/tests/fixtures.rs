//! Replay bytes come directly from forge.wasm's Respond/Output, never hand-built replies.
mod common;
use common::story::*;
use common::*;
use forge::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn save(name: &str, bytes: &[u8], mut sidecar: Value) {
    sidecar["bytes"] = json!(bytes.len());
    sidecar["sha256"] = json!(abi::hex(&Sha256::digest(bytes)));
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let json = serde_json::to_string(&sidecar).unwrap() + "\n";
    let binary = folder.join(format!("{name}.bin"));
    let metadata = folder.join(format!("{name}.json"));
    if std::env::var_os("FORGE_REGENERATE_FIXTURES").as_deref() == Some(std::ffi::OsStr::new("1")) {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&binary, bytes).unwrap();
        std::fs::write(&metadata, json).unwrap();
    } else {
        assert_eq!(
            std::fs::read(&binary).unwrap_or_else(|_| panic!("regenerate {}", binary.display())),
            bytes,
            "{name} actual program bytes"
        );
        assert_eq!(
            std::fs::read_to_string(&metadata).unwrap(),
            json,
            "{name} sidecar"
        );
    }
}
fn capture(remote: &Remote, name: &str, q: Query) -> Reply {
    let bytes = remote.rt.block_on(remote.harness.query(&q)).unwrap();
    let reply: Reply = abi::decode(&bytes).unwrap();
    assert_eq!(abi::encode(&reply), bytes);
    save(
        name,
        &bytes,
        json!({"wire_type":"forge::Reply","codec":"borsh","request":q,
        "request_borsh_hex":abi::hex(&abi::encode(&q)),"reply":reply}),
    );
    reply
}
fn output(remote: &Remote, name: &str, op: Op) -> OpReply {
    let bytes = remote.execute(&op).unwrap();
    let reply: OpReply = abi::decode(&bytes).unwrap();
    assert_eq!(abi::encode(&reply), bytes);
    save(
        name,
        &bytes,
        json!({"wire_type":"forge::OpReply","codec":"borsh","op":op,
        "request_borsh_hex":abi::hex(&abi::encode(&op)),"reply":reply}),
    );
    reply
}
fn git_reply(remote: &Remote, name: &str, q: Query) {
    let bytes = remote.rt.block_on(remote.harness.query(&q)).unwrap();
    assert!(!bytes.is_empty());
    save(
        name,
        &bytes,
        json!({"wire_type":"git smart HTTP","codec":"git","request":q,
        "request_borsh_hex":abi::hex(&abi::encode(&q)),"height":remote.snapshot().height()}),
    );
}

#[test]
fn replay_fixtures_are_the_programs_real_bytes() {
    assert!(!skipped(), "fixture generation requires git");
    let bounds = Bounds {
        blob_bytes: 64,
        ..forge_harness::default_bounds()
    };
    let remote = Remote::start(bounds, HashKind::Sha1);
    let empty = remote
        .rt
        .block_on(Harness::new(wasm(), bounds, b"tester".to_vec()))
        .unwrap();
    let q = Query::Repos {
        cursor: None,
        limit: 2,
    };
    let bytes = remote.rt.block_on(empty.query(&q)).unwrap();
    let reply: Reply = abi::decode(&bytes).unwrap();
    save(
        "repos-empty",
        &bytes,
        json!({"wire_type":"forge::Reply","codec":"borsh","request":q,
        "request_borsh_hex":abi::hex(&abi::encode(&q)),"reply":reply}),
    );
    capture(
        &remote,
        "refs-empty",
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 2,
        },
    );
    capture(
        &remote,
        "log-unborn",
        Query::Log {
            repo: REPO.into(),
            from: reference("main"),
            cursor: None,
            limit: 2,
        },
    );
    capture(&remote, "changes-empty", changes());
    capture(
        &remote,
        "judgment-empty",
        Query::Judgment {
            key: b"reviewer".to_vec(),
            cursor: None,
            limit: 2,
        },
    );
    let story = Story::pushed(&remote);
    capture(
        &remote,
        "repos",
        Query::Repos {
            cursor: None,
            limit: 2,
        },
    );
    remote
        .execute(&Op::Grant {
            repo: REPO.into(),
            key: b"writer".to_vec(),
        })
        .unwrap();
    capture(
        &remote,
        "repo",
        Query::Repo {
            repo: REPO.into(),
            cursor: None,
            limit: 2,
        },
    );
    capture(
        &remote,
        "refs",
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 2,
        },
    );
    let Reply::Log { page, .. } = capture(
        &remote,
        "log",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            cursor: None,
            limit: 1,
        },
    ) else {
        panic!();
    };
    capture(
        &remote,
        "log-next",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            cursor: page.next,
            limit: 1,
        },
    );
    capture(
        &remote,
        "tree",
        Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: vec![],
            cursor: None,
            limit: 2,
        },
    );
    capture(
        &remote,
        "tree-directory",
        Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: b"src".to_vec(),
            cursor: None,
            limit: 2,
        },
    );
    for (name, path) in [
        ("blob", "src/lib.rs"),
        ("blob-binary", "image.bin"),
        ("blob-oversize", "large.txt"),
        ("blob-empty", "empty.txt"),
    ] {
        capture(
            &remote,
            name,
            Query::Blob {
                repo: REPO.into(),
                oid: story.oid(&format!("feature:{path}")),
                range: None,
            },
        );
    }
    capture(
        &remote,
        "blob-range",
        Query::Blob {
            repo: REPO.into(),
            oid: story.oid("feature:src/lib.rs"),
            range: Some(ByteRange { offset: 4, len: 6 }),
        },
    );
    let Reply::Diff { page, .. } = capture(
        &remote,
        "diff",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.feature.clone(),
            path: None,
            cursor: None,
            limit: 2,
        },
    ) else {
        panic!();
    };
    capture(
        &remote,
        "diff-next",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.feature.clone(),
            path: None,
            cursor: page.next,
            limit: 2,
        },
    );
    for (name, path) in [
        ("diff-text", "src/lib.rs"),
        ("diff-mode", "mode.sh"),
        ("diff-binary", "image.bin"),
        ("diff-oversize", "large.txt"),
        ("diff-gitlink", "vendor"),
        ("diff-deleted", "gone.txt"),
        ("diff-added", "new.txt"),
    ] {
        capture(
            &remote,
            name,
            Query::Diff {
                repo: REPO.into(),
                base: Some(story.root.clone()),
                head: story.feature.clone(),
                path: Some(path.as_bytes().to_vec()),
                cursor: None,
                limit: 2,
            },
        );
    }
    capture(
        &remote,
        "diff-root",
        Query::Diff {
            repo: REPO.into(),
            base: None,
            head: story.root.clone(),
            path: Some(b"README.md".to_vec()),
            cursor: None,
            limit: 2,
        },
    );
    capture(
        &remote,
        "diff-empty",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.root.clone(),
            path: None,
            cursor: None,
            limit: 2,
        },
    );
    for (name, from, into) in [
        ("compare", "feature", "main"),
        ("compare-up-to-date", "main", "feature"),
        ("compare-clean", "feature", "clean"),
        ("compare-conflicts", "feature", "conflict"),
        ("compare-unrelated", "feature", "unrelated"),
    ] {
        capture(&remote, name, compare(from, into));
    }
    capture(&remote, "activity", Query::Activity { repo: REPO.into() });
    git_reply(
        &remote,
        "advertise-receive",
        Query::Advertise {
            repo: REPO.into(),
            service: Service::ReceivePack,
        },
    );
    git_reply(
        &remote,
        "advertise-upload",
        Query::Advertise {
            repo: REPO.into(),
            service: Service::UploadPack,
        },
    );
    let mut request = Vec::new();
    pkt(&mut request, "command=ls-refs\n");
    request.extend_from_slice(b"0001");
    pkt(&mut request, "symrefs\n");
    request.extend_from_slice(b"0000");
    git_reply(
        &remote,
        "upload-refs",
        Query::Upload {
            repo: REPO.into(),
            request,
        },
    );
    capture(
        &remote,
        "refused-object-not-held",
        Query::Blob {
            repo: REPO.into(),
            oid: "f".repeat(40),
            range: None,
        },
    );
    capture(
        &remote,
        "refused-not-found",
        Query::Activity {
            repo: "absent".into(),
        },
    );
    capture(
        &remote,
        "refused-invalid-input",
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 0,
        },
    );
    let Reply::Refs { page, .. } = capture(
        &remote,
        "refs-before-update",
        Query::Refs {
            repo: REPO.into(),
            cursor: None,
            limit: 1,
        },
    ) else {
        panic!();
    };
    remote.advance();
    capture(
        &remote,
        "refused-stale",
        Query::Refs {
            repo: REPO.into(),
            cursor: page.next,
            limit: 1,
        },
    );
    output(&remote, "op-change-open", story.open("Review this change"));
    capture(&remote, "change", change(1));
    capture(&remote, "changes", changes());
    capture(
        &remote,
        "judgment",
        Query::Judgment {
            key: b"reviewer".to_vec(),
            cursor: None,
            limit: 2,
        },
    );
    output(
        &remote,
        "op-change-edit",
        Op::ChangeEdit {
            repo: REPO.into(),
            n: 1,
            title: Some("Review this edited change".into()),
            body: Some("An edited body on the forge record.".into()),
            reviewers: None,
        },
    );
    remote.actor(b"reviewer");
    for (name, verdict) in [
        ("op-review-comment", Verdict::Comment),
        ("op-review-request-changes", Verdict::RequestChanges),
        ("op-review-approve", Verdict::Approve),
    ] {
        output(&remote, name, review(&story, verdict));
    }
    remote.advance();
    let Reply::Change { reviews, .. } = capture(
        &remote,
        "change-reviewed",
        Query::Change {
            repo: REPO.into(),
            n: 1,
            cursor: None,
            limit: 2,
        },
    ) else {
        panic!();
    };
    let Reply::Change { reviews, .. } = capture(
        &remote,
        "change-reviews-next",
        Query::Change {
            repo: REPO.into(),
            n: 1,
            cursor: reviews.next,
            limit: 2,
        },
    ) else {
        panic!();
    };
    let chat::ChatViewReply::Message(Some(root)) = remote
        .rt
        .block_on(remote.harness.chat_query(chat::ChatViewQuery::MessageById {
            message_id: reviews.items[0].message_id.clone(),
        }))
        .unwrap()
    else {
        panic!();
    };
    remote
        .rt
        .block_on(remote.harness.chat_execute(
            chat::Party::Key(b"tester".to_vec()),
            chat::ChatMsg::PostMessage {
                channel_id: "forge:project:1".into(),
                message_id: "fixture-reply".into(),
                blocks: vec![chat::Block::paragraph("A reply about the anchored line")],
                thread: Some(root.seq),
            },
        ))
        .unwrap();
    capture(
        &remote,
        "judgment-replies",
        Query::Judgment {
            key: b"reviewer".to_vec(),
            cursor: None,
            limit: 2,
        },
    );
    remote.actor(b"tester");
    commit_file(story.source.path(), "follow-up.txt", "follow-up\n");
    git(story.source.path(), &["push", "-q", &remote.url, "feature"]);
    let tip = head(story.source.path());
    capture(&remote, "change-outdated", change(1));
    capture(
        &remote,
        "judgment-head-moved",
        Query::Judgment {
            key: b"reviewer".to_vec(),
            cursor: None,
            limit: 2,
        },
    );
    output(
        &remote,
        "op-change-open-second",
        story.open("Close this change"),
    );
    output(
        &remote,
        "op-change-close",
        Op::ChangeClose {
            repo: REPO.into(),
            n: 2,
        },
    );
    capture(&remote, "change-closed", change(2));
    capture(
        &remote,
        "changes-filtered",
        Query::Changes {
            repo: REPO.into(),
            filter: ChangeFilter {
                state: Some(ChangeState::Closed),
                ..Default::default()
            },
            cursor: None,
            limit: 2,
        },
    );
    output(
        &remote,
        "op-merge",
        Op::Merge {
            repo: REPO.into(),
            into: b"refs/heads/main".to_vec(),
            from: reference("feature"),
            expected_into: story.root.clone(),
            expected_from: tip.clone(),
            result: tip,
            change: Some(1),
        },
    );
    capture(&remote, "change-merged", change(1));
    let mut conversation = story.open("Conversation attention");
    if let Op::ChangeOpen { from, .. } = &mut conversation {
        *from = reference("clean");
    }
    remote.execute(&conversation).unwrap();
    remote.advance();
    for (key, id, thread) in [
        (b"talker".as_slice(), "conversation-root", None),
        (b"tester".as_slice(), "conversation-reply", Some(2)),
    ] {
        remote
            .rt
            .block_on(remote.harness.chat_execute(
                chat::Party::Key(key.to_vec()),
                chat::ChatMsg::PostMessage {
                    channel_id: "forge:project:3".into(),
                    message_id: id.into(),
                    blocks: vec![chat::Block::paragraph("Conversation")],
                    thread,
                },
            ))
            .unwrap();
    }
    capture(
        &remote,
        "judgment-conversation",
        Query::Judgment {
            key: b"talker".to_vec(),
            cursor: None,
            limit: 128,
        },
    );
    let narrow = Remote::start(
        Bounds {
            log_walk: 1,
            ..bounds
        },
        HashKind::Sha1,
    );
    Story::pushed(&narrow);
    capture(
        &narrow,
        "refused-capacity",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            cursor: None,
            limit: 1,
        },
    );
}
