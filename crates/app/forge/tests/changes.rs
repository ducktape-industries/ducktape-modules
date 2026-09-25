// Every op on the story's repository: what it does, and what it refuses (the wrong signer, the wrong state, a duplicate, an unbounded input).

mod common;
use common::story::*;
use common::*;
use forge::{
    ChangeFilter, ChangeState, LineComment, MAX_KEY_BYTES, OpReply, ReviewDraft, Revision, Side,
    Verdict,
};

const REVIEWER: &[u8] = b"reviewer";

fn story() -> (Rig, Story) {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    (rig, story)
}

/// The op signed by `who`; the rig signs as the owner again afterwards.
fn signed(rig: &mut Rig, who: &[u8], op: &Op) -> Result<Vec<u8>, abi::Refusal> {
    rig.actor = who.to_vec();
    let result = rig.execute(op);
    rig.actor = TESTER.to_vec();
    result
}

fn refused(result: Result<Vec<u8>, abi::Refusal>) -> String {
    result.expect_err("refused").reason
}

fn reply(rig: &Rig, query: &Query) -> Reply {
    abi::decode(&rig.query(query).unwrap()).unwrap()
}

fn record(rig: &Rig, n: u64) -> forge::Change {
    let Reply::Change { change, .. } = reply(rig, &change(n)) else {
        panic!()
    };
    change
}

fn involving(rig: &Rig, key: &[u8]) -> Vec<u64> {
    let query = Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter {
            involves: Some(key.to_vec()),
            ..ChangeFilter::default()
        },
        page: Page::first(128),
    };
    let Reply::Changes { page, .. } = reply(rig, &query) else {
        panic!()
    };
    page.items.iter().map(|summary| summary.n).collect()
}

fn opened(rig: &mut Rig, story: &Story) -> u64 {
    let output = rig.execute(&story.open("Feature")).unwrap();
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!()
    };
    n
}

fn edit_reviewers(n: u64, reviewers: Vec<Vec<u8>>) -> Op {
    Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: None,
        body: None,
        reviewers: Some(reviewers),
    }
}

// ------------------------------------------------------------ repository

#[test]
fn configure_is_the_owners_and_takes_a_ref_for_head() {
    let (mut rig, _) = story();
    let settings = |head: &[u8]| Op::Configure {
        repo: REPO.into(),
        settings: Settings {
            head: head.to_vec(),
            allow_force: true,
            allow_delete: false,
        },
    };
    rig.execute(&settings(b"refs/heads/feature")).unwrap();
    let Reply::Repo { repo, .. } = reply(
        &rig,
        &Query::Repo {
            repo: REPO.into(),
            page: Page::first(8),
        },
    ) else {
        panic!()
    };
    assert_eq!(repo.repo.settings.head, b"refs/heads/feature");
    assert!(repo.repo.settings.allow_force);

    let stranger = signed(&mut rig, STRANGER, &settings(b"refs/heads/main"));
    assert_eq!(refused(stranger), reason::UNAUTHORIZED);
    assert_eq!(
        refused(rig.execute(&settings(b"main"))),
        reason::INVALID_INPUT
    );
    let endless = [b"refs/heads/".as_slice(), &[b'x'; 5000]].concat();
    assert_eq!(
        refused(rig.execute(&settings(&endless))),
        reason::INVALID_INPUT
    );
}

#[test]
fn grant_and_revoke_are_the_owners_and_name_a_bounded_key() {
    let (mut rig, _) = story();
    let grant = |key: &[u8]| Op::Grant {
        repo: REPO.into(),
        key: key.to_vec(),
    };
    let revoke = |key: &[u8]| Op::Revoke {
        repo: REPO.into(),
        key: key.to_vec(),
    };
    let writers = |rig: &Rig| {
        let Reply::Repo { writers, .. } = reply(
            rig,
            &Query::Repo {
                repo: REPO.into(),
                page: Page::first(8),
            },
        ) else {
            panic!()
        };
        writers.items
    };

    rig.execute(&grant(WRITER)).unwrap();
    assert_eq!(writers(&rig), [WRITER.to_vec()]);
    assert_eq!(
        refused(signed(&mut rig, WRITER, &revoke(WRITER))),
        reason::UNAUTHORIZED
    );
    rig.execute(&revoke(WRITER)).unwrap();
    assert!(
        writers(&rig).is_empty(),
        "a revoked key leaves no row behind"
    );

    assert_eq!(
        refused(signed(&mut rig, STRANGER, &grant(STRANGER))),
        reason::UNAUTHORIZED
    );
    assert_eq!(refused(rig.execute(&grant(b""))), reason::INVALID_INPUT);
    assert_eq!(
        refused(rig.execute(&grant(&[7; MAX_KEY_BYTES + 1]))),
        reason::CAPACITY
    );
}

// ---------------------------------------------------------------- changes

#[test]
fn open_numbers_changes_and_refuses_what_cannot_merge() {
    let (mut rig, story) = story();
    assert_eq!(opened(&mut rig, &story), 1);
    assert_eq!(opened(&mut rig, &story), 2, "numbers are never reused");
    let feature = record(&rig, 1);
    assert_eq!(feature.state, ChangeState::Open);
    assert_eq!(feature.author, TESTER);
    assert_eq!(feature.channel, "forge:project:1");

    let open = |from: Revision, into: &[u8], title: &str, reviewers: Vec<Vec<u8>>| Op::ChangeOpen {
        repo: REPO.into(),
        from,
        into: into.to_vec(),
        title: title.into(),
        body: String::new(),
        reviewers,
    };
    let main = b"refs/heads/main";
    let same = open(reference("main"), main, "Nothing", vec![]);
    assert_eq!(refused(rig.execute(&same)), reason::WRONG_STATE);
    let tag = open(reference("feature"), b"refs/tags/v1", "Into a tag", vec![]);
    assert_eq!(refused(rig.execute(&tag)), reason::INVALID_INPUT);
    let blank = open(reference("feature"), main, "  ", vec![]);
    assert_eq!(refused(rig.execute(&blank)), reason::INVALID_INPUT);
    let long = open(reference("feature"), main, &"t".repeat(257), vec![]);
    assert_eq!(refused(rig.execute(&long)), reason::INVALID_INPUT);
    let twice = open(
        reference("feature"),
        main,
        "Twice",
        vec![REVIEWER.to_vec(), REVIEWER.to_vec()],
    );
    assert_eq!(refused(rig.execute(&twice)), reason::INVALID_INPUT);
    let gone = open(reference("nowhere"), main, "Gone", vec![]);
    assert_eq!(refused(rig.execute(&gone)), reason::NOT_FOUND);
}

#[test]
fn edit_is_the_authors_and_drops_the_reviewers_it_unasks() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    assert_eq!(involving(&rig, REVIEWER), [n]);

    let retitle = Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: Some("Renamed".into()),
        body: None,
        reviewers: None,
    };
    rig.execute(&retitle).unwrap();
    assert_eq!(record(&rig, n).title, "Renamed");
    assert_eq!(
        record(&rig, n).body,
        "The author's body.",
        "None keeps a field"
    );
    assert_eq!(
        refused(signed(&mut rig, REVIEWER, &retitle)),
        reason::UNAUTHORIZED
    );

    // Asking a fleet of keys and unasking them leaves nothing behind.
    for round in 0..3u8 {
        let fleet = (0..64).map(|i| vec![round, i]).collect();
        rig.execute(&edit_reviewers(n, fleet)).unwrap();
    }
    rig.execute(&edit_reviewers(n, vec![])).unwrap();
    assert!(involving(&rig, REVIEWER).is_empty());
    assert!(involving(&rig, &[0, 0]).is_empty());
    assert_eq!(involving(&rig, TESTER), [n], "the author stays involved");

    let fleet = (0..65).map(|i| vec![i]).collect();
    assert_eq!(
        refused(rig.execute(&edit_reviewers(n, fleet))),
        reason::CAPACITY
    );
    let missing = edit_reviewers(n + 1, vec![]);
    assert_eq!(refused(rig.execute(&missing)), reason::NOT_FOUND);
}

#[test]
fn a_reviewer_who_reviewed_stays_involved_when_unasked() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let approve = review(&story.feature, &story.root, Verdict::Approve);
    signed(&mut rig, REVIEWER, &approve).unwrap();
    rig.execute(&edit_reviewers(n, vec![])).unwrap();
    assert_eq!(involving(&rig, REVIEWER), [n]);
}

#[test]
fn close_is_the_authors_or_a_writers_and_happens_once() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let close = Op::ChangeClose {
        repo: REPO.into(),
        n,
    };
    assert_eq!(
        refused(signed(&mut rig, STRANGER, &close)),
        reason::UNAUTHORIZED
    );
    rig.execute(&close).unwrap();
    let closed = record(&rig, n);
    assert_eq!(closed.state, ChangeState::Closed);
    assert_eq!(closed.closed_by.as_ref(), Some(&rig.actor));
    assert_eq!(closed.merged_by, None);
    assert_eq!(refused(rig.execute(&close)), reason::WRONG_STATE);

    let second = opened(&mut rig, &story);
    rig.execute(&Op::Grant {
        repo: REPO.into(),
        key: WRITER.to_vec(),
    })
    .unwrap();
    let by_writer = Op::ChangeClose {
        repo: REPO.into(),
        n: second,
    };
    signed(&mut rig, WRITER, &by_writer).unwrap();
    let closed = record(&rig, second);
    assert_eq!(closed.state, ChangeState::Closed);
    assert_eq!(closed.closed_by.as_deref(), Some(WRITER));
}

#[test]
fn a_review_counts_once_and_its_anchors_are_checked() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let output = signed(
        &mut rig,
        REVIEWER,
        &review(&story.feature, &story.root, Verdict::RequestChanges),
    )
    .unwrap();
    assert!(matches!(
        abi::decode(&output).unwrap(),
        OpReply::Review { id: 1, .. }
    ));
    let change = record(&rig, n);
    assert_eq!(change.review_count, 1);
    assert_eq!(change.comment_count, 2);
    assert_eq!(change.verdicts.request_changes, 1);

    let draft = |base: Option<&str>, verdict, comments: Vec<(Side, u64, &str)>| Op::ReviewSubmit {
        repo: REPO.into(),
        n,
        review: ReviewDraft {
            commit_oid: story.feature.clone(),
            base_oid: base.map(str::to_owned),
            verdict,
            body: String::new(),
            comments: comments
                .into_iter()
                .map(|(side, line, body)| LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side,
                    line,
                    body: body.into(),
                })
                .collect(),
        },
    };
    let root = Some(story.root.as_str());
    let twice = draft(
        root,
        Verdict::Comment,
        vec![(Side::New, 2, "a"), (Side::New, 2, "b")],
    );
    assert_eq!(refused(rig.execute(&twice)), reason::INVALID_INPUT);
    let baseless = draft(None, Verdict::Comment, vec![(Side::Old, 2, "old")]);
    assert_eq!(refused(rig.execute(&baseless)), reason::INVALID_INPUT);
    let silent = draft(root, Verdict::Comment, vec![]);
    assert_eq!(refused(rig.execute(&silent)), reason::INVALID_INPUT);
    let line_zero = draft(root, Verdict::Approve, vec![(Side::New, 0, "zero")]);
    assert_eq!(refused(rig.execute(&line_zero)), reason::INVALID_INPUT);
    let mut escape = draft(root, Verdict::Comment, vec![(Side::New, 1, "up")]);
    if let Op::ReviewSubmit { review, .. } = &mut escape {
        review.comments[0].path = b"../etc/passwd".to_vec();
    }
    assert_eq!(refused(rig.execute(&escape)), reason::INVALID_INPUT);
    let flood = draft(
        root,
        Verdict::Comment,
        (1..=65).map(|line| (Side::New, line, "x")).collect(),
    );
    assert_eq!(refused(rig.execute(&flood)), reason::CAPACITY);
    assert_eq!(
        record(&rig, n).review_count,
        1,
        "a refused review counts nothing"
    );
}

#[test]
fn a_merge_lands_its_change_once_and_only_over_the_heads_it_read() {
    let (mut rig, mut story) = story();
    let n = opened(&mut rig, &story);
    let merge_over = |expected_into: &str, from: &str| Op::Merge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: reference("feature"),
        expected_into: expected_into.into(),
        expected_from: from.into(),
        result: from.into(),
        change: Some(n),
    };
    let merge = |expected_into: &str| merge_over(expected_into, &story.feature);
    assert_eq!(
        refused(signed(&mut rig, STRANGER, &merge(&story.root))),
        reason::UNAUTHORIZED
    );
    assert_eq!(refused(rig.execute(&merge(&story.clean))), reason::STALE);

    let output = rig.execute(&merge(&story.root)).unwrap();
    assert!(matches!(
        abi::decode(&output).unwrap(),
        OpReply::Merged {
            change: Some(1),
            ..
        }
    ));
    let change = record(&rig, n);
    assert_eq!(change.state, ChangeState::Merged);
    assert_eq!(change.merge_oid.as_deref(), Some(story.feature.as_str()));
    assert_eq!(change.merged_by.as_ref(), Some(&rig.actor));
    assert_eq!(change.closed_by, None);
    assert_eq!(
        refs_of(&rig.sandbox, REPO)["refs/heads/main"],
        story.feature
    );

    // The feature moves on; its change is merged already.
    let main = refs_of(&rig.sandbox, REPO)["refs/heads/main"].clone();
    let tip = story.push_follow_up(&mut rig, "later.txt", b"later\n");
    let again = merge_over(&main, &tip);
    assert_eq!(refused(rig.execute(&again)), reason::WRONG_STATE);
}
