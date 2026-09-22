use crate::ElementIdWire;

/// An owned typed identity used by keyed diffing.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IdentityKey {
    Element(ElementIdWire),
}

/// A borrowed identity view used while comparing nodes without formatting IDs.
#[derive(Clone, Copy, Debug)]
pub enum IdentityKeyRef<'a> {
    Element(&'a ElementIdWire),
}

impl IdentityKeyRef<'_> {
    pub fn to_owned(self) -> IdentityKey {
        match self {
            Self::Element(id) => IdentityKey::Element(id.clone()),
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
        (None, None) => true,
        _ => false,
    }
}
