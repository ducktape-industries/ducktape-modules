#[cfg(test)]
mod identity_tests {
    use view_wire::{ElementIdWire, Frame, Interactivity, Node, apply, diff, sanitize};

    fn text(id: ElementIdWire, content: &str) -> Node {
        Node::Text(view_wire::TextNode {
            id: Some(id),
            style: gpui::StyleRefinement::default(),
            content: content.into(),
            heading: None,
            live: None,
        })
    }

    fn container(children: Vec<Node>) -> Node {
        Node::Container(view_wire::ContainerNode {
            id: None,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children,
        })
    }

    fn identified_container(id: ElementIdWire, children: Vec<Node>) -> Node {
        Node::Container(view_wire::ContainerNode {
            id: Some(id),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children,
        })
    }

    #[test]
    fn typed_ids_drive_keyed_patch_moves_without_stringification() {
        let old = container(vec![
            text(ElementIdWire::Integer(11), "one"),
            text(ElementIdWire::Uuid([2; 16]), "two"),
        ]);
        let mut new = container(vec![
            text(ElementIdWire::Uuid([2; 16]), "two"),
            text(ElementIdWire::Integer(11), "one changed"),
        ]);
        let mut applied = old.clone();
        let patches = diff(&mut applied, &mut new);
        assert!(
            patches
                .iter()
                .any(|patch| matches!(patch, view_wire::Patch::Move { .. }))
        );
        apply(&mut applied, patches).expect("typed keyed patch should apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn duplicate_typed_sibling_ids_are_refused() {
        let mut frame = Frame {
            root: Some(container(vec![
                text(ElementIdWire::Name("same".into()), "one"),
                text(ElementIdWire::Name("same".into()), "two"),
            ])),
            ..Frame::default()
        };
        assert_eq!(
            sanitize(&mut frame),
            Err("duplicate typed element identity among siblings")
        );
    }

    #[test]
    fn duplicate_ids_through_anonymous_wrappers_share_the_parent_scope() {
        let mut frame = Frame {
            root: Some(container(vec![
                container(vec![text(ElementIdWire::Name("same".into()), "one")]),
                text(ElementIdWire::Name("same".into()), "two"),
            ])),
            ..Frame::default()
        };
        assert_eq!(
            sanitize(&mut frame),
            Err("duplicate typed element identity among siblings")
        );
    }

    #[test]
    fn equal_local_ids_under_distinct_identified_parents_are_allowed() {
        let mut frame = Frame {
            root: Some(container(vec![
                identified_container(
                    ElementIdWire::Name("left".into()),
                    vec![text(ElementIdWire::Name("child".into()), "left")],
                ),
                identified_container(
                    ElementIdWire::Name("right".into()),
                    vec![text(ElementIdWire::Name("child".into()), "right")],
                ),
            ])),
            ..Frame::default()
        };
        assert!(sanitize(&mut frame).is_ok());
    }

    #[test]
    fn numeric_and_name_ids_remain_distinct_in_one_scope() {
        let mut frame = Frame {
            root: Some(container(vec![
                text(ElementIdWire::Integer(1), "number"),
                text(ElementIdWire::Name("1".into()), "name"),
            ])),
            ..Frame::default()
        };
        assert!(sanitize(&mut frame).is_ok());
    }

    #[test]
    fn patch_inserting_a_collision_hidden_by_a_wrapper_is_refused() {
        let mut root = container(vec![text(ElementIdWire::Name("same".into()), "one")]);
        let result = apply(
            &mut root,
            vec![view_wire::Patch::Insert {
                path: vec![],
                index: 1,
                node: container(vec![text(ElementIdWire::Name("same".into()), "two")]),
            }],
        );
        assert_eq!(
            result,
            Err("duplicate typed element identity among siblings")
        );
    }
}

#[cfg(test)]
mod style_tests {
    use gpui::{ElementId, SharedString};
    use std::sync::Arc;
    use view_wire::{ElementIdAtom, ElementIdWire, MAX_ELEMENT_ID_DEPTH};

    #[test]
    fn named_child_is_flattened_and_round_trips() {
        let inner = ElementId::NamedChild(Arc::new(ElementId::Name("row".into())), "inner".into());
        let id = ElementId::NamedChild(Arc::new(inner), "outer".into());

        let wire = ElementIdWire::from_gpui(id.clone()).expect("GPUI ID should be supported");
        let ElementIdWire::NamedChild { names, .. } = &wire else {
            panic!("named child must retain its typed shape")
        };
        assert_eq!(
            names.as_slice(),
            [SharedString::from("inner"), SharedString::from("outer")]
        );
        assert_eq!(wire.to_gpui().expect("wire ID should lower"), id);
    }

    #[test]
    fn named_child_depth_is_bounded_during_decode() {
        let wire = ElementIdWire::NamedChild {
            base: ElementIdAtom::Integer(1),
            names: (0..=MAX_ELEMENT_ID_DEPTH)
                .map(|index| SharedString::from(format!("name-{index}")))
                .collect(),
        };
        let encoded = rmp_serde::to_vec_named(&wire).expect("wire ID should encode");
        let result = rmp_serde::from_slice::<ElementIdWire>(&encoded);
        assert!(result.is_err(), "over-depth identity must be rejected");
    }

    #[test]
    fn unsupported_or_invalid_ids_are_rejected_without_fallbacks() {
        assert!(ElementIdWire::FocusHandle(1).to_gpui().is_err());
        assert!(
            ElementIdWire::CodeLocation {
                file: "view.rs".into(),
                line: 1,
                column: 1,
            }
            .to_gpui()
            .is_err()
        );
        assert!(ElementIdWire::Path(vec![0xff]).to_gpui().is_err());
    }
}
