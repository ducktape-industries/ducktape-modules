// Merge tests: merge-base on a hand-built DAG, three-way line merge, and three-way tree merge clean and conflicting.

mod common;

use common::{blob, commit, file_tree, tree};
use gitcore::merge::{merge_base, merge_lines, merge_trees, ConflictKind, MergeOutcome, Merged};
use gitcore::{Error, Hash, MemoryObjects, Mode, Objects, Oid, Tree};

#[test]
fn merge_base_on_a_small_dag() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t = file_tree(&mut store, &[("f", "x\n")]);
    let root = commit(&mut store, t, &[], 100, "root");
    let a = commit(&mut store, t, &[root], 200, "a");
    let b = commit(&mut store, t, &[root], 300, "b");
    let a2 = commit(&mut store, t, &[a], 400, "a2");
    let m = commit(&mut store, t, &[a2, b], 500, "m");
    let b2 = commit(&mut store, t, &[b], 600, "b2");
    let m2 = commit(&mut store, t, &[m], 700, "m2");
    let lone = commit(&mut store, t, &[], 50, "lone");

    assert_eq!(merge_base(&store, &a2, &b, 100).unwrap(), Some(root));
    assert_eq!(merge_base(&store, &a, &a2, 100).unwrap(), Some(a));
    assert_eq!(merge_base(&store, &a2, &a, 100).unwrap(), Some(a));
    assert_eq!(merge_base(&store, &m, &m, 100).unwrap(), Some(m));
    assert_eq!(merge_base(&store, &m2, &b2, 100).unwrap(), Some(b));
    assert_eq!(merge_base(&store, &b2, &m2, 100).unwrap(), Some(b));
    assert_eq!(merge_base(&store, &lone, &m2, 100).unwrap(), None);
    assert_eq!(merge_base(&store, &m2, &b2, 2), Err(Error::CapReached));
}

#[test]
fn merge_base_with_two_candidates_picks_the_lowest_oid() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t = file_tree(&mut store, &[("f", "x\n")]);
    let root = commit(&mut store, t, &[], 100, "root");
    let x = commit(&mut store, t, &[root], 200, "x");
    let y = commit(&mut store, t, &[root], 200, "y");
    let left = commit(&mut store, t, &[x, y], 300, "left");
    let right = commit(&mut store, t, &[y, x], 300, "right");
    let expected = x.min(y);
    assert_eq!(
        merge_base(&store, &left, &right, 100).unwrap(),
        Some(expected)
    );
    assert_eq!(
        merge_base(&store, &right, &left, 100).unwrap(),
        Some(expected)
    );
}

#[test]
fn merge_base_with_clock_skew() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t = file_tree(&mut store, &[("f", "x\n")]);
    let root = commit(&mut store, t, &[], 900, "root dated late");
    let a = commit(&mut store, t, &[root], 100, "a");
    let b = commit(&mut store, t, &[root], 100, "b");
    assert_eq!(merge_base(&store, &a, &b, 100).unwrap(), Some(root));
}

fn clean(merged: Merged) -> String {
    let Merged::Clean(bytes) = merged else {
        panic!("expected clean merge");
    };
    String::from_utf8(bytes).unwrap()
}

#[test]
fn line_merge_applies_non_overlapping_changes() {
    let base = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n";
    let ours = b"1\n2 ours\n3\n4\n5\n6\n7\n8\n9\n";
    let theirs = b"1\n2\n3\n4\n5\n6\n7\n8 theirs\n9\nten\n";
    assert_eq!(
        clean(merge_lines(base, ours, theirs, 100).unwrap()),
        "1\n2 ours\n3\n4\n5\n6\n7\n8 theirs\n9\nten\n"
    );
    let ours_delete = b"1\n3\n4\n5\n6\n7\n8\n9\n";
    let theirs_insert_end = b"1\n2\n3\n4\n5\n6\n7\n8\n9\nten\n";
    assert_eq!(
        clean(merge_lines(base, ours_delete, theirs_insert_end, 100).unwrap()),
        "1\n3\n4\n5\n6\n7\n8\n9\nten\n"
    );
    let ours_top = b"zero\n1\n2\n3\n4\n5\n6\n7\n8\n9\n";
    assert_eq!(
        clean(merge_lines(base, ours_top, theirs_insert_end, 100).unwrap()),
        "zero\n1\n2\n3\n4\n5\n6\n7\n8\n9\nten\n"
    );
}

#[test]
fn line_merge_shortcuts_and_identical_changes() {
    let base = b"a\nb\n";
    assert_eq!(
        clean(merge_lines(base, base, b"a\nc\n", 100).unwrap()),
        "a\nc\n"
    );
    assert_eq!(
        clean(merge_lines(base, b"a\nc\n", base, 100).unwrap()),
        "a\nc\n"
    );
    assert_eq!(
        clean(merge_lines(base, b"a\nc\n", b"a\nc\n", 100).unwrap()),
        "a\nc\n"
    );
    let both_same_region = merge_lines(b"1\n2\n3\n", b"1\nX\n3\n", b"1\nX\n3\n4\n", 100).unwrap();
    assert_eq!(clean(both_same_region), "1\nX\n3\n4\n");
    assert_eq!(clean(merge_lines(b"", b"a\n", b"", 100).unwrap()), "a\n");
}

#[test]
fn line_merge_conflicts_on_overlap_and_adjacency() {
    let base = b"1\n2\n3\n";
    assert_eq!(
        merge_lines(base, b"1\nours\n3\n", b"1\ntheirs\n3\n", 100).unwrap(),
        Merged::Conflict
    );
    assert_eq!(
        merge_lines(base, b"1\nours\n3\n", b"1\n2\ntheirs\n", 100).unwrap(),
        Merged::Conflict
    );
    assert_eq!(
        merge_lines(b"", b"a\n", b"b\n", 100).unwrap(),
        Merged::Conflict
    );
    assert_eq!(
        merge_lines(base, b"1\n3\n", b"1\nx\n3\n", 100).unwrap(),
        Merged::Conflict
    );
    assert_eq!(
        merge_lines(base, b"x\n2\n3\n", b"y\n2\n3\n", 0),
        Err(Error::CapReached)
    );
}

fn names(store: &MemoryObjects, tree_id: &Oid) -> Vec<(String, Mode, Oid)> {
    let object = store.get(tree_id).unwrap().unwrap();
    Tree::parse(&object.body, Hash::Sha1)
        .unwrap()
        .entries
        .into_iter()
        .map(|e| (String::from_utf8(e.name).unwrap(), e.mode, e.id))
        .collect()
}

#[test]
fn tree_merge_clean_cases() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let shared = blob(&mut store, b"shared\n");
    let base_text = blob(&mut store, b"1\n2\n3\n4\n5\n");
    let our_text = blob(&mut store, b"one\n2\n3\n4\n5\n");
    let their_text = blob(&mut store, b"1\n2\n3\n4\nfive\n");
    let merged_text = blob(&mut MemoryObjects::new(Hash::Sha1), b"one\n2\n3\n4\nfive\n");
    let our_new = blob(&mut store, b"ours only\n");
    let their_new = blob(&mut store, b"theirs only\n");

    let base_sub = tree(&mut store, &[("text.txt", Mode::Regular, base_text)]);
    let our_sub = tree(&mut store, &[("text.txt", Mode::Regular, our_text)]);
    let their_sub = tree(&mut store, &[("text.txt", Mode::Regular, their_text)]);

    let base = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, base_sub),
            ("keep", Mode::Regular, shared),
            ("removed-by-us", Mode::Regular, shared),
            ("mode", Mode::Regular, shared),
        ],
    );
    let ours = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, our_sub),
            ("keep", Mode::Regular, shared),
            ("mode", Mode::Regular, shared),
            ("ours.txt", Mode::Regular, our_new),
        ],
    );
    let theirs = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, their_sub),
            ("keep", Mode::Regular, shared),
            ("removed-by-us", Mode::Regular, shared),
            ("mode", Mode::Executable, shared),
            ("theirs.txt", Mode::Regular, their_new),
        ],
    );
    let before = store.count();
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    let MergeOutcome::Clean(result) = outcome else {
        panic!("expected clean, got {outcome:?}");
    };
    let top = names(&store, &result);
    assert_eq!(top.len(), 5);
    assert_eq!(top[1], ("keep".into(), Mode::Regular, shared));
    assert_eq!(top[2], ("mode".into(), Mode::Executable, shared));
    assert_eq!(top[3], ("ours.txt".into(), Mode::Regular, our_new));
    assert_eq!(top[4], ("theirs.txt".into(), Mode::Regular, their_new));
    assert_eq!(top[0].0, "dir");
    let dir = names(&store, &top[0].2);
    assert_eq!(dir, vec![("text.txt".into(), Mode::Regular, merged_text)]);
    assert!(store.has(&merged_text).unwrap());
    assert_eq!(store.count(), before + 3);

    let again = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    assert_eq!(again, MergeOutcome::Clean(result));

    assert_eq!(
        merge_trees(&mut store, Some(&base), &ours, &base, 100).unwrap(),
        MergeOutcome::Clean(ours)
    );
    assert_eq!(
        merge_trees(&mut store, Some(&base), &base, &theirs, 100).unwrap(),
        MergeOutcome::Clean(theirs)
    );
    assert_eq!(
        merge_trees(&mut store, None, &ours, &ours, 100).unwrap(),
        MergeOutcome::Clean(ours)
    );
}

#[test]
fn tree_merge_reports_every_conflict_and_writes_nothing() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let a = blob(&mut store, b"a\n");
    let b = blob(&mut store, b"b\n");
    let c = blob(&mut store, b"c\n");
    let ok_base = blob(&mut store, b"1\n2\n3\n");
    let ok_ours = blob(&mut store, b"x\n2\n3\n");
    let ok_theirs = blob(&mut store, b"1\n2\ny\n");
    let sub = tree(&mut store, &[("inner", Mode::Regular, a)]);

    let base = tree(
        &mut store,
        &[
            ("content", Mode::Regular, a),
            ("modify-delete", Mode::Regular, a),
            ("mode", Mode::Regular, a),
            ("type", Mode::Regular, a),
            ("fine", Mode::Regular, ok_base),
        ],
    );
    let ours = tree(
        &mut store,
        &[
            ("content", Mode::Regular, b),
            ("mode", Mode::Executable, a),
            ("type", Mode::Directory, sub),
            ("add-add", Mode::Regular, b),
            ("fine", Mode::Regular, ok_ours),
            ("link", Mode::Symlink, a),
        ],
    );
    let theirs = tree(
        &mut store,
        &[
            ("content", Mode::Regular, c),
            ("modify-delete", Mode::Regular, b),
            ("mode", Mode::Symlink, a),
            ("type", Mode::Regular, b),
            ("add-add", Mode::Regular, c),
            ("fine", Mode::Regular, ok_theirs),
            ("link", Mode::Symlink, b),
        ],
    );
    let before = store.count();
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    let MergeOutcome::Conflicts(conflicts) = outcome else {
        panic!("expected conflicts");
    };
    let summary: Vec<(String, ConflictKind)> = conflicts
        .into_iter()
        .map(|c| (String::from_utf8(c.path).unwrap(), c.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("add-add".into(), ConflictKind::AddAdd),
            ("content".into(), ConflictKind::Content),
            ("link".into(), ConflictKind::AddAdd),
            ("mode".into(), ConflictKind::TypeConflict),
            ("modify-delete".into(), ConflictKind::ModifyDelete),
            ("type".into(), ConflictKind::TypeConflict),
        ]
    );
    assert_eq!(store.count(), before);
}

#[test]
fn tree_merge_mode_conflict_and_nested_paths() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let a = blob(&mut store, b"a\n");
    let base_sub = tree(&mut store, &[("f", Mode::Regular, a)]);
    let our_sub = tree(&mut store, &[("f", Mode::Executable, a)]);
    let their_sub = tree(&mut store, &[("f", Mode::Symlink, a)]);
    let base = tree(&mut store, &[("d", Mode::Directory, base_sub)]);
    let ours = tree(&mut store, &[("d", Mode::Directory, our_sub)]);
    let theirs = tree(&mut store, &[("d", Mode::Directory, their_sub)]);
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    let MergeOutcome::Conflicts(conflicts) = outcome else {
        panic!("expected conflicts");
    };
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, b"d/f");
    assert_eq!(conflicts[0].kind, ConflictKind::TypeConflict);

    let gitlink_a = Oid::from_hex(Hash::Sha1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let gitlink_b = Oid::from_hex(Hash::Sha1, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
    let gitlink_c = Oid::from_hex(Hash::Sha1, "cccccccccccccccccccccccccccccccccccccccc").unwrap();
    let base = tree(&mut store, &[("sub", Mode::Gitlink, gitlink_a)]);
    let ours = tree(&mut store, &[("sub", Mode::Gitlink, gitlink_b)]);
    let theirs = tree(&mut store, &[("sub", Mode::Gitlink, gitlink_c)]);
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    assert_eq!(
        outcome,
        MergeOutcome::Conflicts(vec![gitcore::merge::Conflict {
            path: b"sub".to_vec(),
            kind: ConflictKind::Submodule
        }])
    );

    let ours = tree(&mut store, &[("same-content", Mode::Regular, a)]);
    let theirs = tree(&mut store, &[("same-content", Mode::Executable, a)]);
    let outcome = merge_trees(&mut store, None, &ours, &theirs, 100).unwrap();
    assert_eq!(
        outcome,
        MergeOutcome::Conflicts(vec![gitcore::merge::Conflict {
            path: b"same-content".to_vec(),
            kind: ConflictKind::ModeConflict
        }])
    );
    let base = tree(&mut store, &[("same-content", Mode::Symlink, a)]);
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    assert!(
        matches!(outcome, MergeOutcome::Conflicts(ref c) if c[0].kind == ConflictKind::ModeConflict)
    );

    let removed_on_both = tree(&mut store, &[]);
    let base = tree(&mut store, &[("d", Mode::Directory, base_sub)]);
    let ours = tree(
        &mut store,
        &[("d", Mode::Directory, base_sub), ("x", Mode::Regular, a)],
    );
    let theirs = removed_on_both;
    let outcome = merge_trees(&mut store, Some(&base), &ours, &theirs, 100).unwrap();
    let MergeOutcome::Clean(result) = outcome else {
        panic!("expected clean");
    };
    assert_eq!(
        names(&store, &result),
        vec![("x".to_string(), Mode::Regular, a)]
    );
}
