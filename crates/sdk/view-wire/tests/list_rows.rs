//! A list's rows diff by item index: moving the window a row is a row's
//! worth of patches, and every window move still patches to the new tree.
use gpui::StyleRefinement;
use view_wire::{ListAlignment, ListSizingBehavior, Node, Patch, TextNode};

fn list(start: usize, len: usize, edited: Option<usize>) -> Node {
    Node::List {
        state: 1,
        path: Vec::new(),
        item_count: 100,
        alignment: ListAlignment::Bottom,
        overdraw: 0.,
        sizing: ListSizingBehavior::Infer,
        following_tail: false,
        revision: 0,
        commands: Vec::new(),
        request_handler: 1,
        scroll_handler: None,
        range_start: start,
        style: StyleRefinement::default(),
        children: (start..start + len)
            .map(|item| {
                Node::Text(TextNode {
                    id: None,
                    style: StyleRefinement::default(),
                    content: match edited == Some(item) {
                        true => format!("row {item} edited"),
                        false => format!("row {item}"),
                    },
                    heading: None,
                    live: None,
                })
            })
            .collect(),
    }
}

#[test]
fn a_row_scrolled_in_is_one_insert() {
    let mut old = list(50, 24, None);
    let mut new = list(49, 25, None);
    let patches = view_wire::diff(&mut old, &mut new);
    let inserts = patches
        .iter()
        .filter(|p| matches!(p, Patch::Insert { .. }))
        .count();
    assert_eq!(inserts, 1, "{patches:?}");
    assert!(patches.len() <= 2, "{patches:?}");
}

#[test]
fn every_window_move_patches_to_the_new_list() {
    let windows = [
        (50, 24),
        (49, 25),
        (60, 10),
        (40, 40),
        (0, 0),
        (74, 26),
        (0, 64),
        (90, 3),
    ];
    for &(a, n) in &windows {
        for &(b, m) in &windows {
            for edited in [None, Some(55), Some(49)] {
                let mut old = list(a, n, None);
                let mut new = list(b, m, edited);
                let patches = view_wire::diff(&mut old, &mut new);
                view_wire::apply(&mut old, patches).unwrap();
                assert_eq!(old, new, "{a}+{n} -> {b}+{m}, edited {edited:?}");
            }
        }
    }
}
