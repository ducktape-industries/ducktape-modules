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
            left == right
        }
        (None, None) => true,
        _ => false,
    }
}

