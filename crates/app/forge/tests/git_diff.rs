// Diff tests: tree diff on crafted trees, Myers hunks, and unified-diff goldens taken from GNU diff -u.

#[path = "gitcommon/mod.rs"]
mod common;

use common::{blob, tree};
use forge::git::diff::{ChangeKind, Hunk, Op, lines, tree_diff, unified};
use forge::git::{Error, Hash, MemoryObjects, Mode};

#[test]
fn tree_diff_recurses_sorts_and_skips_identical_subtrees() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let a = blob(&mut store, b"a\n");
    let b = blob(&mut store, b"b\n");
    let c = blob(&mut store, b"c\n");
    let same_sub = tree(&mut store, &[("deep", Mode::Regular, a)]);
    let old_sub = tree(
        &mut store,
        &[("x", Mode::Regular, a), ("y", Mode::Regular, b)],
    );
    let new_sub = tree(
        &mut store,
        &[("x", Mode::Regular, c), ("z", Mode::Regular, b)],
    );
    let old = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, old_sub),
            ("exec", Mode::Regular, a),
            ("gone", Mode::Regular, b),
            ("same", Mode::Directory, same_sub),
            ("file-then-dir", Mode::Regular, a),
        ],
    );
    let new = tree(
        &mut store,
        &[
            ("dir", Mode::Directory, new_sub),
            ("exec", Mode::Executable, a),
            ("new", Mode::Regular, c),
            ("same", Mode::Directory, same_sub),
            ("file-then-dir", Mode::Directory, same_sub),
        ],
    );
    let changes = tree_diff(&store, Some(&old), Some(&new)).unwrap();
    let summary: Vec<(String, &ChangeKind)> = changes
        .iter()
        .map(|change| {
            (
                String::from_utf8_lossy(&change.path).into_owned(),
                &change.kind,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "dir/x".into(),
                &ChangeKind::Modified {
                    old_mode: Mode::Regular,
                    new_mode: Mode::Regular,
                    old: a,
                    new: c
                }
            ),
            (
                "dir/y".into(),
                &ChangeKind::Deleted {
                    mode: Mode::Regular,
                    id: b
                }
            ),
            (
                "dir/z".into(),
                &ChangeKind::Added {
                    mode: Mode::Regular,
                    id: b
                }
            ),
            (
                "exec".into(),
                &ChangeKind::ModeChanged {
                    old_mode: Mode::Regular,
                    new_mode: Mode::Executable,
                    id: a
                }
            ),
            (
                "file-then-dir".into(),
                &ChangeKind::Deleted {
                    mode: Mode::Regular,
                    id: a
                }
            ),
            (
                "file-then-dir/deep".into(),
                &ChangeKind::Added {
                    mode: Mode::Regular,
                    id: a
                }
            ),
            (
                "gone".into(),
                &ChangeKind::Deleted {
                    mode: Mode::Regular,
                    id: b
                }
            ),
            (
                "new".into(),
                &ChangeKind::Added {
                    mode: Mode::Regular,
                    id: c
                }
            ),
        ]
    );
    assert!(
        tree_diff(&store, Some(&old), Some(&old))
            .unwrap()
            .is_empty()
    );
    let from_nothing = tree_diff(&store, None, Some(&same_sub)).unwrap();
    assert_eq!(from_nothing.len(), 1);
    assert_eq!(from_nothing[0].path, b"deep");
    let to_nothing = tree_diff(&store, Some(&same_sub), None).unwrap();
    assert!(matches!(to_nothing[0].kind, ChangeKind::Deleted { .. }));
    assert!(tree_diff(&store, None, None).unwrap().is_empty());
}

#[test]
fn line_hunks_are_runs_with_ranges() {
    let hunks = lines(b"a\nb\nc\n", b"a\nc\nd\n", 10).unwrap();
    assert_eq!(
        hunks,
        vec![
            Hunk {
                op: Op::Equal,
                old: 0..1,
                new: 0..1
            },
            Hunk {
                op: Op::Delete,
                old: 1..2,
                new: 1..1
            },
            Hunk {
                op: Op::Equal,
                old: 2..3,
                new: 1..2
            },
            Hunk {
                op: Op::Insert,
                old: 3..3,
                new: 2..3
            },
        ]
    );
    assert_eq!(lines(b"", b"", 0).unwrap(), Vec::<Hunk>::new());
    assert_eq!(lines(b"x\n", b"y\n", 1), Err(Error::CapReached));
    let same = lines(b"same\n", b"same\n", 0).unwrap();
    assert_eq!(
        same,
        vec![Hunk {
            op: Op::Equal,
            old: 0..1,
            new: 0..1
        }]
    );
}

#[test]
fn unified_matches_gnu_diff() {
    let old = b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";
    let new = b"one\n2\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\nthirteen";
    let expected = "@@ -1,5 +1,5 @@\n one\n-two\n+2\n three\n four\n five\n@@ -10,3 +10,4 @@\n ten\n eleven\n twelve\n+thirteen\n\\ No newline at end of file\n";
    assert_eq!(
        String::from_utf8(unified(old, new, 3, 100).unwrap()).unwrap(),
        expected
    );

    let text = unified(b"a\nb\nc\n", b"a\nc\nd\n", 3, 100).unwrap();
    assert_eq!(
        String::from_utf8(text).unwrap(),
        "@@ -1,3 +1,3 @@\n a\n-b\n c\n+d\n"
    );

    let text = unified(b"x\n", b"", 3, 100).unwrap();
    assert_eq!(String::from_utf8(text).unwrap(), "@@ -1 +0,0 @@\n-x\n");

    let text = unified(b"a\nb\n", b"a\nb\nc\n", 0, 100).unwrap();
    assert_eq!(String::from_utf8(text).unwrap(), "@@ -2,0 +3 @@\n+c\n");

    let text = unified(b"keep\n", b"new\nkeep\n", 0, 100).unwrap();
    assert_eq!(String::from_utf8(text).unwrap(), "@@ -0,0 +1 @@\n+new\n");

    assert!(unified(b"same\n", b"same\n", 3, 100).unwrap().is_empty());
    assert_eq!(unified(b"a\n", b"b\n", 3, 0), Err(Error::CapReached));
}
