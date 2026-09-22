use crate::ElementIdWire;
use gpui::SharedString;

/// An owned identity used by keyed diffing. Typed IDs remain typed; legacy
/// string keys stay strings and are never used to represent a GPUI ID.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IdentityKey {
    Element(ElementIdWire),
    Legacy(SharedString),
}

/// A borrowed identity view used while comparing nodes without formatting IDs.
#[derive(Clone, Copy, Debug)]
pub enum IdentityKeyRef<'a> {
    Element(&'a ElementIdWire),
    Legacy(&'a str),
}

impl IdentityKeyRef<'_> {
    pub fn to_owned(self) -> IdentityKey {
        match self {
            Self::Element(id) => IdentityKey::Element(id.clone()),
            Self::Legacy(key) => IdentityKey::Legacy(SharedString::from(key)),
        }
    }
}

pub(crate) fn same_identity(
    left: Option<IdentityKeyRef<'_>>,
    right: Option<IdentityKeyRef<'_>>,
) -> bool {
    match (left, right) {
        (Some(IdentityKeyRef::Element(left)), Some(IdentityKeyRef::Element(right))) => {
            left == right
        }
        (Some(IdentityKeyRef::Legacy(left)), Some(IdentityKeyRef::Legacy(right))) => {
            left.len() == right.len()
                && left
                    .as_bytes()
                    .iter()
                    .zip(right.as_bytes().iter())
                    .all(|(left, right)| left == right)
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Interactivity, Node, apply, diff, sanitize};

    fn text(id: ElementIdWire, content: &str) -> Node {
        Node::Text {
            id: Some(id),
            style: gpui::StyleRefinement::default(),
            content: content.into(),
            heading: None,
            live: None,
        }
    }

    fn container(children: Vec<Node>) -> Node {
        Node::Container {
            id: None,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children,
        }
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
                .any(|patch| matches!(patch, crate::Patch::Move { .. }))
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
}
