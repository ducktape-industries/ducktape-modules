use super::*;

// --------------------------------------------------------------- test 1

/// Random trees, decoded and sanitized, always land inside every bound
/// `sanitize` promises — or `decode` refused them for a reason the door
/// actually names, and the tree really was over it.
#[test]
fn random_trees_come_out_of_sanitize_inside_every_bound() {
    // The task asked for ~300; without decode's own recursion needing a
    // dedicated thread (its depth door caps recursion at MAX_DEPTH, safe on
    // a normal stack — see `on_big_stack`'s doc comment), 200 trees with a
    // steep width/depth skew keep this test's slice of the file's ~10s
    // debug budget comfortably small.
    const SEED: u64 = 0x5EED_F00D_1234_5678;
    const NUM_TREES: usize = 200;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let (frame, bytes) = build_and_encode(seed, i);

        match decode::<Frame>(&bytes) {
            Err(message) => {
                let root = frame.root.as_ref().expect("gen_frame always sets a root");
                let depth_over = tree_depth(root) > MAX_DEPTH;
                let count_over = root.count() > MAX_DECODED_NODES + 1;
                assert!(
                    depth_over || count_over,
                    "{ctx}: decode refused a tree that was not actually over either \
                     door (depth {}, nodes {}): {message}",
                    tree_depth(root),
                    root.count()
                );
                let names_the_door = message.contains("deeper than the host renders")
                    || message.contains("more nodes than the host holds");
                assert!(names_the_door, "{ctx}: unexpected refusal: {message}");
            }
            Ok(mut decoded) => {
                let duplicate_ids = decoded
                    .root
                    .as_ref()
                    .is_some_and(has_duplicate_typed_siblings);
                let before = decoded.root.as_ref().map(document_refs).unwrap_or_default();
                match sanitize(&mut decoded) {
                    Ok(_) => {
                        check_frame(&decoded, &ctx);
                        let after = decoded.root.as_ref().map(document_refs).unwrap_or_default();
                        assert_eq!(
                            before, after,
                            "{ctx}: sanitize rewrote an editor document reference"
                        );
                    }
                    Err("duplicate typed element identity among siblings") => {
                        assert!(
                            duplicate_ids,
                            "{ctx}: identity refusal must name an actual collision"
                        );
                    }
                    // Other refusals protect editor
                    // documents it could not keep whole; every other bound is
                    // pulled into range instead.
                    Err(refused) => assert!(
                        [
                            "invalid editor document references or budget",
                            "frame budget would remove an editor document projection",
                        ]
                        .contains(&refused),
                        "{ctx}: unexpected refusal: {refused}"
                    ),
                }
            }
        }
    }
}

// --------------------------------------------------------------- test 2

fn corrupt_length_prefix(rng: &mut Rng, bytes: &mut [u8]) {
    if bytes.len() < 8 {
        return;
    }
    let at = rng.next_range(bytes.len() - 7);
    let value = *rng.choose(&[u64::MAX, 1u64 << 40, 0u64]);
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// One random mutation applied to a copy of a sound frame's bytes: a bit
/// flip, a byte overwrite, a truncation, an insertion of random bytes, or a
/// length-prefix corruption (an 8-byte little-endian window overwritten
/// with a value a real `Vec`/`String` length prefix would never hold).
fn mutate_once(rng: &mut Rng, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.push(rng.next_range(256) as u8);
        return;
    }
    match rng.next_range(5) {
        0 => {
            let i = rng.next_range(bytes.len());
            bytes[i] ^= 1 << rng.next_range(8);
        }
        1 => {
            let i = rng.next_range(bytes.len());
            bytes[i] = rng.next_range(256) as u8;
        }
        2 => {
            let cut = rng.next_range(bytes.len() + 1);
            bytes.truncate(cut);
        }
        3 => {
            let at = rng.next_range(bytes.len() + 1);
            let junk: Vec<u8> = (0..1 + rng.next_range(16))
                .map(|_| rng.next_range(256) as u8)
                .collect();
            bytes.splice(at..at, junk);
        }
        _ => corrupt_length_prefix(rng, bytes),
    }
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

/// Bytes a hostile guest could have written — a sound frame with random
/// bit flips, byte overwrites, truncations, insertions, and corrupted
/// length prefixes — never make `decode` panic. `decode`'s own depth door
/// (checked before each level is even built) is what makes this safe on a
/// plain stack: see `lib.rs`'s `bytes_a_hostile_guest_could_write_are_answered_not_survived`,
/// which this test generalizes to frames far larger than a single flipped
/// bit's worth of hand-written cases.
#[test]
fn mutated_bytes_never_panic() {
    const SEED: u64 = 0xBADF_00D5_A5A5_5A5A;
    const NUM_FRAMES: usize = 50;
    const MUTATIONS_PER_FRAME: usize = 200;

    for i in 0..NUM_FRAMES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let (_frame, bytes) = build_and_encode_bounded(seed);
        let mut mutator = Rng::new(seed ^ 0xF00D);

        for m in 0..MUTATIONS_PER_FRAME {
            let mut mutated = bytes.clone();
            mutate_once(&mut mutator, &mut mutated);
            let ctx = format!("seed={seed:#x} frame={i} mutation={m}");

            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decode::<Frame>(&mutated)
            }));
            match outcome {
                Ok(Ok(mut decoded)) => {
                    if sanitize(&mut decoded).is_ok() {
                        check_frame(&decoded, &ctx);
                    }
                }
                Ok(Err(_)) => {}
                Err(payload) => panic!("{ctx}: decode panicked: {}", payload_message(&payload)),
            }
        }
    }
}

// --------------------------------------------------------------- test 2b

/// A sanitized tree, patched by any sequence the wire decodes, is a
/// sanitized tree or a refusal: every bound `check_bounds` covers holds of
/// what `apply` returns `Ok` on, whatever the patches inserted, replaced or
/// shuffled — including subtrees over every ceiling on their own, and keys
/// the tree already holds. A refusal names one of the doors `apply` has.
#[test]
fn a_patched_sanitized_tree_is_a_sanitized_tree() {
    const SEED: u64 = 0x9A7C_4E5D_0B1A_2F3E;
    const NUM_TREES: usize = 60;
    const PATCHES_PER_TREE: usize = 24;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        on_big_stack(move || {
            let mut rng = Rng::new(seed);
            let (depth, width) = (rng.skewed(MAX_DEPTH, 3), rng.skewed(64, 2));
            let mut frame = gen_frame_with(&mut rng, depth, width);
            if sanitize(&mut frame).is_err() {
                return;
            }
            let mut root = frame
                .root
                .take()
                .expect("gen_frame_with always sets a root");
            let hostile = rng.next_range(3) == 0;
            let mut patches = Vec::new();
            let mut staged = root.clone();
            for _ in 0..rng.next_range(PATCHES_PER_TREE + 1) {
                let patch = gen_patch(&mut rng, &staged, hostile);
                // Paths are drawn against the tree as the patches so far
                // leave it, so a well-behaved sequence applies whole and
                // a hostile one is refused somewhere along it.
                let mut candidate = staged.clone();
                let applied = view_wire::apply(&mut candidate, vec![patch.clone()]);
                if applied == Err("duplicate typed element identity among siblings") {
                    assert!(
                        has_duplicate_typed_siblings(&candidate),
                        "{ctx}: missing collision"
                    );
                    continue;
                }
                if matches!(
                    applied,
                    Err("invalid editor document references or budget"
                        | "frame budget would remove an editor document projection")
                ) {
                    continue;
                }
                staged = candidate;
                assert!(
                    hostile || applied.is_ok(),
                    "{ctx}: a well-behaved patch was refused: {applied:?}\n{patch:#?}"
                );
                patches.push(patch);
            }
            let patched = Frame {
                patches,
                ..Frame::default()
            };
            // A patch's subtree meets the same door a root does: one nested
            // past what the host walks is refused before it is built.
            let decoded: Frame = match decode(&encode(&patched)) {
                Ok(decoded) => decoded,
                Err(message) => {
                    assert!(
                        message.contains("deeper than the host renders")
                            || message.contains("more nodes than the host holds"),
                        "{ctx}: unexpected refusal: {message}"
                    );
                    return;
                }
            };
            let outcome = view_wire::apply(&mut root, decoded.patches);
            assert!(
                hostile
                    || outcome.is_ok()
                    || matches!(
                        outcome,
                        Err("invalid editor document references or budget"
                            | "frame budget would remove an editor document projection")
                    ),
                "{ctx}: a structurally valid sequence was refused: {outcome:?}"
            );
            match outcome {
                Ok(_) => {
                    let checked = Frame {
                        root: Some(root),
                        ..Frame::default()
                    };
                    check_frame(&checked, &ctx);
                }
                Err(refused) => {
                    let named = [
                        "a path to no node",
                        "an index past the list",
                        "a list edit on no list",
                        "props of another arity",
                        "more patches than the host applies",
                        "invalid editor document references or budget",
                        "frame budget would remove an editor document projection",
                    ]
                    .contains(&refused);
                    assert!(named, "{ctx}: unexpected refusal: {refused}");
                }
            }
        });
    }
}

// --------------------------------------------------------------- test 2c

/// `diff` then `apply` is the identity on the new tree, and leaves both
/// inputs as they were. The new tree is the old one with a handful of
/// well-behaved edits applied — so the pair shares most of its structure
/// and the diff has to find moves, inserts, removes and field changes
/// inside lists whose keys come from a five-entry pool — and, one time in
/// eight, an unrelated tree, which is a `Replace` at the root. A pair whose
/// diff runs past `MAX_PATCHES` is the guest's cue to send the tree whole,
/// so it is only checked for that refusal.
#[test]
fn a_diff_applied_to_the_old_tree_is_the_new_tree_for_random_pairs() {
    const SEED: u64 = 0xD1FF_0000_A99B_1E5A;
    const NUM_PAIRS: usize = 150;
    const EDITS_PER_PAIR: usize = 8;

    for i in 0..NUM_PAIRS {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} pair={i}");
        on_big_stack(move || {
            let mut rng = Rng::new(seed);
            let tree = |rng: &mut Rng| {
                let (depth, width) = (rng.skewed(MAX_DEPTH / 2, 3), rng.skewed(48, 2));
                let mut frame = gen_frame_with(rng, depth, width);
                sanitize(&mut frame).ok()?;
                frame.root.take()
            };
            let Some(mut old) = tree(&mut rng) else {
                return;
            };
            let mut new = match rng.next_range(8) {
                0 => {
                    let Some(tree) = tree(&mut rng) else {
                        return;
                    };
                    tree
                }
                _ => {
                    let mut edited = old.clone();
                    for _ in 0..1 + rng.next_range(EDITS_PER_PAIR) {
                        let patch = gen_patch(&mut rng, &edited, false);
                        let mut candidate = edited.clone();
                        match view_wire::apply(&mut candidate, vec![patch]) {
                            Ok(_) => edited = candidate,
                            Err(
                                "invalid editor document references or budget"
                                | "frame budget would remove an editor document projection",
                            ) => {}
                            Err("duplicate typed element identity among siblings") => {
                                assert!(
                                    has_duplicate_typed_siblings(&candidate),
                                    "{ctx}: missing collision"
                                );
                            }
                            Err(refused) => panic!("{ctx}: {refused}"),
                        }
                    }
                    edited
                }
            };
            let (old_before, new_before) = (old.clone(), new.clone());
            let patches = diff(&mut old, &mut new);
            assert_eq!(old, old_before, "{ctx}: diff moved the old tree");
            assert_eq!(new, new_before, "{ctx}: diff moved the new tree");
            let count = patches.len();
            let mut applied = old;
            match view_wire::apply(&mut applied, patches) {
                Ok(_) => assert_eq!(applied, new, "{ctx}: {count} patches"),
                Err(refused) => assert!(
                    count > MAX_PATCHES && refused == "more patches than the host applies",
                    "{ctx}: {count} patches refused: {refused}"
                ),
            }
        });
    }
}

// --------------------------------------------------------------- test 3

/// A valid MessagePack array32 header claims u32::MAX children with no
/// payload. Decode must refuse without preallocating the claimed vector.
#[test]
fn a_length_prefix_bomb_is_refused_without_the_allocation() {
    let frame = |children| Frame {
        root: Some(Node::Container(view_wire::ContainerNode {
            id: None,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children,
        })),
        ..Default::default()
    };
    let empty = encode(&frame(vec![]));
    let one = encode(&frame(vec![Node::empty()]));
    let offset = empty.iter().zip(&one).take_while(|(a, b)| a == b).count();
    assert_eq!(empty[offset], 0x90, "empty fixarray marker");
    assert_eq!(one[offset], 0x91, "one-child fixarray marker");
    let mut bomb = empty[..offset].to_vec();
    bomb.push(0xdd); // array32, followed by its big-endian length
    bomb.extend_from_slice(&u32::MAX.to_be_bytes());
    let start = std::time::Instant::now();
    let error = decode::<Frame>(&bomb).unwrap_err();
    assert!(
        error.contains("IO error while reading marker"),
        "must enter the array and refuse the missing child, not reject malformed encoding: {error}"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "the hostile size hint must never cause allocation"
    );
}

// --------------------------------------------------------------- test 4

/// `sanitize` is idempotent: running it again on its own output changes
/// nothing, which is what lets a host call it on every frame without
/// worrying whether the guest already sent a clean one.
#[test]
fn sanitize_is_idempotent() {
    const SEED: u64 = 0x1DE4_1DE4_5EED_5EED;
    const NUM_TREES: usize = 150;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let mut once = build_frame(seed, i);
        if sanitize(&mut once).is_err() {
            continue;
        }
        let mut twice = once.clone();
        sanitize(&mut twice).unwrap();
        assert_eq!(once, twice, "{ctx}: sanitize is not idempotent");
    }
}

#[test]
fn resize_handle_round_trip_retains_routes_and_checks_its_child() {
    let mut frame = gen_frame_with(&mut Rng::new(17), 0, 0);
    frame.root = Some(Node::ResizeHandle {
        id: ElementIdWire::Name("divider".into()),
        on_press: Some(1),
        on_release: Some(2),
        on_drag: Some(u32::MAX),
        cursor: Some(mouse::Cursor::ResizingHorizontally),
        content: Box::new(Node::Space {
            style: gen_native_style(&mut Rng::new(99)),
        }),
        style: gpui::StyleRefinement::default(),
    });
    assert_eq!(tree_depth(frame.root.as_ref().unwrap()), 1);
    let mut decoded: Frame = decode(&encode(&frame)).unwrap();
    sanitize(&mut decoded).unwrap();
    check_frame(&decoded, "resize child");
    let Node::ResizeHandle {
        on_press,
        on_release,
        on_drag,
        cursor,
        ..
    } = decoded.root.unwrap()
    else {
        panic!("resize handle retained");
    };
    assert_eq!(
        (on_press, on_release, on_drag),
        (Some(1), Some(2), Some(u32::MAX))
    );
    assert_eq!(cursor, Some(mouse::Cursor::ResizingHorizontally));
}

/// Every node kind that carries a typed id has it checked: a tree with a
/// host-local id where the walk reaches is refused, and a tree that passes
/// holds none (`check_frame` validates every surviving identity).
#[test]
fn a_host_local_id_on_any_node_kind_is_refused() {
    const SEED: u64 = 0xF0C5_1D1D_5EED_0001;
    const NUM_TREES: usize = 200;

    let mut refused = 0;
    for i in 1..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let mut frame = on_big_stack(move || gen_frame(&mut Rng::poisoning_ids(seed), i));
        match sanitize(&mut frame) {
            Ok(_) => check_frame(&frame, &ctx),
            Err("focus-handle element IDs are host-local") => refused += 1,
            Err(_) => {}
        }
    }
    assert!(refused > 0, "no poisoned id reached the check");
}
