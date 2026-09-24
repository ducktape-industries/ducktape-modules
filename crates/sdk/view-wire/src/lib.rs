//! The wire between a host and a view running in wasm.
//!
//! The guest ships a WIDGET TREE, not a picture: every tick it returns the
//! [`Node`] its view built, with every value inlined — text, colours, sizes —
//! and the host's own toolkit does layout, render, fonts, IME, clipboard and
//! scroll. The guest never learns where anything landed, which is the point:
//! there is nothing in it to draw with.
//!
//! Interaction goes back as MEANING, not input. A button carries the index of
//! the message the guest queued for it this frame ([`Node::Button`]'s
//! `on_press`); the host sends [`Event::Message`] with that index and the
//! guest runs its own handler. A text field carries a handler index; the host
//! owns the text and sends [`Event::Input`] with what it now reads; a
//! multiline editor the same, with [`Event::EditorTransaction`]. A
//! checkbox, slider or pick list likewise carries a handler index and the
//! host sends the new value ([`Event::Toggle`], [`Event::Slide`],
//! [`Event::Select`]).
//!
//! The types here are the one definition of the format: the guest serializes
//! them and the host deserializes the same code, so a field neither side can
//! drop silently. A host that reads a frame from an untrusted module runs
//! [`sanitize`] first.

/// Exact named-MessagePack protocol implemented by this build. Bump on serialized shape changes,
/// in the SAME commit as the shape change: a view built against the old shape is
/// refused at load instead of faulting on its first frame.
/// This is independent of the calling convention ([`abi`]) and the manifest text format.
/// `tests/golden.rs` holds the bytes of every node, event and door: it fails on
/// any change and says to bump this and regenerate with `WIRE_GOLDEN_WRITE=1`.
///
/// 8: `Event::Response` carries `Result<Vec<u8>, Refusal>` (1b4d8a0 changed the
///    shape and left the epoch at 7; views deployed before it faulted with
///    "invalid u8 while decoding bool" on the first refusal frame).
/// 9: accessible names and roles: `label` on `Node::Editor`, `Slider`, `ComboBox`
///    and `PickList`; `role`, `label`, `expanded`, `selected` and `checked` on
///    `Node::MouseArea`; `selected` on `Node::Button`.
/// 10: the rest of the accessible shape: `heading` and `live` on `Node::Text`,
///    `label` on `Node::Overlay`, `role` on `Node::Button`.
/// 1: reset with the two-codec rule — the tree and `Frame` are named
///    MessagePack, every door in [`doors`] is borsh. Epochs 1–11 of the
///    development era before it are not honoured.
/// 2: a node's `Interactivity` and its `Aria` leave out what is unset (`None`,
///    `false`, the default hover mode, an empty `Aria`), and a container leaves
///    out a default `Interactivity`: an empty one was ~900 bytes of field
///    names, so a room of chat rows ran past a view's per-tick fuel.
/// 3: `notify.show` is gone; `notify.post` is the one way to hand the host a
///    notice.
pub const WIRE_EPOCH: u32 = 3;

/// For `skip_serializing_if`: a value that says nothing is left out.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

pub mod abi;
pub mod doors;
pub mod manifest;
#[cfg(feature = "schema")]
pub mod schema;

mod sanitization;
pub use sanitization::SanitizeReport;

mod subscription;
pub mod task;
pub use subscription::{Recipe, Subscription};
pub use task::Task;

use serde::{Deserialize, Serialize};

mod editor;
pub mod editor_document;
pub mod editor_presentation;
pub mod editor_rich;
pub mod editor_transaction;
pub use editor_transaction::{
    EditorBinding, EditorDecision, EditorEditKind, EditorFault, EditorHistoryEffect,
    EditorKeyClaim, EditorPatch, EditorPatchError, EditorRequest, EditorRequestInput,
    EditorResponse, EditorTransactionEvent, EditorTransactionId, MAX_EDITOR_PATCHES,
    patched_editor_text,
};

pub use editor::{EditorCursor, EditorPosition, EditorState, editor_lines};

mod image;
pub use image::{ImageData, ViewerOptions, viewer_scale_bounds};

mod snapshot;
pub use snapshot::{MAX_SNAPSHOT_BYTES, Snapshot, SnapshotValue};

mod aria;
pub mod click;
pub use aria::Aria;
mod style;
mod style_sanitize;
pub use style::{
    ElementIdAtom, ElementIdWire, GroupRefinement, Interactivity, MAX_ELEMENT_ID_DEPTH,
};

mod combo;
pub use combo::{ComboIcon, ComboOptions};
mod pick;
pub use pick::{PickHandle, PickIcon, PickOptions};

mod tooltip;
pub use tooltip::TooltipPosition;
mod qr;
pub use qr::{MAX_QR_CODES, MAX_QR_PAYLOAD_BYTES, Qr, QrCorrection, QrSize, QrVersion};
mod rich_text;
pub use rich_text::{
    HighlightStyle as RichTextHighlightStyle, Runs as RichTextRuns, TextRun as RichTextRun,
};
mod canvas;
pub mod list;
pub use canvas::{
    CanvasCommand, CanvasLineCap, CanvasLineJoin, CanvasSegment, CanvasShape, CanvasStroke,
    MAX_CANVAS_PARTS,
};
pub use list::{
    ListAlignment, ListCommand, ListKey, ListOffset, ListRequest, ListScroll, ListSizingBehavior,
    MAX_LIST_COMMANDS, MAX_LIST_ITEMS, MAX_LIST_ROWS,
};
mod query;
pub use query::{ContainerQuery, MAX_QUERY_OPS, QueryOp};

mod window;
pub use window::{WindowCommand, WindowControlArea};

mod widget;
pub use widget::{WidgetCommand, WidgetTarget};

mod surface;
pub use surface::{MAX_SURFACE_DEPTH, MAX_SURFACE_VALUES, SurfaceValue, sanitize_surface_event};

mod styled_nodes;
pub use styled_nodes::{ContainerNode, TextNode};
mod node;
pub use node::{
    Anchor, AnchoredFitMode, AnchoredPositionMode, ButtonContent, ImageObjectFit, ImageStyle, Live,
    Node, Role, SvgSource, SvgTransformation,
};
mod accessibility;
pub use accessibility::{Fault, FaultKind, accessibility_faults};
mod patch;
pub use patch::{MAX_PATCHES, Patch, apply, diff};

pub mod events;
pub mod interactivity;
pub mod keyboard;
pub mod mouse;
pub use interactivity::{
    DispatchPhase, HoverListenerMode, KeyContext, RichTextTooltip, Tooltip, TooltipResponse,
};

mod protocol;
pub use protocol::*;

mod frame_sanitize;
#[cfg(test)]
pub(crate) use frame_sanitize::text_amounts;
pub use frame_sanitize::*;
pub(crate) use frame_sanitize::{bound_optional, bounded, finite};

mod codec;
#[cfg(test)]
pub(crate) use codec::MAX_DECODED_NODES;
pub(crate) use codec::{budget, decode_child, decode_children};
pub use codec::{decode, encode, encoded_size, encoded_size_exceeds};

#[cfg(test)]
mod tests;
