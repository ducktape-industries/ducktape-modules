//! Serializable adapters for the small part of GPUI interactivity that can
//! cross the view boundary. Native GPUI interactivity contains callbacks and
//! frame state, so only its declarative refinements are wire data.

use gpui::{ElementId, EntityId, FocusId, SharedString, StyleRefinement};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

/// A tagged, lossless wire form of GPUI's element identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ElementIdWire {
    View(u64),
    Integer(u64),
    Name(SharedString),
    Uuid([u8; 16]),
    FocusHandle(u64),
    NamedInteger(SharedString, u64),
    Path(String),
    CodeLocation {
        file: String,
        line: u32,
        column: u32,
    },
    NamedChild(Box<Self>, SharedString),
    OpaqueId([u8; 20]),
}

impl ElementIdWire {
    /// Convert an authoring ID without erasing its GPUI variant.
    pub fn from_gpui(id: ElementId) -> Self {
        match id {
            ElementId::View(id) => Self::View(id.as_u64()),
            ElementId::Integer(id) => Self::Integer(id),
            ElementId::Name(id) => Self::Name(id),
            ElementId::Uuid(id) => Self::Uuid(*id.as_bytes()),
            ElementId::FocusHandle(id) => Self::FocusHandle(slotmap::Key::data(&id).as_ffi()),
            ElementId::NamedInteger(name, id) => Self::NamedInteger(name, id),
            ElementId::Path(path) => Self::Path(path.to_string_lossy().into_owned()),
            ElementId::CodeLocation(location) => Self::CodeLocation {
                file: location.file().into(),
                line: location.line(),
                column: location.column(),
            },
            ElementId::NamedChild(id, name) => {
                Self::NamedChild(Box::new(Self::from_gpui((*id).clone())), name)
            }
            ElementId::OpaqueId(id) => Self::OpaqueId(id),
        }
    }

    /// Lower an ID to the native GPUI type. Code locations are source metadata
    /// and cannot be reconstructed as a `'static` caller location; rejecting
    /// that variant preserves the tag instead of silently changing identity.
    pub fn to_gpui(&self) -> Result<ElementId, &'static str> {
        Ok(match self {
            Self::View(id) => ElementId::View(EntityId::from(*id)),
            Self::Integer(id) => ElementId::Integer(*id),
            Self::Name(name) => ElementId::Name(name.clone()),
            Self::Uuid(id) => ElementId::Uuid(Uuid::from_bytes(*id)),
            Self::FocusHandle(id) => ElementId::FocusHandle(FocusId::from(
                slotmap::KeyData::from_ffi(*id),
            )),
            Self::NamedInteger(name, id) => ElementId::NamedInteger(name.clone(), *id),
            Self::Path(path) => ElementId::Path(Arc::from(PathBuf::from(path))),
            Self::CodeLocation { .. } => return Err("code-location element IDs cannot cross the wire"),
            Self::NamedChild(id, name) => {
                ElementId::NamedChild(Arc::new(id.to_gpui()?), name.clone())
            }
            Self::OpaqueId(id) => ElementId::OpaqueId(*id),
        })
    }

    /// The string key used by patch matching for named IDs.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name.as_ref()),
            _ => None,
        }
    }
}

/// Declarative interactivity lowered into native GPUI's `Interactivity`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Interactivity {
    pub id: Option<ElementIdWire>,
    pub group: Option<SharedString>,
    pub hover: Option<StyleRefinement>,
    pub active: Option<StyleRefinement>,
    pub group_hover: Option<GroupRefinement>,
    pub group_active: Option<GroupRefinement>,
    pub on_click: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupRefinement {
    pub group: SharedString,
    pub style: StyleRefinement,
}
