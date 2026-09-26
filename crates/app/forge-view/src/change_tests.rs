//! The Change screens: the list, the detail header, the conversation, the
//! reviewer's Files tab, and the one operation a review becomes.
use super::{booted, change_screen, opened};
use crate::api::{ChatApi, SubmitForge};
use crate::state::ChangeTab;
use ducktape_view_guest::view::Submit;
use forge::{LineComment, Op, Side, Verdict};

#[test]
fn the_change_list_shows_the_plans_row_and_its_filters() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    for filter in crate::state::Filter::ALL {
        assert!(
            cx.find(&format!("forge-filter-{}", filter.slug()))
                .is_some(),
            "{} filter",
            filter.label()
        );
    }
    assert!(cx.has_text("Review this change"), "{:?}", cx.texts());
    assert!(cx.has_text("open"));
    assert!(cx.has_text("feature → main"));
    assert!(cx.has_text("Ada"), "the author key resolves");
    cx.simulate_click("forge-filter-closed");
    cx.run_until_parked();
    assert!(cx.has_text("Close this change"), "{:?}", cx.texts());
    cx.simulate_input("forge-changes-search", "nothing like this");
    cx.run_until_parked();
    assert!(cx.has_text("No changes here"));
    view.read(|forge| assert_eq!(forge.filter, crate::state::Filter::Closed));
}

#[test]
fn the_change_header_carries_its_endpoints_and_a_merge_the_program_allows() {
    let (cx, view) = change_screen("default", ChangeTab::Conversation);
    view.read(|forge| assert_eq!(forge.nav().change, Some(1)));
    assert!(cx.has_text("#1 Review this change"), "{:?}", cx.texts());
    assert!(cx.has_text("feature → main · Ada"));
    // `compare` says FastForward, so the merge is offered.
    let Some(ducktape_view_guest::wire::Node::Container(node)) = cx.find("forge-merge") else {
        panic!("the merge button is a native container");
    };
    assert!(node.interactivity.on_click.is_some());
    assert_ne!(node.interactivity.aria.disabled, Some(true));
    // The reader is not the author, so editing is closed to them.
    let Some(ducktape_view_guest::wire::Node::Container(edit)) = cx.find("forge-edit-change")
    else {
        panic!("the edit button stays visible")
    };
    assert_eq!(edit.interactivity.aria.disabled, Some(true));
}

#[test]
fn a_diverged_comparison_says_why_it_cannot_merge() {
    let (cx, _view) = change_screen("diverged", ChangeTab::Conversation);
    assert!(
        cx.has_text("The endpoints diverged: merge with git and push the result"),
        "{:?}",
        cx.texts()
    );
    let Some(ducktape_view_guest::wire::Node::Container(node)) = cx.find("forge-merge") else {
        panic!("the merge button stays visible");
    };
    assert_eq!(node.interactivity.aria.disabled, Some(true));
    assert!(node.interactivity.on_click.is_none());
}

#[test]
fn merging_submits_the_client_computed_fast_forward() {
    let (mut cx, _) = change_screen("default", ChangeTab::Conversation);
    cx.simulate_click("forge-merge");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<SubmitForge>()
            .iter()
            .any(|op| matches!(
                op,
                Op::Merge {
                    repo,
                    expected_into,
                    expected_from,
                    result,
                    change: Some(1),
                    ..
                } if repo == "project"
                    && expected_into == "ebfb8b62a50d6e5f7d10062af7cc5d15fd224e16"
                    && expected_from == "26607f522099476177a45a8058a93108fba5a84d"
                    && result == expected_from
            ))
    );
    assert!(cx.has_text("Merging this change"));
}

#[test]
fn an_operation_shows_its_submission_then_a_refusal_reverts_it_with_the_reason() {
    let (mut cx, _) = change_screen("default", ChangeTab::Conversation);
    cx.host()
        .refuse::<SubmitForge>("unauthorized", "this key may not close that change");
    cx.simulate_click("forge-close-change");
    cx.run_until_parked();
    assert!(
        cx.has_text("Refused: this key may not close that change"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Closing this change"), "the row stays in place");
}

#[test]
fn a_repository_filter_does_not_carry_into_its_change_search() {
    let (mut cx, _view) = booted("default");
    cx.simulate_input("forge-repos-search", "proj");
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(cx.has_text("Review this change"), "{:?}", cx.texts());
}

#[test]
fn a_merged_change_wears_its_state_and_offers_nothing_more() {
    let (cx, _view) = change_screen("merged", ChangeTab::Conversation);
    assert!(cx.has_text("merged"), "{:?}", cx.texts());
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("merged into main as ")),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.texts()
            .windows(2)
            .any(|w| w[0] == "Ada" && w[1].starts_with("merged into main as ")),
        "the merger is named from the record: {:?}",
        cx.texts()
    );
    assert!(
        cx.has_text("This change is no longer open"),
        "{:?}",
        cx.texts()
    );
    for button in ["forge-edit-change", "forge-close-change", "forge-merge"] {
        assert!(cx.find(button).is_none(), "{button} on an ended change");
    }
}

#[test]
fn a_closed_change_names_who_closed_it_and_offers_no_edit_or_close() {
    let (cx, _view) = change_screen("closed", ChangeTab::Conversation);
    assert!(
        cx.texts()
            .windows(2)
            .any(|w| w[0] == "Ada" && w[1] == "closed this change"),
        "the closer is named from the record: {:?}",
        cx.texts()
    );
    for button in ["forge-edit-change", "forge-close-change", "forge-merge"] {
        assert!(cx.find(button).is_none(), "{button} on an ended change");
    }
}

#[test]
fn the_conversation_is_the_hidden_chat_channel_and_the_forge_body() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Conversation);
    assert!(
        cx.find("forge-change-body-text").is_some(),
        "the body is markdown"
    );
    // forge's own lines read from its records, never from their text
    assert!(cx.has_text("Ada") && cx.has_text("opened this change"));
    assert!(!cx.has_text("raw forge text"), "{:?}", cx.texts());
    assert!(!cx.texts().iter().any(|text| text.contains("module:")));
    assert!(cx.has_text("Reading it now"));
    assert!(cx.has_text("Rae"), "a chat handle resolves to a name");
    // Three reviews across two pages of the Change reply.
    view.read(|forge| {
        let (_, _, _, reviews) = forge.change().expect("the change landed");
        assert_eq!(reviews.items.len(), 3, "the review cursor was followed");
    });
    assert!(cx.has_text("approved") && cx.has_text("requested changes"));
    assert!(cx.has_text("2 line comments"), "{:?}", cx.texts());
    // the change is still open: forge's last line has no ending to name yet
    assert!(
        !cx.texts()
            .iter()
            .any(|text| text.starts_with("merged into"))
    );
    cx.simulate_input("forge-reply", "looks right to me");
    cx.simulate_click("forge-reply-send");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<ChatApi>>()
            .iter()
            .any(|op| matches!(
                op,
                chat::Op::PostMessage { channel_id, .. } if channel_id == "forge:project:1"
            ))
    );
    view.read(|forge| assert!(forge.reply.is_empty()));
}

#[test]
fn a_review_pinned_before_a_push_reads_as_outdated() {
    let (cx, _view) = change_screen("outdated", ChangeTab::Conversation);
    assert!(cx.has_text("outdated"), "{:?}", cx.texts());
}

#[test]
fn the_files_tab_marks_comments_and_viewed_files_and_can_show_one() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    assert!(cx.has_text("src/lib.rs"), "{:?}", cx.texts());
    assert!(cx.has_text("+2 −1"));
    assert!(
        cx.find("forge-file-comments-src/lib.rs").is_some(),
        "published line comments mark the file"
    );
    cx.simulate_click("forge-viewed-src/lib.rs");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.viewed.contains("project#1:src/lib.rs")));
    assert!(cx.has_text("✓ viewed"));
    cx.simulate_click("forge-file-src/lib.rs");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/lib.rs".as_slice())
        )
    });
    cx.simulate_click("forge-files-all");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().diff_path.is_none()));
}

#[test]
fn the_diff_draws_typed_lines_and_believes_the_program_about_a_literal_plus_plus() {
    let (cx, _view) = change_screen("reviewed", ChangeTab::Files);
    assert!(cx.find("forge-diff").is_some());
    // A diff this small is drawn whole, so the screen carries every row of
    // the hunk and not only the one the list would have measured.
    assert!(
        cx.find("forge-diff-hunk-1").is_some(),
        "the hunk header is a row: {:?}",
        cx.texts()
    );
    for line in ["one", "keep", "old", "++ x", "end", "added"] {
        assert!(cx.has_text(line), "{line} is drawn: {:?}", cx.texts());
    }
    for gutter in [
        "forge-gutter-src/lib.rs-old-1",
        "forge-gutter-src/lib.rs-new-1",
        "forge-gutter-src/lib.rs-new-5",
    ] {
        assert!(cx.find(gutter).is_some(), "{gutter} carries its number");
    }
    assert!(
        cx.has_text("+") && cx.has_text("\u{2212}"),
        "an added and a removed marker"
    );
    let Some(ducktape_view_guest::wire::Node::Container(gutter)) =
        cx.find("forge-gutter-src/lib.rs-new-5")
    else {
        panic!("the gutter number is the comment button");
    };
    assert_eq!(
        gutter.interactivity.aria.label.as_deref(),
        Some("Comment on this line")
    );
    assert!(gutter.interactivity.on_click.is_some());
}

#[test]
fn the_gutter_of_a_drawn_line_is_the_comment_button() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    assert!(
        cx.has_text("Review pinned at 26607f52…a84d · 0 pending"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("forge-gutter-src/lib.rs-new-5");
    cx.run_until_parked();
    view.read(|forge| {
        let review = forge.review().expect("a review session");
        let open = review.open.as_ref().expect("an open anchor");
        assert_eq!(open.line, 5);
        assert!(open.new_side);
    });
    assert!(cx.has_text("src/lib.rs:5 (new)"));
}

#[test]
fn a_review_batches_every_anchor_into_exactly_one_operation() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();

    // Two anchors, one of them re-staged: one draft per anchor.
    for (path, new_side, line, body) in [
        (b"src/lib.rs".as_slice(), true, 3u64, "first thought"),
        (b"src/lib.rs".as_slice(), true, 3, "second thought"),
        (b"src/lib.rs".as_slice(), false, 1, "the old side too"),
    ] {
        view.update(&mut cx, |forge, _, cx| {
            forge.open_comment(path.to_vec(), new_side, line, cx)
        });
        cx.run_until_parked();
        cx.simulate_input("forge-comment-body", body);
        cx.simulate_click("forge-comment-save");
        cx.run_until_parked();
    }
    view.read(|forge| {
        let review = forge.review().expect("a review session");
        assert_eq!(review.comments.len(), 2, "re-staging replaces its anchor");
    });

    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_input("forge-review-body", "one batch, one op");
    cx.simulate_click("forge-verdict-request-changes");
    cx.run_until_parked();

    let submitted = cx.host().requests::<SubmitForge>();
    let reviews: Vec<&Op> = submitted
        .iter()
        .filter(|op| matches!(op, Op::ReviewSubmit { .. }))
        .collect();
    assert_eq!(reviews.len(), 1, "one review is one operation");
    let Op::ReviewSubmit { repo, n, review } = reviews[0] else {
        unreachable!()
    };
    assert_eq!((repo.as_str(), *n), ("project", 1));
    assert_eq!(review.verdict, Verdict::RequestChanges);
    assert_eq!(review.body, "one batch, one op");
    assert_eq!(
        review.commit_oid, "26607f522099476177a45a8058a93108fba5a84d",
        "the pin is the head the reader read"
    );
    assert_eq!(
        review.base_oid.as_deref(),
        Some("ebfb8b62a50d6e5f7d10062af7cc5d15fd224e16"),
        "the old side addresses the merge base"
    );
    let mut anchors = review.comments.clone();
    anchors.sort_by_key(|comment| (comment.side, comment.line));
    assert_eq!(
        anchors,
        vec![
            LineComment {
                path: b"src/lib.rs".to_vec(),
                side: Side::Old,
                line: 1,
                body: "the old side too".into(),
            },
            LineComment {
                path: b"src/lib.rs".to_vec(),
                side: Side::New,
                line: 3,
                body: "second thought".into(),
            },
        ]
    );
    assert!(cx.has_text("Submitting this review") || cx.has_text("Waiting for the next block"));
}

#[test]
fn a_refused_review_keeps_every_draft() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    view.update(&mut cx, |forge, _, cx| {
        forge.open_comment(b"src/lib.rs".to_vec(), true, 3, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("forge-comment-body", "keep me");
    cx.simulate_click("forge-comment-save");
    cx.run_until_parked();
    cx.host()
        .refuse::<SubmitForge>("capacity", "this operation exceeds its bound");
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_click("forge-verdict-approve");
    cx.run_until_parked();
    assert!(
        cx.has_text("Refused: this operation exceeds its bound"),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| {
        let review = forge.review().expect("the session survives a refusal");
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].body, "keep me");
    });
}

#[test]
fn an_empty_comment_verdict_is_refused_before_it_reaches_the_program() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    let before = cx.host().requests::<SubmitForge>().len();
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_click("forge-verdict-comment");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<SubmitForge>().len(), before);
    assert!(
        cx.has_text("A comment review needs a body or a line comment"),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| assert!(forge.review().is_some()));
}

#[test]
fn the_docked_panels_show_one_at_a_time_and_jump_to_a_line() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-dock-merge-status");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().dock, Some(crate::state::Dock::MergeStatus)));
    assert!(cx.has_text("fast-forward"), "{:?}", cx.texts());
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("Approvals are advisory"))
    );
    cx.simulate_click("forge-dock-comments");
    cx.run_until_parked();
    assert!(
        cx.has_text("Rae · src/lib.rs:2 — Context is commentable"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("forge-comment-1-src/lib.rs-2");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/lib.rs".as_slice())
        )
    });
    cx.simulate_click("forge-dock-comments");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().dock.is_none()));
}
