use super::*;

fn gen_tree(rng: &mut Rng, depth: usize, width: usize) -> Node {
    let mut node = gen_leaf(rng);
    let width_level = if depth == 0 { 0 } else { rng.next_range(depth) };
    for level in 0..depth {
        if level == width_level && width > 0 {
            let mut children: Vec<Node> = (0..width).map(|_| gen_leaf(rng)).collect();
            children.push(node);
            node = gen_list(rng, children);
            continue;
        }
        node = match rng.next_range(8) {
            7 => Node::ResizeHandle {
                id: ElementIdWire::Name(gen_key(rng).into()),
                on_press: rng.next_bool().then(|| rng.next_u64() as u32),
                on_release: rng.next_bool().then(|| rng.next_u64() as u32),
                on_drag: rng.next_bool().then(|| rng.next_u64() as u32),
                cursor: Some(mouse::Cursor::ResizingHorizontally),
                content: Box::new(node),
                style: gpui::StyleRefinement::default(),
            },
            6 => Node::Tooltip {
                key: gen_key(rng),
                position: TooltipPosition::Bottom,
                delay_ms: rng.next_u64(),
                snap: rng.next_bool(),
                style: gen_native_style(rng),
                children: vec![node, gen_leaf(rng)],
            },
            4 => Node::Sensor {
                id: ElementIdWire::Name(gen_key(rng).into()),
                reset: None,
                on_show: rng.next_bool().then(|| rng.next_u64() as u32),
                on_resize: rng.next_bool().then(|| rng.next_u64() as u32),
                on_hide: rng.next_bool().then(|| rng.next_u64() as u32),
                anticipate: gen_opt_f32(rng),
                delay: gen_opt_f32(rng),
                child: Box::new(node),
                style: gpui::StyleRefinement::default(),
            },
            5 => Node::MouseArea {
                id: ElementIdWire::Name(gen_key(rng).into()),
                role: gen_opt_role(rng),
                label: rng.next_bool().then(|| gen_string(rng)),
                expanded: rng.next_bool().then(|| rng.next_bool()),
                selected: rng.next_bool().then(|| rng.next_bool()),
                checked: rng.next_bool().then(|| rng.next_bool()),
                on_press: rng.next_bool().then(|| rng.next_u64() as u32),
                on_release: rng.next_bool().then(|| rng.next_u64() as u32),
                on_double_click: None,
                on_right_press: None,
                on_right_release: None,
                on_middle_press: None,
                on_middle_release: None,
                on_enter: rng.next_bool().then(|| rng.next_u64() as u32),
                on_exit: None,
                on_move: rng.next_bool().then(|| rng.next_u64() as u32),
                on_press_at: None,
                on_scroll: rng.next_bool().then(|| rng.next_u64() as u32),
                content: Box::new(node),
            },
            0 => gen_container(rng, vec![node]),
            1 => gen_list(rng, vec![node]),
            2 => Node::Scroll {
                on_scroll: Some(7),
                virtual_rows: rng.next_bool(),
                id: ElementIdWire::Name(gen_key(rng).into()),
                direction: *rng.choose(&[
                    ScrollDirection::Vertical,
                    ScrollDirection::Horizontal,
                    ScrollDirection::Both,
                ]),
                style: gen_native_style(rng),
                bar_hidden: rng.next_bool(),
                bar_width: gen_opt_f32(rng),
                bar_margin: gen_opt_f32(rng),
                scroller_width: gen_opt_f32(rng),
                bar_spacing: gen_opt_f32(rng),
                anchor_x: gen_anchor(rng),
                anchor_y: gen_anchor(rng),
                auto_scroll: rng.next_bool(),
                content: Box::new(node),
            },
            _ => Node::Button {
                checked: rng.next_bool().then(|| rng.next_bool()),
                expanded: rng.next_bool().then(|| rng.next_bool()),
                selected: rng.next_bool().then(|| rng.next_bool()),
                role: gen_opt_role(rng),
                description: rng.next_bool().then(|| gen_string(rng)),
                key: gen_key(rng),
                content: ButtonContent::Child(Box::new(node)),
                label: rng.next_bool().then(|| gen_string(rng)),
                on_press: rng.next_bool().then(|| rng.next_u64() as u32),
                style: gpui::StyleRefinement::default(),
            },
        };
    }
    node
}

/// One random `Frame` around a tree of exactly `depth`/`width`: the shared
/// core behind both [`gen_frame`] (which chooses depth/width to stress the
/// decode-time and sanitize-time ceilings) and [`gen_frame_bounded`] (which
/// keeps trees small because its callers re-encode and mutate them
/// hundreds of times each).
pub(super) fn gen_frame_with(rng: &mut Rng, depth: usize, width: usize) -> Frame {
    let root = gen_tree(rng, depth, width);
    let requests = (0..rng.next_range(4))
        .map(|_| Request {
            id: rng.next_u64(),
            kind: gen_string(rng),
            payload: (0..rng.next_range(16))
                .map(|_| rng.next_range(256) as u8)
                .collect(),
        })
        .collect();
    let cancels = (0..rng.next_range(4)).map(|_| rng.next_u64()).collect();
    Frame {
        upstream_sanitization: Default::default(),
        editor_decisions: Vec::new(),
        editor_documents: Vec::new(),
        tooltip_responses: Vec::new(),
        mouse_interest: rng.next_bool(),
        event_interest: Default::default(),
        root: Some(root),
        requests,
        cancels,
        unchanged: rng.next_bool(),
        busy: rng.next_bool(),
        patches: Vec::new(),
    }
}

// ------------------------------------------------------------ patch generator

/// A path into `root`: a real one (a random walk down the tree that stops
/// at a random depth) or, from a `hostile` sender, sometimes one step past
/// a real one or a random vector, so `apply` sees both the paths a guest
/// sends and the ones a hostile one does.
pub(super) fn gen_path(rng: &mut Rng, root: &Node, hostile: bool) -> Vec<u32> {
    let mut path = Vec::new();
    let mut node = root;
    while !node.children().is_empty() && rng.next_range(4) != 0 {
        let index = rng.next_range(node.children().len());
        path.push(index as u32);
        node = &node.children()[index];
    }
    match rng.next_range(12) {
        0 if hostile => path.push(rng.next_range(4) as u32),
        1 if hostile => {
            path = (0..rng.next_range(4))
                .map(|_| rng.next_range(8) as u32)
                .collect()
        }
        _ => {}
    }
    path
}

/// A subtree for a patch to carry: usually small and cheap, occasionally
/// as hostile as [`gen_tree`] goes, so an inserted subtree can push the
/// tree past every ceiling on its own.
pub(super) fn gen_patch_tree(rng: &mut Rng) -> Node {
    let (depth, width) = match rng.next_range(40) {
        0 => (MAX_DEPTH + 4, MAX_NODES / 4),
        _ => (rng.skewed(4, 2), rng.skewed(6, 2)),
    };
    gen_tree(rng, depth, width)
}

/// One random patch against `root` as it stands. A `hostile` sender's
/// indices are sometimes past the list, its list edits sometimes aimed at
/// a node with no list, and its `Props` sometimes a whole subtree; the
/// other kind of sender is what a real diff emits, so a whole sequence of
/// its patches applies and the invariant is checked on the result.
/// A node whose children are a list the host can insert into, remove from
/// and reorder — as opposed to a fixed set of slots. Written out here rather
/// than routed through `Node::child_list_mut`, so a variant that gains or
/// loses its list fails a test instead of agreeing with itself.
pub(super) fn is_list_node(node: &Node) -> bool {
    matches!(
        node,
        Node::Container { .. }
            | Node::Tooltip { .. }
            | Node::Overlay { .. }
            | Node::When { .. }
            | Node::Anchored { .. }
            | Node::Image { .. }
            | Node::UniformList { .. }
    )
}

pub(super) fn gen_patch(rng: &mut Rng, root: &Node, hostile: bool) -> Patch {
    let path = gen_path(rng, root, hostile);
    let mut node = Some(root);
    for index in &path {
        node = node.and_then(|node| node.children().get(*index as usize));
    }
    let is_list = node.is_some_and(is_list_node);
    let len = node.map_or(0, |node| node.children().len());
    let index = |rng: &mut Rng, bound: usize| match rng.next_range(8) {
        0 if hostile => rng.next_range(bound + 3) as u32,
        _ => rng.next_range(bound.max(1)) as u32,
    };
    let kind = match (is_list || hostile, rng.next_range(5)) {
        (false, kind) => kind % 2,
        // Nothing to remove or move in an empty list.
        (true, kind) if kind >= 3 && len == 0 && !hostile => 2,
        (true, kind) => kind,
    };
    match kind {
        0 => Patch::Replace {
            path,
            node: gen_patch_tree(rng),
        },
        1 => {
            // A node with its children set aside, as a guest sends it, of
            // the arity the node at the path has — or, from a hostile
            // sender, any node at all.
            let mut fresh = gen_patch_tree(rng);
            let same_arity = |fresh: &Node| match node {
                // Two list nodes take each other's children whatever the
                // count; two fixed-slot nodes only at the same count.
                Some(at) => match (is_list_node(at), is_list_node(fresh)) {
                    (true, true) => true,
                    (false, false) => at.children().len() == fresh.children().len(),
                    _ => false,
                },
                None => false,
            };
            if !hostile {
                while !same_arity(&fresh) {
                    fresh = gen_patch_tree(rng);
                }
            }
            if hostile && rng.next_range(4) == 0 {
                return Patch::Props { path, node: fresh };
            }
            for child in fresh.children_mut() {
                *child = Node::empty();
            }
            if let Node::Container { children, .. } = &mut fresh {
                children.clear();
            }
            Patch::Props { path, node: fresh }
        }
        2 => Patch::Insert {
            path,
            index: index(rng, len + 1),
            node: gen_patch_tree(rng),
        },
        3 => Patch::Remove {
            path,
            index: index(rng, len),
        },
        _ => Patch::Move {
            path,
            from: index(rng, len),
            to: index(rng, len),
        },
    }
}

/// One random `Frame`: a tree plus a handful of requests whose `kind`
/// string is generated the same hostile way as everything else.
pub(super) fn gen_frame(rng: &mut Rng, i: usize) -> Frame {
    let (depth, width) = if i == 0 {
        // Exactly one tree per run goes just over each door, not far over
        // it, and only once: `sanitize`'s key-collision renaming (`claim`
        // in lib.rs) costs quadratic time in how many nodes share one base
        // key once uniquing runs out of room, this file's key pool is
        // deliberately tiny (collisions are the point), and `sanitize`
        // stops at MAX_NODES regardless of how much wider the input tree
        // claims to be — so paying that quadratic cost even once per
        // saturating tree is unavoidable, and paying it many times over
        // (the previous `3 * MAX_NODES`, every 25th tree) is just wasted
        // wall clock, not more coverage.
        (MAX_DEPTH + 8, MAX_NODES + 300)
    } else {
        // A cap and an exponent chosen so this branch, which runs for
        // every other tree, essentially never saturates `sanitize`'s
        // MAX_NODES budget on its own — the forced tree above is what
        // guarantees a saturating, over-both-ceilings tree is exercised.
        (rng.skewed(2 * MAX_DEPTH, 6), rng.skewed(MAX_NODES / 4, 6))
    };
    gen_frame_with(rng, depth, width)
}

/// A frame capped well below [`gen_frame`]'s ceiling-stressing sizes: only
/// [`mutated_bytes_never_panic`] calls this, and it re-encodes and mutates
/// each frame hundreds of times, so a frame in the hundreds-of-KB range
/// (which `gen_frame`'s skew occasionally draws) turns a few hundred
/// `decode` calls into seconds each. The bomb itself — a claimed size with
/// no data behind it — is what exercises decode's refusal path; a tree
/// actually built this wide adds nothing that `gen_frame`'s own forced
/// giants (covered by `random_trees_come_out_of_sanitize_inside_every_bound`)
/// don't already cover.
pub(super) fn gen_frame_bounded(rng: &mut Rng) -> Frame {
    let depth = rng.skewed(MAX_DEPTH / 2, 3);
    let width = rng.skewed(MAX_NODES / 64, 6);
    gen_frame_with(rng, depth, width)
}

/// Runs `f` on a thread with a much larger stack than a test gets by
/// default, then re-raises whatever it did (return value or panic) on the
/// caller — a panic keeps its original message, seed included, instead of
/// being replaced by a generic "thread panicked" one. Building and encoding
/// a tree recurses once per level of nesting the same way decoding does
/// (see `lib.rs`'s own `deep_chain_bytes`), so the frames this file builds
/// up to `2 * MAX_DEPTH` levels deep get the same headroom.
pub(super) fn on_big_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    let handle = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .expect("spawn a big-stack thread");
    match handle.join() {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub(super) fn build_frame(seed: u64, i: usize) -> Frame {
    on_big_stack(move || {
        let mut rng = Rng::new(seed);
        gen_frame(&mut rng, i)
    })
}

pub(super) fn build_and_encode(seed: u64, i: usize) -> (Frame, Vec<u8>) {
    on_big_stack(move || {
        let mut rng = Rng::new(seed);
        let frame = gen_frame(&mut rng, i);
        let bytes = encode(&frame);
        (frame, bytes)
    })
}

pub(super) fn build_and_encode_bounded(seed: u64) -> (Frame, Vec<u8>) {
    on_big_stack(move || {
        let mut rng = Rng::new(seed);
        let frame = gen_frame_bounded(&mut rng);
        let bytes = encode(&frame);
        (frame, bytes)
    })
}
