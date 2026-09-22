mod common;
use common::story::*;
use common::*;
use forge::*;

fn detail(remote: &Remote, n: u64) -> (Change, Vec<Review>, Option<String>) {
    let Reply::Change {
        change,
        reviews,
        source_head,
        ..
    } = remote.query(&change(n))
    else {
        panic!();
    };
    (change, reviews.items, source_head)
}
fn refusal(remote: &Remote, op: &Op, reason: &str) {
    remote.advance();
    let before = remote.snapshot();
    let Failure::Refused(error) = remote.execute(op).unwrap_err() else {
        panic!("typed op refusal");
    };
    assert_eq!(error.reason, reason);
    let after = remote.snapshot();
    assert_eq!(before.state_snapshot(), after.state_snapshot());
    assert_eq!(before.blob_count(), after.blob_count());
    assert!(after.pending().is_empty());
}
fn judgment(remote: &Remote) -> Page<Judgment> {
    let Reply::Judgment { page, .. } = remote.query(&Query::Judgment {
        key: b"reviewer".to_vec(),
        cursor: None,
        limit: 128,
    }) else {
        panic!();
    };
    page
}

#[test]
fn changes_batch_reviews_chat_replies_head_moves_and_advisory_merge_survive_restart() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    remote.actor(b"author");
    let opened: OpReply = abi::decode(&remote.execute(&story.open("A change")).unwrap()).unwrap();
    assert!(matches!(opened, OpReply::Change { n: 1, .. }));
    let snap = remote.snapshot();
    assert_eq!(snap.pending().len(), 2);
    let (record, _, _) = detail(&remote, 1);
    assert_eq!(record.body, "The author's body.");
    assert_eq!(record.author, b"author");
    assert_eq!(record.channel, "forge:project:1");
    let room = remote
        .rt
        .block_on(remote.harness.chat_query(chat::ChatViewQuery::Channel {
            channel_id: record.channel.clone(),
        }))
        .unwrap();
    assert!(
        matches!(room, chat::ChatViewReply::Channel(None)),
        "emits are queued until the next block"
    );
    remote.advance();
    let room = remote
        .rt
        .block_on(remote.harness.chat_query(chat::ChatViewQuery::Channel {
            channel_id: record.channel.clone(),
        }))
        .unwrap();
    let chat::ChatViewReply::Channel(Some(room)) = room else {
        panic!();
    };
    assert_eq!(room.channel.owner, "module:forge");
    assert_eq!(room.head_seq, 1);
    assert!(judgment(&remote).items[0].requested);
    remote
        .execute(&Op::ChangeEdit {
            repo: REPO.into(),
            n: 1,
            title: Some("Edited title".into()),
            body: Some("Edited body".into()),
            reviewers: None,
        })
        .unwrap();
    assert_eq!(detail(&remote, 1).0.body, "Edited body");
    remote.actor(b"reviewer");
    refusal(
        &remote,
        &Op::ChangeEdit {
            repo: REPO.into(),
            n: 1,
            title: Some("Not mine".into()),
            body: None,
            reviewers: None,
        },
        abi::reason::UNAUTHORIZED,
    );
    let submitted: OpReply = abi::decode(
        &remote
            .execute(&review(&story, Verdict::RequestChanges))
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(submitted, OpReply::Review { n: 1, id: 1, .. }));
    let (record, reviews, head) = detail(&remote, 1);
    assert_eq!((record.review_count, record.comment_count), (1, 2));
    assert_eq!(reviews[0].draft.comments[0].side, Side::Old);
    assert_eq!(reviews[0].draft.comments[1].line, 2);
    assert_eq!(reviews[0].draft.base_oid, Some(story.root.clone()));
    assert_eq!(Some(&reviews[0].draft.commit_oid), head.as_ref());
    remote.advance();
    assert!(judgment(&remote).items.is_empty());
    let root = remote
        .rt
        .block_on(remote.harness.chat_query(chat::ChatViewQuery::MessageById {
            message_id: reviews[0].message_id.clone(),
        }))
        .unwrap();
    let chat::ChatViewReply::Message(Some(root)) = root else {
        panic!();
    };
    assert_eq!(root.author, "module:forge");
    assert!(root.text.contains("RequestChanges"));
    remote
        .rt
        .block_on(remote.harness.chat_execute(
            chat::Party::Key(b"author".to_vec()),
            chat::ChatMsg::PostMessage {
                channel_id: record.channel,
                message_id: "reply-one".into(),
                blocks: vec![chat::Block::paragraph("Fixed the old-side concern")],
                thread: Some(root.seq),
            },
        ))
        .unwrap();
    let attention = judgment(&remote);
    assert_eq!(attention.items.len(), 1);
    assert!(!attention.items[0].requested);
    assert_eq!(
        attention.items[0].replies.as_ref().unwrap().root_seq,
        root.seq
    );
    remote.actor(b"tester");
    commit_file(story.source.path(), "follow-up.txt", "follow-up\n");
    let new_head = head_of(&story);
    git(story.source.path(), &["push", "-q", &remote.url, "feature"]);
    let (_, reviews, head) = detail(&remote, 1);
    assert_ne!(
        Some(&reviews[0].draft.commit_oid),
        head.as_ref(),
        "the view can mark the pinned review outdated"
    );
    assert!(judgment(&remote).items[0].requested);
    let merge = Op::Merge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: reference("feature"),
        expected_into: story.root.clone(),
        expected_from: new_head.clone(),
        result: new_head.clone(),
        change: Some(1),
    };
    let mut stale = merge.clone();
    if let Op::Merge { expected_from, .. } = &mut stale {
        *expected_from = story.feature.clone();
    }
    refusal(&remote, &stale, abi::reason::STALE);
    remote.actor(b"stranger");
    refusal(&remote, &merge, abi::reason::UNAUTHORIZED);
    remote.actor(b"tester");
    let result: OpReply = abi::decode(&remote.execute(&merge).unwrap()).unwrap();
    assert!(matches!(
        result,
        OpReply::Merged {
            change: Some(1),
            ..
        }
    ));
    let (record, _, _) = detail(&remote, 1);
    assert_eq!(record.state, ChangeState::Merged);
    assert_eq!(record.merge_oid, Some(new_head));
    assert_eq!(
        record.verdicts.request_changes, 1,
        "an advisory negative review never blocks merge"
    );
    refusal(
        &remote,
        &Op::ChangeClose {
            repo: REPO.into(),
            n: 1,
        },
        abi::reason::WRONG_STATE,
    );
    assert!(judgment(&remote).items.is_empty());
    let snapshot = remote.snapshot();
    let restarted = Harness::from_snapshot(wasm(), snapshot).unwrap();
    let actual = remote.rt.block_on(restarted.query(&change(1))).unwrap();
    let expected = remote
        .rt
        .block_on(remote.harness.query(&change(1)))
        .unwrap();
    assert_eq!(actual, expected);
    let room = remote
        .rt
        .block_on(restarted.chat_query(chat::ChatViewQuery::Channel {
            channel_id: "forge:project:1".into(),
        }))
        .unwrap();
    let chat::ChatViewReply::Channel(Some(room)) = room else {
        panic!();
    };
    assert_eq!(room.head_seq, 4);
}
fn head_of(story: &Story) -> String {
    head(story.source.path())
}

#[test]
fn limits_duplicate_anchors_authorization_and_filtered_pages_are_not_partial_writes() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    remote.execute(&story.open("First")).unwrap();
    remote.execute(&story.open("Second")).unwrap();
    remote
        .execute(&Op::ChangeClose {
            repo: REPO.into(),
            n: 2,
        })
        .unwrap();
    let filter = ChangeFilter {
        state: Some(ChangeState::Closed),
        ..Default::default()
    };
    let Reply::Changes { page, .. } = remote.query(&Query::Changes {
        repo: REPO.into(),
        filter: filter.clone(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert!(page.items.is_empty());
    assert!(page.next.is_some());
    let Reply::Changes { page, .. } = remote.query(&Query::Changes {
        repo: REPO.into(),
        filter,
        cursor: page.next,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items[0].n, 2);
    assert!(page.next.is_none());
    let Op::ReviewSubmit { review: draft, .. } = review(&story, Verdict::Comment) else {
        panic!();
    };
    for bad in [
        ReviewDraft {
            comments: vec![draft.comments[0].clone(); MAX_REVIEW_COMMENTS + 1],
            ..draft.clone()
        },
        ReviewDraft {
            comments: vec![draft.comments[0].clone(); 2],
            ..draft.clone()
        },
        ReviewDraft {
            comments: vec![LineComment {
                line: 0,
                ..draft.comments[0].clone()
            }],
            ..draft.clone()
        },
        ReviewDraft {
            comments: vec![LineComment {
                path: b"../bad".to_vec(),
                ..draft.comments[0].clone()
            }],
            ..draft.clone()
        },
        ReviewDraft {
            base_oid: None,
            ..draft.clone()
        },
        ReviewDraft {
            commit_oid: "0".repeat(40),
            ..draft.clone()
        },
    ] {
        let expected = if bad.comments.len() > MAX_REVIEW_COMMENTS {
            abi::reason::CAPACITY
        } else {
            abi::reason::INVALID_INPUT
        };
        refusal(
            &remote,
            &Op::ReviewSubmit {
                repo: REPO.into(),
                n: 1,
                review: bad,
            },
            expected,
        );
    }
    let mut cap = draft.clone();
    cap.comments = (1..=MAX_REVIEW_COMMENTS as u64)
        .map(|line| LineComment {
            line,
            ..draft.comments[0].clone()
        })
        .collect();
    remote
        .execute(&Op::ReviewSubmit {
            repo: REPO.into(),
            n: 1,
            review: cap,
        })
        .unwrap();
    remote.execute(&review(&story, Verdict::Approve)).unwrap();
    let Reply::Change { reviews, .. } = remote.query(&Query::Change {
        repo: REPO.into(),
        n: 1,
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(reviews.items[0].draft.comments.len(), MAX_REVIEW_COMMENTS);
    assert!(reviews.next.is_some());
    let Reply::Change { reviews, .. } = remote.query(&Query::Change {
        repo: REPO.into(),
        n: 1,
        cursor: reviews.next,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(reviews.items[0].id, 2);
    assert!(reviews.next.is_none());
    refusal(
        &remote,
        &Op::ChangeEdit {
            repo: REPO.into(),
            n: 1,
            title: None,
            body: Some("x".repeat(65 << 10)),
            reviewers: None,
        },
        abi::reason::CAPACITY,
    );
    let expected = detail(&remote, 1).0;
    assert_eq!(expected.body, "The author's body.");
    assert_eq!(expected.review_count, 2);
    let Reply::Changes { page, .. } = remote.query(&Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter {
            author: Some(b"nobody".to_vec()),
            ..Default::default()
        },
        cursor: None,
        limit: 128,
    }) else {
        panic!();
    };
    assert!(page.items.is_empty());
}

#[test]
fn merge_compares_both_heads_without_reading_locally_missing_objects() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    remote.execute(&story.open("CAS")).unwrap();
    remote.advance();
    let mut snapshot = remote.snapshot();
    let mut digest = [0u8; 20];
    for (i, b) in digest.iter_mut().enumerate() {
        *b = u8::from_str_radix(&story.feature[i * 2..i * 2 + 2], 16).unwrap();
    }
    snapshot.remove_blob(&abi::BlobId::Sha1(digest));
    let sparse = Harness::from_snapshot(wasm(), snapshot).unwrap();
    let merge = Op::Merge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: reference("feature"),
        expected_into: story.root,
        expected_from: story.feature.clone(),
        result: story.feature,
        change: Some(1),
    };
    remote
        .rt
        .block_on(sparse.execute(&merge))
        .expect("CAS is independent of local objects");
    let bytes = remote.rt.block_on(sparse.query(&change(1))).unwrap();
    assert!(matches!(
        abi::decode::<Reply>(&bytes).unwrap(),
        Reply::Change {
            change: Change {
                state: ChangeState::Merged,
                ..
            },
            ..
        }
    ));
}

#[test]
fn judgment_includes_chat_authored_threads_and_replies_to_older_reviews() {
    let remote = Remote::start(forge_harness::default_bounds(), HashKind::Sha1);
    let story = Story::pushed(&remote);
    remote.execute(&story.open("Discussion")).unwrap();
    remote.advance();
    let post = |party, id: &str, thread| {
        remote
            .rt
            .block_on(remote.harness.chat_execute(
                party,
                chat::ChatMsg::PostMessage {
                    channel_id: "forge:project:1".into(),
                    message_id: id.into(),
                    blocks: vec![chat::Block::paragraph("Discuss")],
                    thread,
                },
            ))
            .unwrap()
    };
    post(chat::Party::Key(b"talker".to_vec()), "question", None);
    post(chat::Party::Key(b"tester".to_vec()), "answer", Some(2));
    let Reply::Judgment { page, .. } = remote.query(&Query::Judgment {
        key: b"talker".to_vec(),
        cursor: None,
        limit: 1,
    }) else {
        panic!();
    };
    assert_eq!(page.items.len(), 1);
    let attention = page.items[0].replies.as_ref().unwrap();
    assert_eq!(
        (
            attention.review,
            attention.root_seq,
            attention.last_reply_seq
        ),
        (None, 2, 3)
    );
    remote.actor(b"reviewer");
    remote.execute(&review(&story, Verdict::Comment)).unwrap();
    remote.execute(&review(&story, Verdict::Approve)).unwrap();
    remote.advance();
    let (_, rows, _) = detail(&remote, 1);
    let chat::ChatViewReply::Message(Some(root)) = remote
        .rt
        .block_on(remote.harness.chat_query(chat::ChatViewQuery::MessageById {
            message_id: rows[0].message_id.clone(),
        }))
        .unwrap()
    else {
        panic!();
    };
    post(
        chat::Party::Key(b"tester".to_vec()),
        "older-review-reply",
        Some(root.seq),
    );
    assert_eq!(
        judgment(&remote).items[0].replies.as_ref().unwrap().review,
        Some(1)
    );
}
