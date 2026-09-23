//! Wire representations of GPUI style and element identity.
//!
//! The wire identity keeps the GPUI tag instead of reducing an ID to a string.
//! `NamedChild` is stored as one atom plus a bounded list of names. This keeps
//! decoding iterative and prevents attacker-controlled recursive allocation.

use gpui::{ElementId, EntityId, FocusId, SharedString, StyleRefinement};
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

/// The maximum number of named-child components accepted in one element ID.
pub const MAX_ELEMENT_ID_DEPTH: usize = 64;

/// The non-recursive base of a GPUI element ID.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ElementIdAtom {
    View(u64),
    Integer(u64),
    Name(SharedString),
    Uuid([u8; 16]),
    FocusHandle(u64),
    NamedInteger(SharedString, u64),
    Path(Vec<u8>),
    CodeLocation {
        file: String,
        line: u32,
        column: u32,
    },
    OpaqueId([u8; 20]),
}

/// A tagged, lossless wire form of GPUI's element identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub enum ElementIdWire {
    View(u64),
    Integer(u64),
    Name(SharedString),
    Uuid([u8; 16]),
    FocusHandle(u64),
    NamedInteger(SharedString, u64),
    /// UTF-8 path bytes. Non-UTF-8 paths are rejected at the GPUI boundary.
    Path(Vec<u8>),
    CodeLocation {
        file: String,
        line: u32,
        column: u32,
    },
    NamedChild {
        base: ElementIdAtom,
        names: Vec<SharedString>,
    },
    OpaqueId([u8; 20]),
}

impl<'de> Deserialize<'de> for ElementIdWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum WireRepr {
            View(u64),
            Integer(u64),
            Name(SharedString),
            Uuid([u8; 16]),
            FocusHandle(u64),
            NamedInteger(SharedString, u64),
            Path(Vec<u8>),
            CodeLocation {
                file: String,
                line: u32,
                column: u32,
            },
            NamedChild {
                base: ElementIdAtom,
                #[serde(deserialize_with = "deserialize_names")]
                names: Vec<SharedString>,
            },
            OpaqueId([u8; 20]),
        }

        Ok(match WireRepr::deserialize(deserializer)? {
            WireRepr::View(id) => Self::View(id),
            WireRepr::Integer(id) => Self::Integer(id),
            WireRepr::Name(name) => Self::Name(name),
            WireRepr::Uuid(id) => Self::Uuid(id),
            WireRepr::FocusHandle(id) => Self::FocusHandle(id),
            WireRepr::NamedInteger(name, id) => Self::NamedInteger(name, id),
            WireRepr::Path(path) => Self::Path(path),
            WireRepr::CodeLocation { file, line, column } => {
                Self::CodeLocation { file, line, column }
            }
            WireRepr::NamedChild { base, names } => Self::NamedChild { base, names },
            WireRepr::OpaqueId(id) => Self::OpaqueId(id),
        })
    }
}

fn deserialize_names<'de, D>(deserializer: D) -> Result<Vec<SharedString>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Names;

    impl<'de> de::Visitor<'de> for Names {
        type Value = Vec<SharedString>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most 64 named-child components")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut names = Vec::with_capacity(MAX_ELEMENT_ID_DEPTH.min(8));
            while let Some(name) = sequence.next_element::<SharedString>()? {
                if names.len() == MAX_ELEMENT_ID_DEPTH {
                    return Err(de::Error::custom("named-child identity is too deep"));
                }
                names.push(name);
            }
            Ok(names)
        }
    }

    deserializer.deserialize_seq(Names)
}

impl ElementIdWire {
    /// Convert an authoring ID without erasing its GPUI variant.
    pub fn from_gpui(id: ElementId) -> Result<Self, &'static str> {
        let mut names = Vec::new();
        let mut current = id;
        let base = loop {
            match current {
                ElementId::NamedChild(parent, name) => {
                    if names.len() == MAX_ELEMENT_ID_DEPTH {
                        return Err("named-child identity is too deep");
                    }
                    names.push(name);
                    current = (*parent).clone();
                }
                base => break ElementIdAtom::from_gpui(base)?,
            }
        };

        if names.is_empty() {
            Ok(Self::from_atom(base))
        } else {
            names.reverse();
            Ok(Self::NamedChild { base, names })
        }
    }

    /// Lower an ID to native GPUI, rejecting IDs that cannot be reconstructed
    /// in the host process without changing their identity.
    pub fn to_gpui(&self) -> Result<ElementId, &'static str> {
        self.validate_host()?;
        match self {
            Self::View(id) => Ok(ElementId::View(EntityId::from(*id))),
            Self::Integer(id) => Ok(ElementId::Integer(*id)),
            Self::Name(name) => Ok(ElementId::Name(name.clone())),
            Self::Uuid(id) => Ok(ElementId::Uuid(Uuid::from_bytes(*id))),
            Self::FocusHandle(id) => Ok(ElementId::FocusHandle(FocusId::from(
                slotmap::KeyData::from_ffi(*id),
            ))),
            Self::NamedInteger(name, id) => Ok(ElementId::NamedInteger(name.clone(), *id)),
            Self::Path(path) => Ok(ElementId::Path(Arc::from(PathBuf::from(
                std::str::from_utf8(path).map_err(|_| "element path is not UTF-8")?,
            )))),
            Self::CodeLocation { .. } => Err("code-location element IDs cannot cross the wire"),
            Self::NamedChild { base, names } => {
                let mut id = base.to_gpui()?;
                for name in names {
                    id = ElementId::NamedChild(Arc::new(id), name.clone());
                }
                Ok(id)
            }
            Self::OpaqueId(id) => Ok(ElementId::OpaqueId(*id)),
        }
    }

    /// Reject host-unsupported IDs before the renderer can invent a fallback.
    pub fn validate_host(&self) -> Result<(), &'static str> {
        match self {
            Self::FocusHandle(_) => Err("focus-handle element IDs are host-local"),
            Self::Name(name) | Self::NamedInteger(name, _)
                if name.len() > crate::MAX_STRING_BYTES =>
            {
                Err("element identity name is too long")
            }
            Self::CodeLocation { file, .. } if file.len() > crate::MAX_STRING_BYTES => {
                Err("element identity source path is too long")
            }
            Self::Path(path) if path.len() > crate::MAX_STRING_BYTES => {
                Err("element path is too long")
            }
            Self::CodeLocation { .. } => Err("code-location element IDs cannot cross the wire"),
            Self::Path(path) if std::str::from_utf8(path).is_err() => {
                Err("element path is not UTF-8")
            }
            Self::NamedChild { base, names } => {
                if names.is_empty() || names.len() > MAX_ELEMENT_ID_DEPTH {
                    return Err("named-child identity has an invalid depth");
                }
                if names
                    .iter()
                    .any(|name| name.len() > crate::MAX_STRING_BYTES)
                {
                    return Err("named-child identity name is too long");
                }
                base.validate_host()
            }
            _ => Ok(()),
        }
    }

    /// The string key used only by legacy callers that explicitly need names.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name.as_ref()),
            _ => None,
        }
    }

    fn from_atom(atom: ElementIdAtom) -> Self {
        match atom {
            ElementIdAtom::View(id) => Self::View(id),
            ElementIdAtom::Integer(id) => Self::Integer(id),
            ElementIdAtom::Name(name) => Self::Name(name),
            ElementIdAtom::Uuid(id) => Self::Uuid(id),
            ElementIdAtom::FocusHandle(id) => Self::FocusHandle(id),
            ElementIdAtom::NamedInteger(name, id) => Self::NamedInteger(name, id),
            ElementIdAtom::Path(path) => Self::Path(path),
            ElementIdAtom::CodeLocation { file, line, column } => {
                Self::CodeLocation { file, line, column }
            }
            ElementIdAtom::OpaqueId(id) => Self::OpaqueId(id),
        }
    }
}

impl ElementIdAtom {
    fn from_gpui(id: ElementId) -> Result<Self, &'static str> {
        Ok(match id {
            ElementId::View(id) => Self::View(id.as_u64()),
            ElementId::Integer(id) => Self::Integer(id),
            ElementId::Name(id) => Self::Name(id),
            ElementId::Uuid(id) => Self::Uuid(*id.as_bytes()),
            ElementId::FocusHandle(id) => Self::FocusHandle(slotmap::Key::data(&id).as_ffi()),
            ElementId::NamedInteger(name, id) => Self::NamedInteger(name, id),
            ElementId::Path(path) => Self::Path(
                path.to_str()
                    .ok_or("non-UTF-8 element paths cannot cross the wire")?
                    .as_bytes()
                    .to_vec(),
            ),
            ElementId::CodeLocation(location) => Self::CodeLocation {
                file: location.file().into(),
                line: location.line(),
                column: location.column(),
            },
            ElementId::NamedChild(_, _) => {
                return Err("named-child must be flattened before atom conversion");
            }
            ElementId::OpaqueId(id) => Self::OpaqueId(id),
        })
    }

    fn to_gpui(&self) -> Result<ElementId, &'static str> {
        Ok(match self {
            Self::View(id) => ElementId::View(EntityId::from(*id)),
            Self::Integer(id) => ElementId::Integer(*id),
            Self::Name(name) => ElementId::Name(name.clone()),
            Self::Uuid(id) => ElementId::Uuid(Uuid::from_bytes(*id)),
            Self::FocusHandle(id) => {
                ElementId::FocusHandle(FocusId::from(slotmap::KeyData::from_ffi(*id)))
            }
            Self::NamedInteger(name, id) => ElementId::NamedInteger(name.clone(), *id),
            Self::Path(path) => ElementId::Path(Arc::from(PathBuf::from(
                std::str::from_utf8(path).map_err(|_| "element path is not UTF-8")?,
            ))),
            Self::CodeLocation { .. } => {
                return Err("code-location element IDs cannot cross the wire");
            }
            Self::OpaqueId(id) => ElementId::OpaqueId(*id),
        })
    }

    fn validate_host(&self) -> Result<(), &'static str> {
        match self {
            Self::FocusHandle(_) => Err("focus-handle element IDs are host-local"),
            Self::Name(name) | Self::NamedInteger(name, _)
                if name.len() > crate::MAX_STRING_BYTES =>
            {
                Err("element identity name is too long")
            }
            Self::CodeLocation { file, .. } if file.len() > crate::MAX_STRING_BYTES => {
                Err("element identity source path is too long")
            }
            Self::Path(path) if path.len() > crate::MAX_STRING_BYTES => {
                Err("element path is too long")
            }
            Self::CodeLocation { .. } => Err("code-location element IDs cannot cross the wire"),
            Self::Path(path) if std::str::from_utf8(path).is_err() => {
                Err("element path is not UTF-8")
            }
            _ => Ok(()),
        }
    }
}

/// Declarative interactivity lowered into native GPUI's `Interactivity`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Interactivity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<gpui::Role>,
    #[serde(skip_serializing_if = "crate::is_default")]
    pub aria: crate::Aria,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub focusable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_stop: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_index: Option<i32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tab_group: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<StyleRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_focus: Option<StyleRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_visible: Option<StyleRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_context: Option<crate::interactivity::KeyContext>,
    /// Guest-app-local opaque focus allocation. It is never an authored element ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_handle: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub occlude: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub block_mouse_except_scroll: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_control_area: Option<crate::WindowControlArea>,
    #[serde(skip_serializing_if = "crate::is_default")]
    pub hover_listener_mode: crate::HoverListenerMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<SharedString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<StyleRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<StyleRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_hover: Option<GroupRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_active: Option<GroupRefinement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_click: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_aux_click: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_down: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mouse_down: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_down_out: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_up: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mouse_up: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_up_out: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_pressure: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mouse_pressure: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_move: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_mouse_exit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_scroll_wheel: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_pinch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_pinch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_key_down: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_key_down: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_key_up: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_key_up: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_modifiers_changed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_hover: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_file_drop_exit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<crate::Tooltip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupRefinement {
    pub group: SharedString,
    pub style: StyleRefinement,
}
