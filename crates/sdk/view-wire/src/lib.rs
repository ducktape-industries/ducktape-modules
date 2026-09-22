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
///
/// 8: `Event::Response` carries `Result<Vec<u8>, Refusal>` (1b4d8a0 changed the
///    shape and left the epoch at 7; views deployed before it faulted with
///    "invalid u8 while decoding bool" on the first refusal frame).
/// 9: accessible names and roles: `label` on `Node::Editor`, `Slider`, `ComboBox`
///    and `PickList`; `role`, `label`, `expanded`, `selected` and `checked` on
///    `Node::MouseArea`; `selected` on `Node::Button`.
/// 10: the rest of the accessible shape: `heading` and `live` on `Node::Text`,
///    `label` on `Node::Overlay`, `role` on `Node::Button`.
/// 11: GPUI-shaped container/text styles, typed element IDs, and named
///    MessagePack framing.
pub const WIRE_EPOCH: u32 = 11;

pub mod abi;
pub mod manifest;
#[cfg(feature = "schema")]
pub mod schema;

mod sanitization;
pub use sanitization::SanitizeReport;

mod subscription;
pub mod task;
pub use subscription::{Observer, Recipe, Subscription};
pub use task::Task;

use serde::{Deserialize, Serialize};

mod background;
pub use background::{Background, ColorStop};
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
mod identity;
mod style;
mod style_sanitize;
pub use identity::{IdentityKey, IdentityKeyRef};
pub use style::{
    ElementIdAtom, ElementIdWire, GroupRefinement, Interactivity, MAX_ELEMENT_ID_DEPTH,
};

mod combo;
pub use combo::{ComboIcon, ComboOptions};
mod pick;
pub use pick::{PickHandle, PickIcon, PickOptions};

mod tooltip;
pub use tooltip::{TooltipPosition, TooltipPreset, TooltipStyle};
mod qr;
pub use qr::{MAX_QR_CODES, MAX_QR_PAYLOAD_BYTES, Qr, QrCorrection, QrSize, QrVersion};
mod rich_text;
mod text;
pub use rich_text::{
    HighlightStyle as RichTextHighlightStyle, Runs as RichTextRuns, TextRun as RichTextRun,
};
pub use text::{
    Align, FontFamily, FontStretch, FontStyle, LineHeight, NamedFont, Shaping, TextOptions,
    Wrapping,
};
mod button;
pub use button::{ButtonPreset, ButtonRecipe};
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
pub use interactivity::{DispatchPhase, HoverListenerMode, KeyContext, Tooltip, TooltipResponse};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextHover {
    pub index: Option<u32>,
    pub position: gpui::Point<gpui::Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: gpui::Modifiers,
}

/// Something the host tells the guest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// Observation only: widgets already handled this event once.
    Observation {
        event: events::Event,
        captured: bool,
    },
    /// A mouse interaction in logical coordinates local to the guest surface.
    Mouse {
        event: mouse::Event,
        captured: bool,
    },
    /// A keyboard interaction after the mounted native widgets handled it.
    Keyboard {
        event: keyboard::Event,
        captured: bool,
    },
    /// The user activated the widget the guest gave this message index to
    /// (a button press, an input submit). Indices are per frame: they name
    /// entries in the table the guest filled while building the tree it
    /// last sent.
    Message(u32),
    /// A GPUI click, kept distinct from message routes and carrying its input data.
    Click {
        handler: u32,
        event: click::Click,
    },
    /// GPUI element listener payloads. These routes are allocated per frame.
    MouseDown {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseDown,
    },
    MouseUp {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseUp,
    },
    MouseDownOut {
        handler: u32,
        event: interactivity::MouseDown,
    },
    MouseUpOut {
        handler: u32,
        event: interactivity::MouseUp,
    },
    MousePressure {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MousePressure,
    },
    MouseMove {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseMove,
    },
    MouseExit {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseExit,
    },
    ScrollWheel {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::ScrollWheel,
    },
    Pinch {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::Pinch,
    },
    KeyDown {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::KeyDown,
    },
    KeyUp {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::KeyUp,
    },
    ModifiersChanged {
        handler: u32,
        event: interactivity::ModifiersChanged,
    },
    Hover {
        handler: u32,
        hovered: bool,
    },
    FileDropExit {
        handler: u32,
    },
    AuxClick {
        handler: u32,
        event: click::Click,
    },
    /// Native hover asked the guest to build a tooltip for the displayed frame.
    TooltipRequest {
        request: u32,
    },
    /// A registered host surface emitted its declared result value.
    Surface {
        handler: u32,
        value: SurfaceValue,
    },
    /// A text field's content changed. `handler` indexes the guest's
    /// per-frame input-handler table; `text` is the whole value the host now
    /// holds.
    Input {
        handler: u32,
        text: String,
    },
    /// An editor's text or cursor changed. `reset` fences document replacements;
    /// `revision` orders host observations. Caret-only changes are included.
    /// Initial assignment, mirror repair and exact transfer acknowledgments.
    EditorDocument {
        handler: u32,
        message: editor_document::EditorDocumentMessage,
    },
    EditorRequest {
        handler: u32,
        request: EditorRequest,
    },
    EditorTransaction {
        handler: u32,
        event: EditorTransactionEvent,
    },
    /// The host theme changed; the guest stores the corresponding `Theme` global.
    Theme {
        dark: bool,
    },
    /// A checkbox or toggler flipped. `handler` indexes the guest's
    /// per-frame handler table; `on` is the state it now shows.
    Toggle {
        handler: u32,
        on: bool,
    },
    /// A slider moved to `value`.
    Slide {
        handler: u32,
        value: f32,
    },
    /// A pick list chose the option at `index` in the node's `options`.
    Select {
        handler: u32,
        index: u32,
    },
    /// A native interactive-text hover changed character index.
    RichTextHover {
        handler: u32,
        event: RichTextHover,
    },
    /// A [`Node::Sensor`]'s child was measured: shown at, or resized to,
    /// `width` by `height` — the child's own laid-out size in logical
    /// pixels, never where it sits in the window. `handler` is the node's
    /// `on_show` or `on_resize`.
    ///
    /// Delivered after layout, like a DOM `ResizeObserver`: the host lays
    /// the tree out, the sensor reads its child's size, and the event goes
    /// to the guest on the next tick. A guest whose answer changes the
    /// tree so the child measures differently again is measured again;
    /// a host bounds how many times in a row that may drive a tick before
    /// it stops delivering and logs `sensor loop limit exceeded`.
    Size {
        handler: u32,
        width: f32,
        height: f32,
    },
    /// The pointer is at (`x`, `y`) inside a [`Node::MouseArea`], in the
    /// area's own coordinates — the DOM's `offsetX`/`offsetY`, never the
    /// window's. Carries a move (`on_move`) or a left press (`on_press_at`).
    ///
    /// A host sends at most ONE move per handler per redraw frame, the last
    /// position it saw, as a browser delivers one `pointermove` per frame:
    /// the pointer crosses a thousand pixels a second and every event is a
    /// guest tick. A press is never coalesced.
    Pointer {
        handler: u32,
        x: f32,
        y: f32,
    },
    /// Accumulated logical-pixel movement of a grabbed resize handle.
    Drag {
        handler: u32,
        dx: f64,
        dy: f64,
    },
    /// The wheel turned over a [`Node::MouseArea`] by (`dx`, `dy`), in
    /// pixels when `pixels` is set and in lines otherwise.
    Scroll {
        handler: u32,
        dx: f32,
        dy: f32,
        pixels: bool,
    },
    /// A scrollable's content offset in logical pixels and anchor-relative
    /// fractions, emitted only when its native viewport changes. No window
    /// coordinates cross the wire.
    ScrollOffset {
        handler: u32,
        x: f32,
        y: f32,
        relative_x: f32,
        relative_y: f32,
    },
    /// The native uniform-list viewport needs these rows on the next guest
    /// frame. The route fences a stale request from an older list instance.
    UniformListRange {
        path: Vec<ElementIdWire>,
        route: u32,
        start: u32,
        end: u32,
    },
    UniformListState {
        path: Vec<ElementIdWire>,
        route: u32,
        top_index: u32,
        scrollable: bool,
        scrolled_to_end: Option<bool>,
    },
    /// A native variable-height list requested a bounded item window.
    ListRequest {
        handler: u32,
        request: ListRequest,
    },
    /// Settled native list geometry, emitted after layout state is released.
    ListScroll {
        handler: u32,
        event: ListScroll,
    },
    /// One answer to a [`Request`]. A one-shot request gets exactly one with
    /// `done`; a subscription gets many, the last one `done`.
    Response {
        id: u64,
        result: Result<Vec<u8>, Refusal>,
        done: bool,
    },
    /// The host no longer holds the tree the guest is patching — a patch it
    /// could not apply, a tree it dropped — and wants the next frame whole.
    Resync,
}

/// Why a request failed, as the guest gets it: a stable snake_case `reason` to
/// branch on and the `sentence` to show.
///
/// The host owns the split, and that is the point of this type existing. A
/// refusal used to cross as one flat string carrying its own transport
/// envelope — `RPC returned 400 Bad Request: {"error":"Module(<the module's
/// words>)"}` — so a view that wanted the sentence had to peel the envelope
/// back off, and one that wanted to tell "never" from "not yet" had only prose
/// to read. Every view doing that itself is the same code written as many times
/// as there are views, drifting apart.
///
/// `sentence` is the refusing module's own words, verbatim. Nothing here
/// paraphrases a refusal it did not write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub reason: String,
    pub sentence: String,
}

impl Refusal {
    pub fn new(reason: impl Into<String>, sentence: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            sentence: sentence.into(),
        }
    }
}

/// So a view that only wants to SHOW the refusal writes `{refusal}` and is
/// done — the token is for branching, not for reading.
impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.sentence)
    }
}

/// Something the guest asked the host for. The guest never blocks on it: a
/// future (or stream) inside the guest waits for the matching
/// [`Event::Response`]s, which the host delivers on its own schedule.
///
/// `kind` is `<capability>.<operation>`; the host refuses a capability the
/// app's manifest did not declare.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub kind: String,
    pub payload: Vec<u8>,
}

/// What one tick of the guest produced.
///
/// The tree crosses one of three ways: whole in `root`; not at all, with
/// `unchanged` set; or as `patches` against the tree the host holds, with
/// `root` empty and `unchanged` clear.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Advisory producer report, sticky when a producer sanitizes before encoding.
    /// Receivers must independently sanitize the received whole/applied tree.
    pub upstream_sanitization: SanitizeReport,
    #[serde(deserialize_with = "editor_transaction::decode_responses")]
    pub editor_decisions: Vec<EditorResponse>,
    /// One bounded document message, independent of display text budgets.
    #[serde(deserialize_with = "editor_document::decode_messages")]
    pub editor_documents: Vec<editor_document::EditorDocumentMessage>,
    /// Tooltip subtrees built only after a native hover request.
    pub tooltip_responses: Vec<TooltipResponse>,
    /// The current subscription requests guest-local mouse observations.
    pub mouse_interest: bool,
    /// Live subscriptions opt into each copied event category.
    pub event_interest: events::Interest,
    /// The tree to show. `None` with `unchanged` set means "what you have";
    /// `None` otherwise means "what you have, with `patches` applied".
    pub root: Option<Node>,
    /// Edits to the tree the host holds, in order, when `root` is `None`
    /// and `unchanged` is clear. The host applies them with [`apply`].
    pub patches: Vec<Patch>,
    /// What the guest asked for while producing this frame.
    pub requests: Vec<Request>,
    /// Requests the guest stopped waiting on — a dropped future or stream.
    /// The host frees whatever it kept for them and sends no more answers.
    pub cancels: Vec<u64>,
    /// `root` is `None` because the tree is the one the guest sent last:
    /// the host keeps what it has instead of decoding it again. Requests and
    /// cancels still cross.
    pub unchanged: bool,
    /// The guest ran out of tick budget with work still ready — a task that
    /// yields more than one tick runs, a handler chain longer than one
    /// round — and wants the next tick now, not at the next event or answer.
    pub busy: bool,
}

/// Red, green, blue, alpha in `0.0..=1.0`. The guest resolves its own
/// palette; the host paints what it is told.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba(pub [f32; 4]);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Length {
    Fill,
    FillPortion(u16),
    Shrink,
    Fixed(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    /// Unspecified fields retain the host style, including earlier faces.
    pub color: Option<Rgba>,
    pub width: Option<f32>,
    /// top-left, top-right, bottom-right, bottom-left. `Some([0.0; 4])`
    /// explicitly squares the corners; `None` preserves their radius.
    pub radius: Option<[f32; 4]>,
}

/// Optional native shadow fields; omission retains the host style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub color: Option<Rgba>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub blur: Option<f32>,
}

impl Shadow {
    fn sanitize(&mut self) {
        bound_color(&mut self.color);
        bound_optional(&mut self.blur);
        for value in [&mut self.x, &mut self.y].into_iter().flatten() {
            *value = if value.is_finite() {
                value.clamp(-MAX_PIXELS, MAX_PIXELS)
            } else {
                0.0
            };
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AlignX {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AlignY {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Axis {
    Column,
    Row,
}

/// Optional native row/column wrapping configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Wrap {
    pub spacing: Option<f32>,
    pub align: Option<AlignX>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScrollDirection {
    Vertical,
    Horizontal,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Weight {
    #[default]
    Normal,
    Medium,
    Semibold,
    Bold,
    Thin,
    ExtraLight,
    Light,
    ExtraBold,
    Black,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Font {
    pub monospace: bool,
    pub weight: Weight,
}

/// One state of a button or input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub background: Option<Rgba>,
    pub text: Option<Rgba>,
    pub border: Option<Border>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ButtonStyle {
    pub preset: ButtonPreset,
    pub recipe: Option<ButtonRecipe>,
    pub active: Face,
    pub hovered: Option<Face>,
    pub pressed: Option<Face>,
    pub disabled: Option<Face>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputFace {
    pub icon: Option<Rgba>,
    pub background: Option<Rgba>,
    pub border: Option<Border>,
    pub value: Option<Rgba>,
    pub placeholder: Option<Rgba>,
    pub selection: Option<Rgba>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputStyle {
    pub utility: InputFace,
    pub focus_border: Option<Rgba>,
    pub focused_hovered: Option<InputFace>,
    pub active: InputFace,
    pub hovered: Option<InputFace>,
    pub focused: Option<InputFace>,
    pub disabled: Option<InputFace>,
}

/// Copied input accessibility and native layout options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputOptions {
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
}

/// Copied native multiline editor presentation; state faces share input semantics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditorOptions {
    pub rich: Option<Box<editor_rich::RichPresentation>>,
    pub binding: Option<Box<EditorBinding>>,
    pub presentation: Option<Box<editor_presentation::EditorPresentation>>,
}

impl InputStyle {
    fn sanitize(&mut self) {
        bound_color(&mut self.focus_border);
        for face in [
            Some(&mut self.utility),
            Some(&mut self.active),
            self.hovered.as_mut(),
            self.focused.as_mut(),
            self.focused_hovered.as_mut(),
            self.disabled.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            bound_color(&mut face.background);
            bound_border(&mut face.border);
            bound_color(&mut face.value);
            bound_color(&mut face.icon);
            bound_color(&mut face.placeholder);
            bound_color(&mut face.selection);
        }
    }
}

/// One state of a checkbox, toggler or radio: the box, track or ring; the
/// mark inside it (the check, the knob, the dot); the label; and the border
/// around the box or track. `None` leaves the host's theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlFace {
    pub background: Option<Rgba>,
    pub mark: Option<Rgba>,
    pub text: Option<Rgba>,
    pub border: Option<Border>,
}

/// A theme role a preset paints a control in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Tone {
    Primary,
    Secondary,
    Success,
    Warning,
    Danger,
}

/// A checkbox's or toggler's faces, per state and per value. A hovered or
/// disabled face paints over the active face of the same value, so a state
/// that names only its difference inherits the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToggleStyle {
    /// A checkbox's preset; a switch has none and ignores it.
    pub tone: Option<Tone>,
    pub active_on: Option<ControlFace>,
    pub active_off: Option<ControlFace>,
    pub hovered_on: Option<ControlFace>,
    pub hovered_off: Option<ControlFace>,
    pub disabled_on: Option<ControlFace>,
    pub disabled_off: Option<ControlFace>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RadioStyle {
    pub active_on: Option<ControlFace>,
    pub active_off: Option<ControlFace>,
    pub hovered_on: Option<ControlFace>,
    pub hovered_off: Option<ControlFace>,
}

/// A native slider handle, copied independently for each interaction face.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SliderHandleShape {
    Circle { radius: f32 },
    Rectangle { width: u16, border_radius: [f32; 4] },
}

/// One state of a slider: the rail's two halves and its border, and the
/// handle's colour, border and shape. Omitted fields retain the native style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SliderFace {
    pub rail_start: Option<Rgba>,
    pub rail_end: Option<Rgba>,
    pub rail_width: Option<f32>,
    pub rail_border: Option<Border>,
    pub handle: Option<Rgba>,
    pub handle_border: Option<Border>,
    pub handle_shape: Option<SliderHandleShape>,
}

/// A hovered or dragged face paints over the active one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SliderStyle {
    pub active: Option<SliderFace>,
    pub hovered: Option<SliderFace>,
    pub dragged: Option<SliderFace>,
}

/// One state of a pick list's closed face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PickFace {
    pub background: Option<Rgba>,
    pub text: Option<Rgba>,
    pub placeholder: Option<Rgba>,
    pub handle: Option<Rgba>,
    pub border: Option<Border>,
}

/// The menu a pick list opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MenuFace {
    pub shadow: Shadow,
    pub background: Option<Rgba>,
    pub text: Option<Rgba>,
    pub border: Option<Border>,
    pub selected_text: Option<Rgba>,
    pub selected_background: Option<Rgba>,
}

/// Active overrides apply first; opened-hovered also inherits opened overrides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PickListStyle {
    pub active: Option<PickFace>,
    pub hovered: Option<PickFace>,
    pub opened: Option<PickFace>,
    pub opened_hovered: Option<PickFace>,
    pub menu: Option<MenuFace>,
}

/// Where a scroll's offset is measured from. `Keep` rests at the start and
/// holds the visible rows still when content lands above them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ScrollAnchor {
    #[default]
    Start,
    End,
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContentFit {
    Contain,
    Cover,
    Fill,
    None,
    ScaleDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToggleKind {
    Checkbox,
    Switch,
}

/// A tree deeper than this is cut off: a guest cannot make the host's
/// layout recurse without bound.
pub const MAX_DEPTH: usize = 64;
/// More nodes than this and the host stops reading: the widget tree of a
/// screen, not of a spreadsheet.
pub const MAX_NODES: usize = 8_192;
/// The longest string a single node may carry (text, placeholder, key).
pub const MAX_STRING_BYTES: usize = 64 << 10;
/// The most SHAPED text one frame may carry in total — every [`Node::Text`]
/// content, input or editor value and placeholder, and plain button label
/// together.
///
/// The per-string and per-node caps do not bound this: 128 strings of
/// [`MAX_STRING_BYTES`] are a legal 8 MiB frame, and the host reshapes all
/// of it on the window thread every time the guest changes a character.
/// Measured on a desk machine, that frame took 6.2 s and allocated 3.3 GiB;
/// held to one full-length string's worth of text it takes 69 ms and 26 MiB.
/// A screen shows a couple of kilobytes, so this leaves a guest that means
/// well thirty times what it needs while taking two orders of magnitude off
/// what a hostile one can spend.
///
/// It bounds bytes and nothing else. Two other costs live outside it and are
/// the host's to attack: [`MAX_NODES`] nodes cost around a hundred
/// milliseconds to lay out however short their text, and a byte of Hangul,
/// Han or emoji costs some twenty times a byte of ASCII to shape.
pub const MAX_TEXT_BYTES_PER_FRAME: usize = MAX_STRING_BYTES;
/// The most picture bytes one frame may carry in total, over every
/// [`Node::Svg`] or [`Node::Image`] that brings its payload. A picture that does not fit in
/// what is left is dropped whole, not cut: half an SVG is not an SVG, and
/// the host draws an unknown hash as empty space. A guest sends each
/// picture once, so this bounds what a frame can make the host parse, not
/// what an app can show over its life; an icon is a few kilobytes.
pub const MAX_PICTURE_BYTES_PER_FRAME: usize = 1 << 20;
/// The most options one [`Node::PickList`] may offer: a menu, not a table.
/// Each option is shaped text and spends the frame's text budget too.
pub const MAX_OPTIONS: usize = 256;

/// A uniform list may describe a large logical list without allocating rows.
pub const MAX_UNIFORM_LIST_COUNT: usize = 65_536;
/// A frame and one host range request carry at most this many uniform rows.
pub const MAX_UNIFORM_LIST_ROWS: usize = 256;

/// Maximum positional values supplied to one host surface.
pub const MAX_SURFACE_ARGS: usize = 256;
/// Text and spacing sizes are pixels; nothing on a screen needs more.
const MAX_PIXELS: f32 = 8192.0;
/// A text size, which is not a length: every glyph at it is rasterized and
/// cached, so a screenful of 8192 px text is an atlas no screen asked for.
const MAX_TEXT_PIXELS: f32 = 512.0;

/// Pulls a frame from an untrusted module into what the host is willing to
/// lay out: the tree is truncated past [`MAX_DEPTH`] and [`MAX_NODES`],
/// strings past [`MAX_STRING_BYTES`], shaped text past
/// [`MAX_TEXT_BYTES_PER_FRAME`] in total, picture bytes past
/// [`MAX_PICTURE_BYTES_PER_FRAME`] in total, text sizes to [`MAX_TEXT_PIXELS`],
/// every other size, colour and spacing clamped to a finite range, and a key
/// used twice moved off the one already taken. A frame from a well-behaved
/// guest passes through unchanged.
///
/// A frame that arrived as bytes has passed [`decode`] first, which refuses
/// one nested deeper than this walk goes. A frame that carries `patches`
/// instead of a tree is bounded by [`apply`], since every bound is on the
/// tree the patches make and only the host holds it.
pub fn sanitize(frame: &mut Frame) -> Result<SanitizeReport, &'static str> {
    let mut budgets = Budgets::frame();
    let mut report = if let Some(root) = &mut frame.root {
        sanitize_tree_with(root, &mut budgets)?
    } else {
        SanitizeReport::default()
    };
    frame.tooltip_responses.truncate(MAX_PATCHES);
    let mut kept = 0;
    for index in 0..frame.tooltip_responses.len() {
        if budgets.nodes == 0 {
            break;
        }
        report.merge(sanitize_tree_with(
            &mut frame.tooltip_responses[index].content,
            &mut budgets,
        )?);
        kept += 1;
    }
    frame.tooltip_responses.truncate(kept);
    frame.upstream_sanitization.merge(report);
    for request in &mut frame.requests {
        truncate_string(&mut request.kind);
    }
    Ok(report)
}

// Sanitization may shorten display text, but never an authoritative document.
fn text_amounts(root: &Node) -> Result<(usize, usize), &'static str> {
    let mut pending = vec![root];
    let mut surface_values = Vec::new();
    let mut references = Vec::new();
    let mut display = 0usize;
    while let Some(node) = pending.pop() {
        let mut add = |text: &str| display = display.saturating_add(text.len());
        match node {
            Node::Text { content, .. } => add(content),
            Node::RichText { text, .. } => add(text),
            Node::Input {
                value,
                placeholder,
                options,
                ..
            } => {
                add(value);
                add(placeholder);
                add(&options.label);
                if let Some(description) = &options.description {
                    add(description);
                }
            }
            Node::Editor {
                placeholder,
                label,
                options,
                ..
            } => {
                add(placeholder);
                if let Some(label) = label {
                    add(label);
                }
                if let Some(rich) = &options.rich {
                    for item in &rich.toolbar {
                        add(&item.label);
                    }
                }
            }
            Node::Image { label, .. }
            | Node::ImageViewer { label, .. }
            | Node::Svg { label, .. }
            | Node::MouseArea { label, .. }
            | Node::Slider { label, .. }
            | Node::Overlay { label, .. } => {
                if let Some(label) = label {
                    add(label);
                }
            }
            // Unknown surfaces display their name in the native placeholder.
            Node::Surface { name, args, .. } => {
                add(name);
                surface_values.extend(args);
            }
            Node::Button {
                content,
                label,
                description,
                ..
            } => {
                if let ButtonContent::Label(text) = content {
                    add(text);
                }
                if let Some(label) = label {
                    add(label);
                }
                if let Some(description) = description {
                    add(description);
                }
            }
            Node::Toggle { label, .. } | Node::Radio { label, .. } => add(label),
            Node::ComboBox {
                options,
                placeholder,
                label,
                ..
            } => {
                for option in options {
                    add(option);
                }
                add(placeholder);
                if let Some(label) = label {
                    add(label);
                }
            }
            Node::PickList {
                options,
                placeholder,
                label,
                ..
            } => {
                for option in options {
                    add(option);
                }
                if let Some(placeholder) = placeholder {
                    add(placeholder);
                }
                if let Some(label) = label {
                    add(label);
                }
            }
            _ => {}
        }
        if let Node::Editor {
            document, options, ..
        } = node
        {
            if let Some(rich) = &options.rich {
                rich.document.validate()?;
                if rich.toolbar.len() > editor_presentation::MAX_EDITOR_MENU_ITEMS {
                    return Err("rich toolbar limit");
                }
            }
            references.push(document);
        }
        pending.extend(node.children());
    }
    // Surface strings share the display budget (for example a code preview).
    // Record/type names are routing metadata, not the textual payload itself.
    while let Some(value) = surface_values.pop() {
        match value {
            SurfaceValue::Str(text) => display = display.saturating_add(text.len()),
            SurfaceValue::List(items) => surface_values.extend(items),
            SurfaceValue::Option(Some(item)) => surface_values.push(item.as_ref()),
            SurfaceValue::Record { fields, .. } => {
                surface_values.extend(fields.iter().map(|(_, value)| value));
            }
            SurfaceValue::Unit
            | SurfaceValue::Bool(_)
            | SurfaceValue::I64(_)
            | SurfaceValue::F64(_)
            | SurfaceValue::Option(None) => {}
        }
    }
    editor_document::validate_editor_document_refs(references.iter().copied())
        .map_err(|_| "invalid editor document references or budget")?;
    Ok((references.len(), display))
}

fn sanitize_tree(root: &mut Node) -> Result<SanitizeReport, &'static str> {
    sanitize_tree_with(root, &mut Budgets::frame())
}

fn sanitize_tree_with(
    root: &mut Node,
    budgets: &mut Budgets,
) -> Result<SanitizeReport, &'static str> {
    let (documents, before) = text_amounts(root)?;
    let mut taken = Taken::new();
    let mut identity_scopes = vec![std::collections::HashSet::new()];
    let mut authored_path = Vec::new();
    sanitize_node(
        root,
        0,
        budgets,
        &mut taken,
        &mut identity_scopes,
        &mut authored_path,
    )?;
    let (after_documents, after) = text_amounts(root)?;
    if after_documents != documents {
        return Err("frame budget would remove an editor document projection");
    }
    Ok(SanitizeReport {
        display_text_truncated: after < before,
    })
}

/// Every key claimed in one tree, each with the suffix its next duplicate
/// will try. Remembering the suffix is what keeps a tree of one key linear:
/// searching upward from `#2` on every duplicate walks past every earlier
/// one, and a screen of eight thousand nodes sharing a key — the guest's to
/// send — took nine seconds of the window thread that way.
type Taken = std::collections::HashMap<String, usize>;

/// Typed IDs are unique among the children of the first containing element
/// with an ID. An id-less wrapper is transparent to that GPUI scope; an
/// identified node starts a fresh scope for its descendants.
type IdentityScopes = Vec<std::collections::HashSet<IdentityKey>>;

fn claim_typed_scope(node: &Node, scopes: &mut IdentityScopes) -> Result<bool, &'static str> {
    let Some(IdentityKeyRef::Element(id)) = node.identity() else {
        return Ok(false);
    };
    let scope = scopes
        .last_mut()
        .expect("the root identity scope is always present");
    if !scope.insert(IdentityKey::Element(id.clone())) {
        return Err("duplicate typed element identity among siblings");
    }
    scopes.push(std::collections::HashSet::new());
    Ok(true)
}

fn finish_typed_scope(scopes: &mut IdentityScopes, started: bool) {
    if started {
        scopes.pop();
    }
}

/// A key already used in this tree, made unique. A key is the node's
/// identity — its widget state, its focus target, its accessibility id, and
/// the [`Node::Input`] whose text the host owns — so two nodes sharing one
/// share all of that: typing in either edits both. The tree the guest sent
/// is kept, with the later node moved off the taken key.
fn claim(key: &mut String, taken: &mut Taken) {
    truncate_string(key);
    let Some(mut nth) = taken.get(key.as_str()).copied() else {
        taken.insert(key.clone(), 2);
        return;
    };
    // The guest may itself have sent `key#2`: a suffix already taken is
    // skipped, and the count moves past it for good.
    let mut unique = format!("{key}#{nth}");
    while taken.contains_key(unique.as_str()) {
        nth += 1;
        unique = format!("{key}#{nth}");
    }
    taken.insert(std::mem::replace(key, unique.clone()), nth + 1);
    taken.insert(unique, 2);
}

/// What is left of a frame's per-frame budgets while its tree is walked.
pub(crate) struct Budgets {
    pub(crate) nodes: usize,
    pub(crate) qr_codes: usize,
    pub(crate) canvas_parts: usize,
    pub(crate) surface_values: usize,
    pub(crate) text: usize,
    pub(crate) pictures: usize,
    pub(crate) list_items: usize,
}

impl Budgets {
    pub(crate) fn frame() -> Self {
        Self {
            nodes: MAX_NODES,
            text: MAX_TEXT_BYTES_PER_FRAME,
            pictures: MAX_PICTURE_BYTES_PER_FRAME,
            list_items: MAX_LIST_ITEMS,
            surface_values: MAX_SURFACE_VALUES,
            canvas_parts: MAX_CANVAS_PARTS,
            qr_codes: MAX_QR_CODES,
        }
    }
}

/// Truncates one shaped string to what is left of the frame's text budget
/// and spends what survives. Nodes are walked in tree order, so a frame past
/// the budget keeps its head and loses its tail.
fn spend_text(text: &mut String, budgets: &mut Budgets) {
    truncate_to(text, budgets.text.min(MAX_STRING_BYTES));
    budgets.text -= text.len();
}

/// Spends a picture's bytes from the frame's picture budget, or drops them
/// whole when they do not fit.
fn spend_svg(bytes: &mut Option<Vec<u8>>, budgets: &mut Budgets) {
    match bytes {
        Some(picture) if picture.len() <= budgets.pictures => budgets.pictures -= picture.len(),
        _ => *bytes = None,
    }
}

fn sanitize_node(
    node: &mut Node,
    depth: usize,
    budgets: &mut Budgets,
    taken: &mut Taken,
    identity_scopes: &mut IdentityScopes,
    authored_path: &mut Vec<ElementIdWire>,
) -> Result<(), &'static str> {
    // The caller guarantees one node of budget; a node too deep spends it
    // on the empty node that stands in for it.
    budgets.nodes -= 1;
    if depth >= MAX_DEPTH {
        *node = Node::empty();
        return Ok(());
    }
    let typed_id = match node.identity() {
        Some(IdentityKeyRef::Element(id)) => Some(id.clone()),
        _ => None,
    };
    let typed_scope_started = claim_typed_scope(node, identity_scopes)?;
    if let Some(id) = typed_id {
        authored_path.push(id);
    }
    if let Node::Container { interactivity, .. }
    | Node::UniformList { interactivity, .. }
    | Node::Image { interactivity, .. }
    | Node::Svg { interactivity, .. } = node
    {
        sanitize_interactivity(interactivity);
        if let Some(tooltip) = &mut interactivity.tooltip {
            tooltip.delay_ms = tooltip.delay_ms.min(60_000);
            if budgets.nodes == 0 {
                tooltip.content = None;
            } else if let Some(content) = &mut tooltip.content {
                sanitize_node(
                    content,
                    depth + 1,
                    budgets,
                    taken,
                    &mut vec![std::collections::HashSet::new()],
                    &mut Vec::new(),
                )?;
            }
        }
    }
    match node {
        Node::Container { id, style, .. } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
        }
        Node::UniformList {
            id,
            path,
            style,
            count,
            measure_index,
            scroll_request,
            indices,
            children,
            ..
        } => {
            style_sanitize::sanitize(style);
            id.validate_host()?;
            if path.is_empty()
                || path.len() > 64
                || path.last() != Some(id)
                || path != authored_path
            {
                return Err("uniform-list authored path is invalid");
            }
            for ancestor in path.iter() {
                ancestor.validate_host()?;
            }
            *count = (*count).min(MAX_UNIFORM_LIST_COUNT);
            *measure_index = (*measure_index).min(count.saturating_sub(1));
            if let Some(request) = scroll_request {
                request.offset = request.offset.min(MAX_UNIFORM_LIST_COUNT);
            }
            let mut kept_indices = Vec::with_capacity(indices.len().min(MAX_UNIFORM_LIST_ROWS));
            let mut kept_children = Vec::with_capacity(children.len().min(MAX_UNIFORM_LIST_ROWS));
            for (index, child) in indices.drain(..).zip(children.drain(..)) {
                if (index as usize) < *count && kept_indices.len() < MAX_UNIFORM_LIST_ROWS {
                    kept_indices.push(index);
                    kept_children.push(child);
                }
            }
            *indices = kept_indices;
            *children = kept_children;
        }
        Node::List {
            path,
            item_count,
            overdraw,
            style,
            commands,
            range_start,
            children,
            ..
        } => {
            if path != authored_path {
                return Err("list authored path is invalid");
            }
            for id in path.iter() {
                id.validate_host()?;
            }
            *item_count = (*item_count).min(budgets.list_items);
            budgets.list_items -= *item_count;
            *overdraw = bounded(*overdraw).min(4096.0);
            style_sanitize::sanitize(style);
            commands.truncate(MAX_LIST_COMMANDS);
            for command in commands {
                match command {
                    ListCommand::Reset { count } => *count = (*count).min(MAX_LIST_ITEMS),
                    ListCommand::Splice { start, end, count } => {
                        *start = (*start).min(MAX_LIST_ITEMS);
                        *end = (*end).clamp(*start, MAX_LIST_ITEMS);
                        *count = (*count).min(MAX_LIST_ITEMS);
                    }
                    ListCommand::Remeasure { start, end } => {
                        *start = (*start).min(*item_count);
                        *end = (*end).clamp(*start, *item_count);
                    }
                    ListCommand::ScrollTo(offset) => {
                        offset.item_ix = offset.item_ix.min(*item_count);
                        offset.offset_in_item = bounded(offset.offset_in_item);
                    }
                    ListCommand::ScrollToRevealItem(index) => {
                        *index = (*index).min(item_count.saturating_sub(1));
                    }
                    ListCommand::ScrollToEnd
                    | ListCommand::SetFollowMode { .. }
                    | ListCommand::PauseFollowingTail => {}
                }
            }
            *range_start = (*range_start).min(*item_count);
            children.truncate(MAX_LIST_ROWS.min(item_count.saturating_sub(*range_start)));
        }
        Node::Sensor {
            id,
            reset,
            anticipate,
            delay,
            ..
        } => {
            id.validate_host()?;
            if let Some(value) = reset
                && !value.bound(0, budgets, false)
            {
                *reset = None;
            }
            bound_optional(anticipate);
            if let Some(delay) = delay {
                *delay = finite(*delay).max(0.0);
            }
        }
        Node::MouseArea { key, label, .. } => {
            claim(key, taken);
            if let Some(label) = label {
                truncate_string(label);
            }
        }
        Node::ResizeHandle { id, .. } => id.validate_host()?,
        Node::Responsive { key, .. } | Node::Lazy { key, .. } => claim(key, taken),
        }
        Node::Float {
            key,
            x,
            y,
            scale,
            shadow,
            radius,
            ..
        } => {
            claim(key, taken);
            *x = finite(*x).clamp(-MAX_PIXELS, MAX_PIXELS);
            *y = finite(*y).clamp(-MAX_PIXELS, MAX_PIXELS);
            *scale = finite(*scale).clamp(f32::EPSILON, MAX_PIXELS);
            shadow.sanitize();
            if let Some(corners) = radius {
                for corner in corners {
                    *corner = bounded(*corner);
                }
            }
        }
        Node::Tooltip {
            key,
            gap,
            padding,
            delay_ms,
            style,
            children,
            ..
        } => {
            claim(key, taken);
            *gap = bounded(*gap);
            *padding = bounded(*padding);
            style.sanitize();
            *delay_ms = (*delay_ms).min(60_000);
            children.truncate(2);
        }
        Node::Overlay {
            id,
            label,
            padding,
            backdrop,
            children,
            ..
        } => {
            id.validate_host()?;
            if let Some(label) = label {
                truncate_string(label);
            }
            *padding = bounded(*padding);
            children.truncate(2);
            for channel in &mut backdrop.0 {
                *channel = finite(*channel).clamp(0.0, 1.0);
            }
        }

        Node::Canvas { style, commands } => {
            style_sanitize::sanitize(style);
            canvas::sanitize(commands, budgets);
        }
        Node::Anchored {
            fit,
            position,
            offset,
            ..
        } => {
            for point in [position, offset].into_iter().flatten() {
                for value in point {
                    *value = signed_bounded(*value);
                }
            }
            if let AnchoredFitMode::SnapToWindowWithMargin(edges) = fit {
                for edge in edges {
                    *edge = bounded(*edge);
                }
            }
        }
        Node::Deferred { priority, .. } => *priority = (*priority).min(16),
        Node::When { key, condition, .. } => {
            claim(key, taken);
            condition.sanitize();
        }
        Node::Scroll {
            key,
            bar_width,
            bar_margin,
            scroller_width,
            bar_spacing,
            background,
            border,
            ..
        } => {
            claim(key, taken);
            for number in [bar_width, bar_margin, scroller_width, bar_spacing] {
                bound_optional(number);
            }
            bound_color(background);
            bound_border(border);
        }
        Node::Qr { key, code } => {
            claim(key, taken);
            code.sanitize(budgets);
        }
        Node::RichText {
            id,
            style,
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
            rich_text::sanitize(text, runs, font_family_overrides, clickable_ranges, budgets);
        }
        Node::Text {
            id,
            style,
            content,
            heading,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            style_sanitize::sanitize(style);
            spend_text(content, budgets);
            if heading.is_some_and(|level| !(1..=6).contains(&level)) {
                *heading = None;
            }
        }
        Node::ImageViewer {
            key,
            data,
            label,
            options,
            ..
        } => {
            claim(key, taken);
            ImageData::sanitize(data, budgets);
            if let Some(label) = label {
                truncate_string(label);
            }
            options.sanitize();
        }
        Node::Image {
            id,
            data,
            label,
            style,
            loading,
            fallback,
            state_children,
            ..
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            ImageData::sanitize(data, budgets);
            style_sanitize::sanitize(style);

            if let Some(label) = label {
                spend_text(label, budgets);
            }
            let expected = usize::from(*loading) + usize::from(*fallback);
            state_children.truncate(expected);
            if state_children.len() < expected {
                *loading = false;
                *fallback = false;
                state_children.clear();
            }
        }
        Node::Svg {
            id,
            source,
            transformation,
            label,
            style,
        } => {
            if let Some(id) = id {
                id.validate_host()?;
            }
            match source {
                SvgSource::Data { bytes, .. } => spend_svg(bytes, budgets),
                SvgSource::Asset(path) | SvgSource::External(path) => truncate_string(path),
                SvgSource::None => {}
            }
            for value in &mut transformation.scale {
                *value = signed_bounded(*value);
            }
            for value in &mut transformation.translate {
                *value = signed_bounded(*value);
            }
            transformation.rotate = signed_bounded(transformation.rotate);
            style_sanitize::sanitize(style);

            if let Some(label) = label {
                spend_text(label, budgets);
            }
        }
        Node::Input {
            id,
            placeholder,
            value,
            options,
            style,
            ..
        } => {
            id.validate_host()?;
            spend_text(placeholder, budgets);
            spend_text(value, budgets);
            spend_text(&mut options.label, budgets);
            if let Some(description) = &mut options.description {
                spend_text(description, budgets);
            }
            style_sanitize::sanitize(style);
        }
        Node::Editor {
            options,
            id,
            style,
            placeholder,
            label,
            ..
        } => {
            id.validate_host()?;
            style_sanitize::sanitize(style);
            if let Some(presentation) = &mut options.presentation {
                presentation.sanitize(budgets);
            }
            if let Some(rich) = &mut options.rich {
                for item in &mut rich.toolbar {
                    spend_text(&mut item.label, budgets);
                }
            }
            spend_text(placeholder, budgets);
            if let Some(label) = label {
                spend_text(label, budgets);
            }
        }
        Node::Button {
            key,
            content,
            label,
            description,
            padding,
            style,
            ..
        } => {
            claim(key, taken);
            if let ButtonContent::Label(label) = content {
                spend_text(label, budgets);
            }
            if let Some(label) = label {
                truncate_string(label);
            }
            if let Some(description) = description {
                spend_text(description, budgets);
            }
            if let Some(recipe) = &mut style.recipe {
                recipe.sanitize(budgets);
            }
            bound_edges(padding);
            for face in [
                Some(&mut style.active),
                style.hovered.as_mut(),
                style.pressed.as_mut(),
                style.disabled.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                bound_color(&mut face.background);
                bound_color(&mut face.text);
                bound_border(&mut face.border);
            }
        }
        Node::Space { .. } => {}
        Node::Rule {
            key,
            thickness,
            color,
            radius,
            ..
        } => {
            claim(key, taken);
            *thickness = bounded(*thickness);
            bound_color(color);
            if let Some(radius) = radius {
                for corner in radius {
                    *corner = bounded(*corner);
                }
            }
        }
        Node::Toggle {
            key, label, style, ..
        } => {
            claim(key, taken);
            spend_text(label, budgets);
            for face in [
                &mut style.active_on,
                &mut style.active_off,
                &mut style.hovered_on,
                &mut style.hovered_off,
                &mut style.disabled_on,
                &mut style.disabled_off,
            ]
            .into_iter()
            .flatten()
            {
                bound_control_face(face);
            }
        }
        Node::Radio {
            key, label, style, ..
        } => {
            claim(key, taken);
            spend_text(label, budgets);
            for face in [
                &mut style.active_on,
                &mut style.active_off,
                &mut style.hovered_on,
                &mut style.hovered_off,
            ]
            .into_iter()
            .flatten()
            {
                bound_control_face(face);
            }
        }
        Node::Slider {
            key,
            label,
            value,
            min,
            max,
            step,
            style,
            ..
        } => {
            claim(key, taken);
            if let Some(label) = label {
                truncate_string(label);
            }
            for number in [value, min, max, step] {
                *number = finite(*number);
            }
            for face in [&mut style.active, &mut style.hovered, &mut style.dragged]
                .into_iter()
                .flatten()
            {
                bound_color(&mut face.rail_start);
                bound_color(&mut face.rail_end);
                bound_optional(&mut face.rail_width);
                bound_border(&mut face.rail_border);
                bound_color(&mut face.handle);
                bound_border(&mut face.handle_border);
                if let Some(shape) = &mut face.handle_shape {
                    let radii: &mut [f32] = match shape {
                        SliderHandleShape::Circle { radius } => std::slice::from_mut(radius),
                        SliderHandleShape::Rectangle { border_radius, .. } => border_radius,
                    };
                    for radius in radii {
                        *radius = bounded(*radius);
                    }
                }
            }
        }
        Node::ComboBox {
            key,
            state_key,
            options,
            selected,
            placeholder,
            label,
            settings,
            ..
        } => {
            claim(key, taken);
            spend_text(state_key, budgets);
            settings.sanitize(budgets);
            options.truncate(MAX_OPTIONS);
            for option in options.iter_mut() {
                spend_text(option, budgets);
            }
            spend_text(placeholder, budgets);
            if let Some(label) = label {
                truncate_string(label);
            }
            if selected.is_some_and(|index| index as usize >= options.len()) {
                *selected = None;
            }
        }
        Node::PickList {
            settings,
            key,
            options,
            selected,
            placeholder,
            label,
            style,
            ..
        } => {
            claim(key, taken);
            if let Some(label) = label {
                truncate_string(label);
            }
            settings.sanitize(budgets);
            for face in [
                &mut style.active,
                &mut style.hovered,
                &mut style.opened,
                &mut style.opened_hovered,
            ]
            .into_iter()
            .flatten()
            {
                bound_color(&mut face.background);
                bound_color(&mut face.text);
                bound_color(&mut face.placeholder);
                bound_color(&mut face.handle);
                bound_border(&mut face.border);
            }
            if let Some(menu) = &mut style.menu {
                menu.shadow.sanitize();
                bound_color(&mut menu.background);
                bound_color(&mut menu.text);
                bound_border(&mut menu.border);
                bound_color(&mut menu.selected_text);
                bound_color(&mut menu.selected_background);
            }
            options.truncate(MAX_OPTIONS);
            for option in options.iter_mut() {
                spend_text(option, budgets);
            }
            if let Some(placeholder) = placeholder {
                spend_text(placeholder, budgets);
            }
            if selected.is_some_and(|index| index as usize >= options.len()) {
                *selected = None;
            }
        }
        Node::Progress {
            key,
            value,
            min,
            max,
            background,
            bar,
            border,
            ..
        } => {
            claim(key, taken);
            for number in [value, min, max] {
                *number = finite(*number);
            }
            bound_color(background);
            bound_color(bar);
            bound_border(border);
        }
        Node::Surface {
            key, name, args, ..
        } => {
            claim(key, taken);
            spend_text(name, budgets);
            args.truncate(MAX_SURFACE_ARGS);
            let mut kept = 0;
            for value in args.iter_mut() {
                if budgets.surface_values == 0 {
                    break;
                }
                if !value.bound(0, budgets, false) {
                    *value = SurfaceValue::Unit;
                }
                kept += 1;
            }
            args.truncate(kept);
        }
    }
    for length in lengths_mut(node) {
        if let Length::Fixed(value) = length {
            *value = bounded(*value);
        }
    }
    // Children past the budget are dropped, not stood in for: a layout of
    // ten thousand rows becomes its first rows, which is what a host can
    // lay out, rather than ten thousand empty nodes it still has to walk.
    if let Node::Container { children, .. }
    | Node::List { children, .. }
    | Node::When { children, .. }
    | Node::Tooltip { children, .. }
    | Node::Overlay { children, .. }
    | Node::Anchored { children, .. }
    | Node::Image {
        state_children: children,
        ..
    } = node
    {
        let mut kept = 0;
        for child in children.iter_mut() {
            if budgets.nodes == 0 {
                break;
            }
            sanitize_node(
                child,
                depth + 1,
                budgets,
                taken,
                identity_scopes,
                authored_path,
            )?;
            kept += 1;
        }
        children.truncate(kept);
        if let Node::Image {
            loading,
            fallback,
            state_children,
            ..
        } = node
        {
            if state_children.len() < usize::from(*loading) + usize::from(*fallback) {
                *loading = false;
                *fallback = false;
                state_children.clear();
            }
        }
        finish_typed_scope(identity_scopes, typed_scope_started);
        if typed_scope_started {
            authored_path.pop();
        }
        return Ok(());
    }
    for child in node.children_mut() {
        if budgets.nodes == 0 {
            *child = Node::empty();
            continue;
        }
        sanitize_node(
            child,
            depth + 1,
            budgets,
            taken,
            identity_scopes,
            authored_path,
        )?;
    }
    finish_typed_scope(identity_scopes, typed_scope_started);
    if typed_scope_started {
        authored_path.pop();
    }
    Ok(())
}

fn lengths_mut(node: &mut Node) -> Vec<&mut Length> {
    let slots: Vec<&mut Option<Length>> = match node {
        Node::Scroll { width, height, .. }
        | Node::Button { width, height, .. }
        | Node::ImageViewer { width, height, .. }
        | Node::Slider { width, height, .. }
        | Node::Space { width, height } => vec![width, height],
        Node::Progress { length, girth, .. } => vec![length, girth],
        Node::Toggle { width, .. }
        | Node::Radio { width, .. }
        | Node::PickList { width, .. }
        | Node::ComboBox { width, .. } => vec![width],
        Node::Container { .. }
        | Node::UniformList { .. }
        | Node::List { .. }
        | Node::Text { .. }
        | Node::Input { .. }
        | Node::RichText { .. }
        | Node::Editor { .. }
        | Node::Qr { .. }
        | Node::Rule { .. }
        | Node::Lazy { .. }
        | Node::Responsive { .. }
        | Node::Sensor { .. }
        | Node::ResizeHandle { .. }
        | Node::MouseArea { .. }
        | Node::Overlay { .. }
        | Node::Tooltip { .. }
        | Node::When { .. }
        | Node::Float { .. }
        | Node::Surface { .. }
        | Node::Anchored { .. }
        | Node::Deferred { .. }
        | Node::Image { .. }
        | Node::Svg { .. }
        | Node::Canvas { .. } => Vec::new(),
    };
    slots.into_iter().flatten().collect()
}

/// Cuts `text` down to [`MAX_STRING_BYTES`] on a char boundary, in place.
/// Shared by [`sanitize`] (a guest's outbound frame) and the host's inbound
/// edit path (a user's keystroke or paste into an [`Node::Input`]) — one
/// bound on any string either side of the wire sends the other.
pub fn truncate_string(text: &mut String) {
    truncate_to(text, MAX_STRING_BYTES);
}

fn truncate_to(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

fn bounded(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(0.0, MAX_PIXELS),
    }
}

/// A number that is not a size: a slider or progress value is the app's,
/// so it is made finite and nothing more. The host clamps it into the range
/// it lays out.
/// A pixel measure that may point either way (a paint-only inset), bounded
/// on both sides; NaN reads as 0.
fn signed_bounded(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(-MAX_PIXELS, MAX_PIXELS),
    }
}

fn finite(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(f32::MIN, f32::MAX),
    }
}

fn bound_optional(value: &mut Option<f32>) {
    if let Some(value) = value {
        *value = bounded(*value);
    }
}

fn bound_edges(edges: &mut Option<Edges>) {
    if let Some(edges) = edges {
        edges.top = bounded(edges.top);
        edges.right = bounded(edges.right);
        edges.bottom = bounded(edges.bottom);
        edges.left = bounded(edges.left);
    }
}

fn sanitize_interactivity(interactivity: &mut Interactivity) {
    interactivity.window_control_area = None;
    interactivity.aria.sanitize();
    for style in [
        &mut interactivity.focus,
        &mut interactivity.in_focus,
        &mut interactivity.focus_visible,
    ]
    .into_iter()
    .flatten()
    {
        style_sanitize::sanitize(style);
    }
    if let Some(context) = &mut interactivity.key_context {
        context
            .entries
            .truncate(interactivity::MAX_KEY_CONTEXT_ENTRIES);
        for entry in &mut context.entries {
            let mut key = entry.key.to_string();
            truncate_string(&mut key);
            entry.key = key.into();
            if let Some(value) = &mut entry.value {
                let mut bounded = value.to_string();
                truncate_string(&mut bounded);
                *value = bounded.into();
            }
        }
    }
    for style in [&mut interactivity.hover, &mut interactivity.active]
        .into_iter()
        .flatten()
    {
        style_sanitize::sanitize(style);
    }
    for group in [
        &mut interactivity.group_hover,
        &mut interactivity.group_active,
    ]
    .into_iter()
    .flatten()
    {
        style_sanitize::sanitize(&mut group.style);
        let mut name = group.group.to_string();
        truncate_string(&mut name);
        group.group = name.into();
    }
    if let Some(group) = &mut interactivity.group {
        let mut name = group.to_string();
        truncate_string(&mut name);
        *group = name.into();
    }
}

fn bound_color(color: &mut Option<Rgba>) {
    if let Some(Rgba(channels)) = color {
        for channel in channels {
            *channel = match channel.is_nan() {
                true => 0.0,
                false => channel.clamp(0.0, 1.0),
            };
        }
    }
}

fn bound_control_face(face: &mut ControlFace) {
    bound_color(&mut face.background);
    bound_color(&mut face.mark);
    bound_color(&mut face.text);
    bound_border(&mut face.border);
}

fn bound_border(border: &mut Option<Border>) {
    if let Some(border) = border {
        bound_color(&mut border.color);
        if let Some(width) = &mut border.width {
            *width = bounded(*width);
        }
        for radius in border.radius.iter_mut().flatten() {
            *radius = bounded(*radius);
        }
    }
}

/// What one `decode` may build before it is refused: enough that
/// [`sanitize`]'s truncation still shapes any tree a real view sends, and
/// few enough that a hostile one cannot make the host allocate its way
/// through a frame's worth of nodes every tick.
const MAX_DECODED_NODES: usize = 16 * MAX_NODES;

/// Bounds what a decode may descend into, since decoding is recursive: a
/// [`Node`] holds its children and serde builds them from the inside out, so
/// a chain of containers is a chain of stack frames. The tree the host walks
/// afterwards — [`sanitize`], the renderer, `Drop` — recurses the same way,
/// which is why the limit is the door rather than each walk.
///
/// [`MAX_FRAME_BYTES`-sized](Frame) input is no protection: a chain deep
/// enough to overflow a host thread's stack is a few tens of kilobytes.
mod budget {
    use std::cell::Cell;

    use super::{MAX_DECODED_NODES, MAX_DEPTH};

    thread_local! {
        static DEPTH: Cell<usize> = const { Cell::new(0) };
        static NODES: Cell<usize> = const { Cell::new(0) };
    }

    /// One node being decoded. Descending past what the host walks, or
    /// building more nodes than it will hold, refuses the whole frame:
    /// there is no partial tree to keep, and a truncated one would be a
    /// tree the guest did not write.
    pub(super) struct Node(());

    impl Node {
        pub(super) fn enter() -> Result<Self, &'static str> {
            let depth = DEPTH.get() + 1;
            if depth > MAX_DEPTH {
                return Err("a tree deeper than the host renders");
            }
            spend(1)?;
            DEPTH.set(depth);
            Ok(Self(()))
        }
    }

    impl Drop for Node {
        fn drop(&mut self) {
            DEPTH.set(DEPTH.get().saturating_sub(1));
        }
    }

    /// Native paragraph spans share the same aggregate allocation allowance as nodes.
    pub(super) fn spend(count: usize) -> Result<(), &'static str> {
        let nodes = NODES.get().saturating_add(count);
        if nodes > MAX_DECODED_NODES {
            return Err("more nodes than the host holds");
        }
        NODES.set(nodes);
        Ok(())
    }

    /// A fresh budget for one top-level [`decode`](super::decode). The depth
    /// unwinds itself; the node count is what one frame may spend.
    pub(super) fn reset() {
        NODES.set(0);
    }
}

fn decode_child<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Box<Node>, D::Error> {
    let _node = budget::Node::enter().map_err(serde::de::Error::custom)?;
    Box::<Node>::deserialize(deserializer)
}

fn decode_children<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Node>, D::Error> {
    struct Children;

    impl<'de> serde::de::Visitor<'de> for Children {
        type Value = Vec<Node>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a list of nodes")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut children: A,
        ) -> Result<Self::Value, A::Error> {
            let mut nodes = Vec::new();
            // The guard is held while one child is built and dropped before
            // the next: siblings share a depth, and each costs a node.
            while let Some(child) = {
                let _node = budget::Node::enter().map_err(serde::de::Error::custom)?;
                children.next_element::<Node>()?
            } {
                nodes.push(child);
            }
            Ok(nodes)
        }
    }

    deserializer.deserialize_seq(Children)
}

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    rmp_serde::to_vec_named(value).expect("wire types are plain data")
}

/// How many bytes [`encode`] would write, without writing them.
pub fn encoded_size<T: Serialize>(value: &T) -> u64 {
    struct Count(u64);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len() as u64;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    value
        .serialize(&mut rmp_serde::Serializer::new(&mut count).with_struct_map())
        .expect("wire types are plain data");
    count.0
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, String> {
    budget::reset();
    surface::reset_decode_budget();
    editor_transaction::reset_decode_budget();
    canvas::reset_decode_budget();
    let mut deserializer = rmp_serde::Deserializer::new(std::io::Cursor::new(bytes));
    let value = T::deserialize(&mut deserializer).map_err(|error| error.to_string())?;
    if deserializer.position() != bytes.len() as u64 {
        return Err("trailing MessagePack bytes".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str) -> Node {
        Node::Text {
            id: None,
            style: gpui::StyleRefinement::default(),
            content: content.into(),
            heading: None,
            live: None,
        }
    }

    #[test]
    fn actual_display_truncation_report_survives_encoding_and_resanitizing() {
        let mut frame = Frame {
            root: Some(text(&"x".repeat(MAX_STRING_BYTES + 1))),
            ..Default::default()
        };
        let report = sanitize(&mut frame).unwrap();
        assert!(
            report.display_text_truncated,
            "actual shortened text must be reported"
        );
        assert_eq!(frame.upstream_sanitization, report);
        let mut received: Frame = decode(&encode(&frame)).unwrap();
        assert!(
            !sanitize(&mut received).unwrap().display_text_truncated,
            "receiver observes no additional shortening"
        );
        assert!(
            received.upstream_sanitization.display_text_truncated,
            "producer loss cannot disappear across a wire hop"
        );
        let mut small = Frame {
            root: Some(text("complete")),
            ..Default::default()
        };
        assert_eq!(sanitize(&mut small).unwrap(), SanitizeReport::default());
        assert_eq!(small.upstream_sanitization, SanitizeReport::default());
    }

    #[test]
    fn text_passed_to_a_host_surface_reports_actual_loss() {
        let mut frame = Frame {
            root: Some(Node::Surface {
                key: "preview".into(),
                name: "forge_code".into(),
                args: vec![SurfaceValue::Record {
                    name: "Preview".into(),
                    fields: vec![(
                        "text".into(),
                        SurfaceValue::Option(Some(Box::new(SurfaceValue::List(vec![
                            SurfaceValue::Str("x".repeat(MAX_STRING_BYTES)),
                        ])))),
                    )],
                }],
                on_event: None,
            }),
            ..Default::default()
        };
        assert!(
            sanitize(&mut frame).unwrap().display_text_truncated,
            "surface text spends the same frame budget and its loss must be reported"
        );
        assert!(!sanitize(&mut frame).unwrap().display_text_truncated);
    }

    #[test]
    fn applied_aggregate_text_and_rich_text_loss_is_reported_but_removal_is_not() {
        let rich = Node::RichText {
            id: Some(ElementIdWire::Name("rich".into())),
            style: gpui::StyleRefinement::default(),
            text: "y".repeat(MAX_TEXT_BYTES_PER_FRAME / 2),
            runs: RichTextRuns::default(),
            font_family_overrides: vec![],
            clickable_ranges: vec![],
            on_click: None,
            on_hover: None,
        };
        let mut root = Node::Container {
            id: Some(ElementIdWire::Name("root".into())),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children: vec![text(&"x".repeat(MAX_TEXT_BYTES_PER_FRAME / 2 + 1))],
        };
        let report = apply(
            &mut root,
            vec![Patch::Insert {
                path: vec![],
                index: 1,
                node: rich,
            }],
        )
        .unwrap();
        assert!(
            report.display_text_truncated,
            "each node fits, but the applied aggregate loses tail text"
        );
        let (_, bytes) = text_amounts(&root).unwrap();
        assert_eq!(bytes, MAX_TEXT_BYTES_PER_FRAME);
        let report = apply(
            &mut root,
            vec![Patch::Remove {
                path: vec![],
                index: 0,
            }],
        )
        .unwrap();
        assert!(
            !report.display_text_truncated,
            "intentional removal precedes the measured sanitizer pass"
        );
    }

    #[test]
    fn shadow_sanitization_preserves_signed_offsets_and_bounds_untrusted_values() {
        let mut shadow = Shadow {
            color: Some(Rgba([f32::NAN, -1.0, 2.0, 0.5])),
            x: Some(-12.0),
            y: Some(f32::INFINITY),
            blur: Some(-1.0),
        };
        shadow.sanitize();
        assert_eq!(
            shadow,
            Shadow {
                color: Some(Rgba([0.0, 0.0, 1.0, 0.5])),
                x: Some(-12.0),
                y: Some(0.0),
                blur: Some(0.0)
            }
        );
        shadow.x = Some(-f32::MAX);
        shadow.y = Some(f32::MAX);
        shadow.blur = Some(f32::MAX);
        shadow.sanitize();
        assert_eq!(
            (shadow.x, shadow.y, shadow.blur),
            (Some(-MAX_PIXELS), Some(MAX_PIXELS), Some(MAX_PIXELS))
        );
    }

    fn document_reference(document: &str, byte_len: u32) -> editor_document::EditorDocumentRef {
        editor_document::EditorDocumentRef {
            document: document.into(),
            reset: 3,
            text_revision: 5,
            revision: 7,
            cursor: EditorCursor::default(),
            byte_len,
        }
    }

    fn editor(key: &str, placeholder: &str, document: editor_document::EditorDocumentRef) -> Node {
        Node::Editor {
            options: Default::default(),
            id: ElementIdWire::Name(key.into()),
            style: gpui::StyleRefinement::default(),
            placeholder: placeholder.into(),
            label: None,
            document,
            on_document: 1,
            editable: true,
        }
    }

    /// The tree a host is left holding. Most tests here want only that —
    /// the `Frame` around it is scaffolding, and the report is the business
    /// of the few tests that read it.
    fn sanitized_root(root: Node) -> Node {
        let mut frame = Frame {
            root: Some(root),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        frame.root.unwrap()
    }

    fn sanitized_children(root: Node) -> Vec<Node> {
        let Node::Container { children, .. } = sanitized_root(root) else {
            panic!("a sanitized column is still a column")
        };
        children
    }

    fn column(children: Vec<Node>) -> Node {
        Node::Container {
            id: None,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children,
        }
    }

    #[test]
    fn sanitized_surfaces_share_the_decoders_value_budget() {
        let mut frame = Frame {
            root: Some(column(
                (0..20)
                    .map(|i| Node::Surface {
                        key: format!("surface-{i}"),
                        name: "many".into(),
                        args: vec![SurfaceValue::Unit; MAX_SURFACE_ARGS],
                        on_event: None,
                    })
                    .collect(),
            )),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        let decoded = decode::<Frame>(&encode(&frame));
        assert!(decoded.is_ok(), "sanitized frame must decode: {decoded:?}");
    }

    #[test]
    fn surfaces_round_trip_patch_and_bound_their_arguments() {
        use SurfaceValue as V;
        let values = vec![
            V::Unit,
            V::Bool(true),
            V::I64(i64::MAX),
            V::F64(1.25),
            V::Str("link".into()),
        ];
        let node = Node::Surface {
            key: "view".into(),
            name: "preview".into(),
            args: values.clone(),
            on_event: Some(4),
        };
        assert_eq!(decode::<Node>(&encode(&node)).unwrap(), node);
        for value in values {
            let event = Event::Surface { handler: 4, value };
            assert_eq!(decode::<Event>(&encode(&event)).unwrap(), event);
        }
        let mut changed = node.clone();
        if let Node::Surface { args, on_event, .. } = &mut changed {
            args[1] = V::Bool(false);
            *on_event = Some(9);
        }
        let patches = diff(&mut node.clone(), &mut changed.clone());
        let mut applied = node;
        apply(&mut applied, patches).unwrap();
        assert_eq!(applied, changed);
        let Node::Surface { name, args, .. } = sanitized_root(Node::Surface {
            key: "view".into(),
            name: "preview".into(),
            args: std::iter::once(V::F64(f64::NAN))
                .chain(std::iter::repeat_n(
                    V::Str("é".repeat(MAX_STRING_BYTES)),
                    MAX_SURFACE_ARGS + 1,
                ))
                .collect(),
            on_event: None,
        }) else {
            unreachable!()
        };
        assert_eq!(args.len(), MAX_SURFACE_ARGS);
        assert_eq!(args[0], V::F64(0.0));
        let bytes = args
            .iter()
            .map(|value| match value {
                V::Str(text) => text.len(),
                _ => 0,
            })
            .sum::<usize>();
        assert!(bytes + name.len() <= MAX_TEXT_BYTES_PER_FRAME);
    }

    #[test]
    fn border_sanitization_preserves_absence_and_explicit_zero() {
        let absent = Some(Border {
            color: None,
            width: None,
            radius: None,
        });
        let mut bounded = absent;
        bound_border(&mut bounded);
        assert_eq!(bounded, absent);
        assert_eq!(decode::<Option<Border>>(&encode(&bounded)).unwrap(), absent);
        let mut explicit = Some(Border {
            color: Some(Rgba([0.0; 4])),
            width: Some(f32::NAN),
            radius: Some([-1.0; 4]),
        });
        bound_border(&mut explicit);
        let zero = Some(Border {
            color: Some(Rgba([0.0; 4])),
            width: Some(0.0),
            radius: Some([0.0; 4]),
        });
        assert_eq!(explicit, zero);
        assert_ne!(explicit, absent);
        assert_eq!(decode::<Option<Border>>(&encode(&explicit)).unwrap(), zero);
    }

    #[test]
    fn encoded_size_matches_named_messagepack_without_a_second_buffer() {
        for count in [0, 1, 16, 256, 2000] {
            let frame = Frame {
                root: Some(column(
                    (0..count)
                        .map(|index| keyed(&index.to_string(), "한é"))
                        .collect(),
                )),
                ..Default::default()
            };
            assert_eq!(encoded_size(&frame), encode(&frame).len() as u64);
        }
    }

    #[test]
    fn tooltip_responses_share_the_frame_node_budget() {
        let response = || TooltipResponse {
            request: 1,
            content: Box::new(column((0..MAX_NODES).map(|_| Node::empty()).collect())),
        };
        let mut frame = Frame {
            tooltip_responses: vec![response(), response()],
            ..Default::default()
        };
        sanitize(&mut frame).unwrap();
        assert_eq!(frame.tooltip_responses.len(), 1);
        assert!(frame.tooltip_responses[0].content.count() <= MAX_NODES);
    }

    #[test]
    fn a_frame_round_trips() {
        let frame = Frame {
            upstream_sanitization: Default::default(),
            editor_decisions: Vec::new(),
            editor_documents: Vec::new(),
            tooltip_responses: Vec::new(),
            mouse_interest: true,
            event_interest: Default::default(),
            root: Some(column(vec![
                text("hello"),
                Node::Button {
                    checked: None,
                    expanded: None,
                    selected: None,
                    role: None,
                    description: None,
                    key: "App/b".into(),
                    content: ButtonContent::Label("Go".into()),
                    label: None,
                    on_press: Some(3),
                    width: None,
                    height: None,
                    padding: Some(Edges::all(4.0)),
                    style: ButtonStyle::default(),
                },
                Node::Input {
                    options: Default::default(),
                    id: ElementIdWire::Name("App/i".into()),
                    placeholder: "Name".into(),
                    value: "x".into(),
                    on_input: 0,
                    on_submit: Some(4),
                    secure: false,
                    style: gpui::StyleRefinement::default(),
                },
                Node::Editor {
                    options: Default::default(),
                    id: ElementIdWire::Name("App/e".into()),
                    style: gpui::StyleRefinement::default(),
                    placeholder: "Notes".into(),
                    label: None,
                    document: document_reference("app:draft", 9),
                    on_document: 5,
                    editable: true,
                },
            ])),
            patches: vec![Patch::Remove {
                path: vec![0, 1],
                index: 2,
            }],
            requests: vec![Request {
                id: 1,
                kind: "host.echo".into(),
                payload: b"hi".to_vec(),
            }],
            cancels: vec![2],
            unchanged: false,
            busy: false,
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
        let events = vec![
            Event::Message(3),
            Event::Input {
                handler: 0,
                text: "xy".into(),
            },
            Event::EditorTransaction {
                handler: 5,
                event: EditorTransactionEvent::Commit {
                    origin: None,
                    id: EditorTransactionId {
                        instance: 1,
                        document: "app:draft".into(),
                        reset: 0,
                        sequence: 2,
                        attempt: 0,
                        text_revision: 1,
                        revision: 3,
                    },
                    before: document_reference("app:draft", 9),
                    after: document_reference("app:draft", 10),
                    patches: vec![EditorPatch {
                        start_byte: 9,
                        end_byte: 9,
                        replacement: "z".into(),
                    }],
                    kind: EditorEditKind::Insert,
                    history: EditorHistoryEffect::ExtendPrevious,
                    input_time_ms: 42,
                },
            },
            Event::Toggle {
                handler: 1,
                on: true,
            },
            Event::Slide {
                handler: 2,
                value: 0.5,
            },
            Event::Select {
                handler: 3,
                index: 1,
            },
            Event::Pointer {
                handler: 4,
                x: 12.5,
                y: 3.0,
            },
            Event::Scroll {
                handler: 5,
                dx: 0.0,
                dy: -1.0,
                pixels: false,
            },
            Event::ScrollOffset {
                handler: 6,
                x: 24.0,
                y: 50.0,
                relative_x: 0.2,
                relative_y: 0.1,
            },
            Event::Response {
                id: 1,
                result: Err(Refusal::new("module", "nope")),
                done: true,
            },
            Event::Resync,
        ];
        assert_eq!(decode::<Vec<Event>>(&encode(&events)).unwrap(), events);
    }

    fn keyed(key: &str, content: &str) -> Node {
        let mut node = text(content);
        let Node::Text { id, .. } = &mut node else {
            unreachable!()
        };
        *id = Some(ElementIdWire::Name(key.into()));
        node
    }

    fn uniform(path: Vec<ElementIdWire>) -> Node {
        Node::UniformList {
            id: ElementIdWire::Name("list".into()),
            path,
            route: 1,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            count: 1,
            measure_index: 0,
            sizing: list::UniformListSizing::Auto,
            horizontal_sizing: list::UniformListHorizontalSizing::FitList,
            y_flipped: false,
            scroll_request: None,
            indices: vec![0],
            children: vec![text("row")],
        }
    }

    /// `diff` then `apply` is the identity on the new tree, and the patches
    /// are the ones a reader expects: a changed field is `Props`, a moved
    /// key is `Move`, a new key `Insert`, a gone key `Remove`, and a node
    /// of another kind `Replace`.
    #[test]
    fn a_diff_applied_to_the_old_tree_is_the_new_tree() {
        let old = column(vec![
            keyed("a", "one"),
            keyed("b", "two"),
            keyed("c", "three"),
            Node::Container {
                id: Some(ElementIdWire::Name("box".into())),
                style: gpui::StyleRefinement::default(),
                interactivity: Interactivity::default(),
                children: vec![keyed("inner", "deep")],
            },
        ]);
        let mut new = column(vec![
            keyed("c", "three"),
            keyed("a", "one!"),
            keyed("d", "four"),
            Node::Container {
                id: Some(ElementIdWire::Name("box".into())),
                style: gpui::StyleRefinement::default(),
                interactivity: Interactivity::default(),
                children: vec![Node::empty()],
            },
        ]);
        let mut applied = old.clone();
        let patches = diff(&mut applied.clone(), &mut new.clone());
        let kinds: Vec<&str> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Replace { .. } => "replace",
                Patch::Props { .. } => "props",
                Patch::Insert { .. } => "insert",
                Patch::Remove { .. } => "remove",
                Patch::Move { .. } => "move",
            })
            .collect();
        assert_eq!(
            kinds,
            ["remove", "move", "props", "insert", "remove", "insert"],
            "{patches:#?}"
        );
        apply(&mut applied, patches).unwrap();
        assert_eq!(applied, new);
        // Nothing changed hands: both inputs of the diff are as they were.
        let mut untouched = old.clone();
        diff(&mut untouched, &mut new);
        assert_eq!(untouched, old);
        assert!(diff(&mut new.clone(), &mut new).is_empty());
    }

    #[test]
    fn a_patch_the_tree_cannot_take_is_refused() {
        let tree = column(vec![keyed("a", "one")]);
        let refused = |patch: Patch| apply(&mut tree.clone(), vec![patch]).unwrap_err();
        assert_eq!(
            refused(Patch::Remove {
                path: vec![7],
                index: 0
            }),
            "a path to no node"
        );
        assert_eq!(
            refused(Patch::Remove {
                path: vec![],
                index: 1
            }),
            "an index past the list"
        );
        assert_eq!(
            refused(Patch::Insert {
                path: vec![0],
                index: 0,
                node: Node::empty()
            }),
            "a list edit on no list"
        );
        assert_eq!(
            refused(Patch::Props {
                path: vec![0],
                node: button(ButtonContent::Child(Box::new(Node::empty())))
            }),
            "props of another arity"
        );
        let many = vec![
            Patch::Move {
                path: vec![],
                from: 0,
                to: 0
            };
            MAX_PATCHES + 1
        ];
        assert_eq!(
            apply(&mut tree.clone(), many).unwrap_err(),
            "more patches than the host applies"
        );
    }

    /// A patch is bounded like a tree: the result of applying it is inside
    /// every limit, however the patches were shaped.
    #[test]
    fn an_applied_patch_frame_is_a_sanitized_tree() {
        let mut tree = column(
            (0..MAX_NODES - 1)
                .map(|i| keyed(&i.to_string(), "x"))
                .collect(),
        );
        sanitize_tree(&mut tree).unwrap();
        assert_eq!(tree.count(), MAX_NODES);
        let mut deep = keyed("0", "leaf");
        for _ in 0..MAX_DEPTH {
            deep = column(vec![deep]);
        }
        apply(
            &mut tree,
            vec![
                Patch::Insert {
                    path: vec![],
                    index: 0,
                    node: keyed("inserted", &"y".repeat(MAX_STRING_BYTES + 1)),
                },
                Patch::Replace {
                    path: vec![1],
                    node: deep,
                },
            ],
        )
        .unwrap();
        assert!(tree.count() <= MAX_NODES, "{}", tree.count());
        let Node::Container { children, .. } = &tree else {
            panic!()
        };
        // A new typed ID leaves every existing sibling identity unchanged.
        assert_eq!(children[0].key(), Some("inserted"));
        assert_eq!(children[2].key(), Some("1"));
        let Node::Text { content, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(content.len(), MAX_STRING_BYTES);
        let mut depth = 0;
        let mut node = &children[1];
        while let Node::Container { children, .. } = node {
            depth += 1;
            node = &children[0];
        }
        assert!(depth <= MAX_DEPTH, "{depth}");
    }

    #[test]
    fn a_well_behaved_frame_is_untouched() {
        let mut frame = Frame {
            root: Some(column(vec![text("hello")])),
            ..Frame::default()
        };
        let before = frame.clone();
        sanitize(&mut frame).unwrap();
        assert_eq!(frame, before);
    }

    #[test]
    fn a_frame_past_the_text_budget_keeps_its_head_and_loses_its_tail() {
        const NODES: usize = 8;
        const EACH: usize = MAX_TEXT_BYTES_PER_FRAME / 4;

        let children = sanitized_children(column(
            (0..NODES).map(|_| text(&"é".repeat(EACH / 2))).collect(),
        ));
        let shaped: Vec<usize> = children
            .iter()
            .map(|child| match child {
                Node::Text { content, .. } => content.len(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(shaped.iter().sum::<usize>(), MAX_TEXT_BYTES_PER_FRAME);
        assert_eq!(&shaped[..4], &[EACH; 4]);
        assert_eq!(&shaped[4..], &[0; NODES - 4]);
    }

    #[test]
    fn display_truncation_shortens_a_placeholder_and_never_a_document_reference() {
        let long = "x".repeat(2 * MAX_TEXT_BYTES_PER_FRAME);
        let document = document_reference(
            "app:draft",
            editor_document::MAX_EDITOR_DOCUMENT_BYTES as u32,
        );
        let mut frame = Frame {
            root: Some(column(vec![
                editor("App/e", &long, document.clone()),
                text(&long),
            ])),
            ..Frame::default()
        };
        assert_eq!(
            sanitize(&mut frame),
            Ok(SanitizeReport {
                display_text_truncated: true
            })
        );
        let Some(Node::Container { children, .. }) = &frame.root else {
            panic!()
        };
        let Node::Editor {
            placeholder,
            document: kept,
            ..
        } = &children[0]
        else {
            panic!()
        };
        assert!(
            placeholder.len() < long.len(),
            "an editor placeholder is display text and spends the frame budget"
        );
        assert_eq!(
            kept, &document,
            "a 1 MiB document crosses as an exact reference, not as truncated text"
        );
    }

    #[test]
    fn repeated_editor_bindings_must_describe_one_identical_document() {
        let document = document_reference("app:draft", 32);
        let mut agreeing = Frame {
            root: Some(column(vec![
                editor("App/one", "a", document.clone()),
                editor("App/two", "b", document.clone()),
            ])),
            ..Frame::default()
        };
        assert_eq!(sanitize(&mut agreeing), Ok(SanitizeReport::default()));
        // A distinct logical document is independent, even at the same state.
        let mut independent = Frame {
            root: Some(column(vec![
                editor("App/one", "a", document.clone()),
                editor("App/two", "b", document_reference("app:notes", 32)),
            ])),
            ..Frame::default()
        };
        assert_eq!(sanitize(&mut independent), Ok(SanitizeReport::default()));
        let mut stale = document.clone();
        stale.revision -= 1;
        let mut conflicting = Frame {
            root: Some(column(vec![
                editor("App/one", "a", document),
                editor("App/two", "b", stale),
            ])),
            ..Frame::default()
        };
        assert_eq!(
            sanitize(&mut conflicting),
            Err("invalid editor document references or budget")
        );
    }

    #[test]
    fn a_document_projection_cannot_be_silently_removed_by_the_node_budget() {
        let mut deep = editor("App/e", "notes", document_reference("app:draft", 8));
        for _ in 0..MAX_DEPTH {
            deep = column(vec![deep]);
        }
        let mut frame = Frame {
            root: Some(deep),
            ..Frame::default()
        };
        assert_eq!(
            sanitize(&mut frame),
            Err("frame budget would remove an editor document projection")
        );
    }

    #[test]
    fn every_shaped_string_spends_the_same_budget() {
        let long = "x".repeat(MAX_TEXT_BYTES_PER_FRAME);
        let children = sanitized_children(column(vec![
            Node::Input {
                options: Default::default(),
                id: ElementIdWire::Name("App/i".into()),
                placeholder: long.clone(),
                value: long.clone(),
                on_input: 0,
                on_submit: None,
                secure: false,
                style: gpui::StyleRefinement::default(),
            },
            Node::Button {
                checked: None,
                expanded: None,
                selected: None,
                role: None,
                description: Some("Details".into()),
                key: "App/b".into(),
                content: ButtonContent::Label(long.clone()),
                label: Some(long),
                on_press: None,
                width: None,
                height: None,
                padding: None,
                style: ButtonStyle::default(),
            },
            text("tail"),
        ]));
        let Node::Input {
            placeholder, value, ..
        } = &children[0]
        else {
            panic!()
        };
        // The placeholder took the frame's whole budget and everything
        // shaped after it came out empty. The accessible name is not shaped,
        // so it answers to the per-string cap alone.
        assert_eq!(placeholder.len(), MAX_TEXT_BYTES_PER_FRAME);
        assert!(value.is_empty());
        let Node::Button {
            content,
            label,
            description,
            ..
        } = &children[1]
        else {
            panic!()
        };
        assert_eq!(*content, ButtonContent::Label(String::new()));
        assert_eq!(description.as_deref(), Some(""));
        assert_eq!(label.as_deref().map(str::len), Some(MAX_STRING_BYTES));
        assert_eq!(children[2], text(""));
    }

    #[test]
    fn a_hostile_frame_is_pulled_into_range() {
        let mut deep = text("leaf");
        for _ in 0..MAX_DEPTH + 10 {
            deep = column(vec![deep]);
        }
        let wide = column((0..MAX_NODES + 5).map(|_| text("x")).collect());
        let root = sanitized_root(column(vec![
            Node::Text {
                id: Some(ElementIdWire::Name("k".repeat(MAX_STRING_BYTES).into())),
                style: gpui::StyleRefinement::default(),
                content: "é".repeat(MAX_STRING_BYTES),
                heading: None,
                live: None,
            },
            deep,
            wide,
        ]));
        // A container whose child fell past the budget keeps an empty
        // stand-in, one per level at most.
        assert!(root.count() <= MAX_NODES + MAX_DEPTH, "{}", root.count());
        let Node::Container { children, .. } = &root else {
            panic!()
        };
        let Node::Text { id, content, .. } = &children[0] else {
            panic!("{:?}", children[0])
        };
        assert_eq!(
            id.as_ref().and_then(ElementIdWire::name).unwrap().len(),
            MAX_STRING_BYTES
        );
        assert!(content.len() <= MAX_STRING_BYTES && content.is_char_boundary(content.len()));
    }

    #[test]
    fn oversized_typed_identity_is_refused_whole_instead_of_truncated() {
        let mut frame = Frame {
            root: Some(keyed(&"k".repeat(MAX_STRING_BYTES + 3), "text")),
            ..Default::default()
        };
        assert_eq!(
            sanitize(&mut frame).unwrap_err(),
            "element identity name is too long"
        );
    }

    #[test]
    fn depth_is_cut_before_the_host_recurses_into_it() {
        let mut deep = text("leaf");
        for _ in 0..MAX_DEPTH * 2 {
            deep = column(vec![deep]);
        }
        let root = sanitized_root(deep);
        let mut depth = 0;
        let mut node = &root;
        while let Node::Container { children, .. } = node {
            depth += 1;
            node = &children[0];
        }
        assert!(depth <= MAX_DEPTH, "{depth}");
    }

    fn button(content: ButtonContent) -> Node {
        Node::Button {
            checked: None,
            expanded: None,
            selected: None,
            role: None,
            description: None,
            key: "App/b".into(),
            content,
            label: None,
            on_press: Some(1),
            width: None,
            height: None,
            padding: None,
            style: ButtonStyle::default(),
        }
    }

    /// A button holding a node is the third way the tree recurses, and the
    /// only one that hangs off an enum's field rather than a struct's.
    #[test]
    fn a_button_holding_a_node_round_trips_and_counts_as_a_child() {
        let frame = Frame {
            root: Some(button(ButtonContent::Child(Box::new(text("inside"))))),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);

        let mut nested = Node::empty();
        for _ in 0..MAX_DEPTH + 1 {
            nested = button(ButtonContent::Child(Box::new(nested)));
        }
        let bytes = encode(&Frame {
            root: Some(nested),
            ..Frame::default()
        });
        assert!(decode::<Frame>(&bytes).is_err());
    }

    fn mouse_area(key: &str, on_move: Option<u32>, content: Node) -> Node {
        Node::MouseArea {
            key: key.into(),
            role: None,
            label: None,
            expanded: None,
            selected: None,
            checked: None,
            on_press: Some(1),
            on_release: None,
            on_double_click: None,
            on_right_press: None,
            on_right_release: None,
            on_middle_press: None,
            on_middle_release: None,
            on_enter: Some(2),
            on_exit: None,
            on_move,
            on_press_at: None,
            on_scroll: Some(3),
            content: Box::new(content),
        }
    }

    /// A mouse area recurses like a container, is diffed as a node with
    /// one fixed child, and claims its key like every other node.
    #[test]
    fn a_mouse_area_round_trips_diffs_by_props_and_claims_its_key() {
        let frame = Frame {
            root: Some(mouse_area("App/m", Some(0), text("inside"))),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
        assert_eq!(frame.root.as_ref().unwrap().count(), 2);

        // A changed route index is a `Props` patch that keeps the child.
        let mut old = mouse_area("App/m", Some(0), text("inside"));
        let mut new = mouse_area("App/m", Some(7), text("inside"));
        let patches = diff(&mut old, &mut new);
        assert!(
            matches!(patches.as_slice(), [Patch::Props { path, .. }] if path.is_empty()),
            "{patches:?}"
        );
        apply(&mut old, patches).unwrap();
        assert_eq!(old, new);

        // Two areas on one key: the second is moved off it, its child kept.
        let children = sanitized_children(column(vec![
            mouse_area("App/m", None, text("a")),
            mouse_area("App/m", None, text("b")),
        ]));
        assert_eq!(children[1].key(), Some("App/m#2"));
        assert_eq!(children[1].children().len(), 1);
    }

    #[test]
    fn control_labels_round_trip() {
        let mut notes = editor("App/e", "Notes", document_reference("app:draft", 9));
        let Node::Editor { label, .. } = &mut notes else {
            unreachable!()
        };
        *label = Some("Notes".into());
        let frame = Frame {
            root: Some(column(vec![
                notes,
                Node::Slider {
                    key: "App/s".into(),
                    label: Some("Volume".into()),
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    on_change: 1,
                    on_release: None,
                    axis: Axis::Row,
                    width: None,
                    height: None,
                    style: SliderStyle::default(),
                },
                Node::ComboBox {
                    key: "App/c".into(),
                    state_key: "App/c".into(),
                    options: vec!["Serif".into()],
                    selected: None,
                    reset: 0,
                    placeholder: String::new(),
                    label: Some("Font".into()),
                    on_select: 2,
                    width: None,
                    settings: Box::default(),
                },
                Node::PickList {
                    settings: Default::default(),
                    key: "App/p".into(),
                    options: vec!["Dark".into()],
                    selected: Some(0),
                    placeholder: None,
                    label: Some("Theme".into()),
                    on_select: 3,
                    width: None,
                    style: PickListStyle::default(),
                },
            ])),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn a_mouse_areas_role_name_and_state_round_trip() {
        let mut area = mouse_area("App/m", None, text("inside"));
        let Node::MouseArea {
            role,
            label,
            expanded,
            selected,
            checked,
            ..
        } = &mut area
        else {
            unreachable!()
        };
        *role = Some(Role::Checkbox);
        *label = Some("Wrap lines".into());
        *expanded = Some(false);
        *selected = Some(true);
        *checked = Some(true);
        let frame = Frame {
            root: Some(area),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn a_selected_button_round_trips() {
        let mut tab = button(ButtonContent::Label("Inbox".into()));
        let Node::Button { selected, .. } = &mut tab else {
            unreachable!()
        };
        *selected = Some(true);
        let frame = Frame {
            root: Some(tab),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn a_buttons_role_round_trips() {
        let mut link = button(ButtonContent::Label("Docs".into()));
        let Node::Button { role, .. } = &mut link else {
            unreachable!()
        };
        *role = Some(Role::Link);
        let frame = Frame {
            root: Some(link),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn a_texts_heading_and_live_region_round_trip() {
        let mut title = text("Inbox");
        let Node::Text { heading, live, .. } = &mut title else {
            unreachable!()
        };
        *heading = Some(1);
        *live = Some(Live::Assertive);
        let mut status = text("3 new");
        let Node::Text { live, .. } = &mut status else {
            unreachable!()
        };
        *live = Some(Live::Polite);
        let frame = Frame {
            root: Some(column(vec![title, status])),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn an_overlays_label_round_trips() {
        let frame = Frame {
            root: Some(Node::Overlay {
                key: "App/ask".into(),
                label: Some("Delete page".into()),
                padding: 16.0,
                backdrop: Rgba([0.0, 0.0, 0.0, 0.4]),
                align_x: AlignX::Center,
                align_y: AlignY::Center,
                on_dismiss: Some(4),
                children: vec![text("base"), text("Delete this page?")],
            }),
            ..Frame::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }

    #[test]
    fn a_heading_level_outside_1_to_6_is_no_heading() {
        for (level, kept) in [
            (0, None),
            (1, Some(1)),
            (6, Some(6)),
            (7, None),
            (255, None),
        ] {
            let mut node = text("Title");
            let Node::Text { heading, .. } = &mut node else {
                unreachable!()
            };
            *heading = Some(level);
            let Node::Text { heading, .. } = sanitized_root(node) else {
                panic!("still text")
            };
            assert_eq!(heading, kept, "level {level}");
        }
    }

    /// Building and encoding a chain this deep recurses as far as decoding
    /// it would, so the hostile frame is made where there is stack for it.
    fn deep_chain_bytes(depth: usize) -> Vec<u8> {
        std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn(move || {
                let mut node = Node::empty();
                for _ in 0..depth {
                    node = Node::Lazy {
                        key: String::new(),
                        generation: 0,
                        content: Box::new(node),
                    };
                }
                let frame = Frame {
                    root: Some(node),
                    ..Frame::default()
                };
                let bytes = encode(&frame);
                // Dropping it recurses too, and this thread is the one with
                // the stack to do it.
                drop(frame);
                bytes
            })
            .expect("hostile frame thread")
            .join()
            .expect("hostile frame")
    }

    #[test]
    fn a_tree_the_host_would_not_walk_is_refused_before_it_is_built() {
        assert!(decode::<Frame>(&deep_chain_bytes(MAX_DEPTH - 1)).is_ok());
        let refused = decode::<Frame>(&deep_chain_bytes(MAX_DEPTH + 1)).unwrap_err();
        assert!(
            refused.contains("deeper than the host renders"),
            "{refused}"
        );
    }

    /// The bug this guards: a chain of a few thousand containers is a frame
    /// of ~100 KB — far inside any byte cap a host sets — and decoding it
    /// walked a host thread off its stack, aborting the process. A refusal
    /// is a message in one app's window; an overflow is every window gone.
    #[test]
    fn a_chain_that_overflowed_the_host_stack_is_an_error_not_a_crash() {
        let bytes = deep_chain_bytes(5_000);
        assert!(bytes.len() < 1 << 20, "{} bytes", bytes.len());
        assert!(decode::<Frame>(&bytes).is_err());
    }

    #[test]
    fn more_nodes_than_the_host_holds_is_refused() {
        let wide = column((0..MAX_DECODED_NODES + 2).map(|_| Node::empty()).collect());
        let bytes = encode(&Frame {
            root: Some(wide),
            ..Frame::default()
        });
        let refused = decode::<Frame>(&bytes).unwrap_err();
        assert!(
            refused.contains("more nodes than the host holds"),
            "{refused}"
        );
    }

    /// A frame is bytes a module wrote, so every byte of it is the guest's
    /// to choose. Whatever they say, `decode` answers rather than aborts.
    #[test]
    fn bytes_a_hostile_guest_could_write_are_answered_not_survived() {
        let sound = encode(&Frame {
            upstream_sanitization: Default::default(),
            editor_decisions: Vec::new(),
            editor_documents: Vec::new(),
            tooltip_responses: Vec::new(),
            mouse_interest: false,
            event_interest: Default::default(),
            root: Some(column(vec![text("hello"), Node::empty()])),
            requests: vec![Request {
                id: 7,
                kind: "host.echo".into(),
                payload: b"hi".to_vec(),
            }],
            cancels: vec![1, 2],
            unchanged: false,
            busy: false,
            patches: Vec::new(),
        });
        for cut in 0..sound.len() {
            let _ = decode::<Frame>(&sound[..cut]);
        }
        for at in 0..sound.len() {
            for bit in 0..8 {
                let mut flipped = sound.clone();
                flipped[at] ^= 1 << bit;
                let _ = decode::<Frame>(&flipped);
            }
        }
    }

    #[test]
    fn duplicate_typed_ids_are_refused_instead_of_silently_renamed() {
        let mut frame = Frame {
            root: Some(column(vec![keyed("same", "one"), keyed("same", "two")])),
            ..Default::default()
        };
        assert_eq!(
            sanitize(&mut frame).unwrap_err(),
            "duplicate typed element identity among siblings"
        );
        let mut root = column(vec![keyed("same", "one")]);
        let error = apply(
            &mut root,
            vec![Patch::Insert {
                path: vec![],
                index: 1,
                node: keyed("same", "two"),
            }],
        )
        .unwrap_err();
        assert_eq!(error, "duplicate typed element identity among siblings");
    }

    #[test]
    fn uniform_list_path_must_match_its_typed_tree_ancestry() {
        let parent = ElementIdWire::Integer(7);
        let list = ElementIdWire::Name("list".into());
        let mut valid = Frame {
            root: Some(Node::Container {
                id: Some(parent.clone()),
                style: gpui::StyleRefinement::default(),
                interactivity: Interactivity::default(),
                children: vec![uniform(vec![parent.clone(), list.clone()])],
            }),
            ..Default::default()
        };
        sanitize(&mut valid).unwrap();

        let Node::Container { children, .. } = valid.root.as_mut().unwrap() else {
            unreachable!()
        };
        let Node::UniformList { path, .. } = &mut children[0] else {
            unreachable!()
        };
        *path = vec![ElementIdWire::Name("forged-parent".into()), list];
        assert_eq!(
            sanitize(&mut valid).unwrap_err(),
            "uniform-list authored path is invalid"
        );
    }

    /// A hostile screen cannot cause quadratic identity repair or alias state.
    #[test]
    fn a_screen_of_one_typed_id_is_refused_in_linear_time() {
        let mut frame = Frame {
            root: Some(column(
                (0..MAX_NODES - 1).map(|_| keyed("same", "x")).collect(),
            )),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        assert_eq!(
            sanitize(&mut frame).unwrap_err(),
            "duplicate typed element identity among siblings"
        );
        if cfg!(not(debug_assertions)) {
            assert!(started.elapsed() < std::time::Duration::from_millis(200));
        }
    }

    #[test]
    fn sensor_reset_values_share_the_frame_budget() {
        let sensor = |key: &str| Node::Sensor {
            key: key.into(),
            reset: Some(SurfaceValue::List(vec![SurfaceValue::Unit; 3000])),
            on_show: Some(3),
            on_resize: None,
            on_hide: None,
            anticipate: None,
            delay: None,
            child: Box::new(text("child")),
        };
        // Not `sanitized_children`: the frame itself is asserted on below.
        let mut frame = Frame {
            root: Some(column(vec![sensor("first"), sensor("second")])),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        let Some(Node::Container { children, .. }) = &frame.root else {
            panic!("column retained")
        };
        for (index, node) in children.iter().enumerate() {
            let Node::Sensor {
                key,
                reset,
                on_show,
                child,
                ..
            } = node
            else {
                panic!("sensor retained")
            };
            assert_eq!(
                reset.is_some(),
                index == 0,
                "individually valid reset values must share one frame budget"
            );
            assert_eq!(key, if index == 0 { "first" } else { "second" });
            assert_eq!(*on_show, Some(3));
            assert!(matches!(&**child, Node::Text { content, .. } if content == "child"));
        }
        assert!(
            decode::<Frame>(&encode(&frame)).is_ok(),
            "sanitized aggregate fits the decoder budget"
        );
    }

    #[test]
    fn a_sensor_is_pulled_into_range_and_keeps_its_child() {
        let root = sanitized_root(Node::Sensor {
            key: "App/watch".into(),
            reset: None,
            on_show: Some(0),
            on_resize: Some(0),
            on_hide: Some(1),
            anticipate: Some(f32::INFINITY),
            delay: Some(-5.0),
            child: Box::new(text("a")),
        });
        let Node::Sensor {
            anticipate,
            delay,
            child,
            ..
        } = &root
        else {
            panic!("{root:?}")
        };
        assert_eq!(*anticipate, Some(MAX_PIXELS));
        assert_eq!(*delay, Some(0.0));
        assert_eq!(**child, text("a"));
        assert_eq!(root.count(), 2);
    }

    /// The form controls: a menu is cut to `MAX_OPTIONS` with a selection
    /// past the cut dropped, and a slider's numbers are made finite but not
    /// clamped like a size — a value of a million is the app's to send.
    #[test]
    fn form_controls_are_pulled_into_range() {
        let children = sanitized_children(column(vec![
            Node::PickList {
                settings: Default::default(),
                key: "App/pick".into(),
                options: (0..MAX_OPTIONS + 3).map(|i| i.to_string()).collect(),
                selected: Some((MAX_OPTIONS + 1) as u32),
                placeholder: Some("é".repeat(MAX_STRING_BYTES)),
                label: None,
                on_select: 0,
                width: Some(Length::Fixed(f32::INFINITY)),
                style: PickListStyle::default(),
            },
            Node::Slider {
                key: "App/slide".into(),
                label: None,
                value: f32::NAN,
                min: f32::NEG_INFINITY,
                max: 1_000_000.0,
                step: f32::INFINITY,
                on_change: 1,
                on_release: None,
                axis: Axis::Row,
                width: None,
                height: None,
                style: SliderStyle::default(),
            },
            Node::Toggle {
                key: "App/pick".into(),
                kind: ToggleKind::Switch,
                label: "x".repeat(MAX_STRING_BYTES + 1),
                checked: true,
                on_toggle: None,
                width: None,
                style: ToggleStyle::default(),
            },
        ]));
        let Node::PickList {
            options,
            selected,
            placeholder,
            width,
            ..
        } = &children[0]
        else {
            panic!("{:?}", children[0])
        };
        assert_eq!(options.len(), MAX_OPTIONS);
        assert_eq!(*selected, None);
        assert!(placeholder.as_ref().unwrap().len() <= MAX_STRING_BYTES);
        assert_eq!(*width, Some(Length::Fixed(MAX_PIXELS)));
        let Node::Slider {
            value,
            min,
            max,
            step,
            ..
        } = &children[1]
        else {
            panic!("{:?}", children[1])
        };
        assert_eq!(
            (*value, *min, *max, *step),
            (0.0, f32::MIN, 1_000_000.0, f32::MAX)
        );
        let Node::Toggle { key, label, .. } = &children[2] else {
            panic!("{:?}", children[2])
        };
        assert_eq!(key, "App/pick#2");
        assert!(label.len() <= MAX_STRING_BYTES);
    }

    /// A grid is bounded like a linear layout: its numbers are pulled into
    /// the pixel range and children past the node budget are dropped, not
    /// stood in for.
    #[test]
    fn a_container_is_pulled_into_range_and_cut_like_a_layout() {
        let root = sanitized_root(Node::Container {
            id: Some(ElementIdWire::Name("App/cells".into())),
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
            children: (0..MAX_NODES + 5).map(|_| text("x")).collect(),
        });
        let Node::Container { children, .. } = &root else {
            panic!()
        };
        assert_eq!(children.len(), MAX_NODES - 1);
        assert_eq!(root.count(), MAX_NODES);
    }

    fn picture(bytes: Option<Vec<u8>>) -> Node {
        Node::Svg {
            id: None,
            source: SvgSource::Data { hash: 7, bytes },
            transformation: SvgTransformation {
                scale: [1., 1.],
                translate: [0., 0.],
                rotate: 0.,
            },
            label: None,
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
        }
    }

    #[test]
    fn svg_and_raster_images_share_the_frame_picture_budget() {
        let image = Node::Image {
            id: Some(ElementIdWire::Name("App/raster".into())),
            hash: 8,
            data: Some(ImageData::Encoded(vec![
                0;
                MAX_PICTURE_BYTES_PER_FRAME / 2 + 1
            ])),
            label: None,
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::Contain,
            },
            loading: false,
            fallback: false,
            state_children: vec![],
            style: gpui::StyleRefinement::default(),
            interactivity: Interactivity::default(),
        };
        let mut frame = Frame {
            root: Some(column(vec![
                picture(Some(vec![0; MAX_PICTURE_BYTES_PER_FRAME / 2])),
                image,
            ])),
            ..Frame::default()
        };
        sanitize(&mut frame).unwrap();
        let children = frame.root.as_ref().unwrap().children();
        assert!(matches!(
            children[0],
            Node::Svg {
                source: SvgSource::Data { bytes: Some(_), .. },
                ..
            }
        ));
        assert!(
            matches!(children[1], Node::Image { data: None, .. }),
            "SVG consumption must reduce raster admission"
        );
        assert!(decode::<Frame>(&encode(&frame)).is_ok());
    }

    #[test]
    fn primitive_geometry_and_state_children_are_bounded_in_the_main_walk() {
        let image = Node::Image {
            id: Some(ElementIdWire::Integer(1)),
            hash: 1,
            data: Some(ImageData::Refusal("x".repeat(MAX_STRING_BYTES + 1))),
            label: None,
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::Contain,
            },
            loading: true,
            fallback: true,
            state_children: vec![text("loading"), text("fallback"), text("extra")],
            style: Default::default(),
            interactivity: Default::default(),
        };
        let svg = Node::Svg {
            id: Some(ElementIdWire::Integer(2)),
            source: SvgSource::External("x".repeat(MAX_STRING_BYTES + 1)),
            transformation: SvgTransformation {
                scale: [f32::NAN, f32::INFINITY],
                translate: [f32::NEG_INFINITY, f32::INFINITY],
                rotate: f32::NAN,
            },
            label: None,
            style: Default::default(),
            interactivity: Default::default(),
        };
        let anchored = Node::Anchored {
            anchor: Anchor::TopLeft,
            fit: AnchoredFitMode::SnapToWindowWithMargin([f32::NAN, f32::INFINITY, -1., 4.]),
            position: Some([f32::INFINITY, f32::NEG_INFINITY]),
            position_mode: AnchoredPositionMode::Local,
            offset: Some([f32::NAN, 3.]),
            children: vec![Node::Deferred {
                priority: usize::MAX,
                content: Box::new(text("child")),
            }],
        };
        let children = sanitized_children(column(vec![image, svg, anchored]));
        let Node::Image {
            data: Some(ImageData::Refusal(reason)),
            state_children,
            ..
        } = &children[0]
        else {
            panic!()
        };
        assert!(reason.len() <= MAX_STRING_BYTES);
        assert_eq!(state_children.len(), 2);
        let Node::Svg {
            source: SvgSource::External(path),
            transformation,
            ..
        } = &children[1]
        else {
            panic!()
        };
        assert!(path.len() <= MAX_STRING_BYTES);
        assert_eq!(transformation.scale, [0., MAX_PIXELS]);
        assert_eq!(transformation.translate, [-MAX_PIXELS, MAX_PIXELS]);
        assert_eq!(transformation.rotate, 0.);
        let Node::Anchored {
            fit,
            position,
            offset,
            children,
            ..
        } = &children[2]
        else {
            panic!()
        };
        assert_eq!(*position, Some([MAX_PIXELS, -MAX_PIXELS]));
        assert_eq!(*offset, Some([0., 3.]));
        assert_eq!(
            *fit,
            AnchoredFitMode::SnapToWindowWithMargin([0., MAX_PIXELS, 0., 4.])
        );
        assert!(matches!(children[0], Node::Deferred { priority: 16, .. }));
    }

    /// A picture past what is left of the frame's budget is dropped whole,
    /// never cut: half an SVG is not an SVG. The head of the frame keeps
    /// its pictures; a hash without bytes passes as the reference it is.
    #[test]
    fn a_frame_past_the_picture_budget_drops_whole_pictures_from_its_tail() {
        const EACH: usize = MAX_PICTURE_BYTES_PER_FRAME / 4 * 3;
        let children = sanitized_children(column(vec![
            picture(Some(vec![b'<'; EACH])),
            picture(Some(vec![b'<'; EACH])),
            picture(None),
            picture(Some(vec![b'<'; MAX_PICTURE_BYTES_PER_FRAME / 4])),
        ]));
        let carried: Vec<Option<usize>> = children
            .iter()
            .map(|child| match child {
                Node::Svg {
                    source: SvgSource::Data { bytes, .. },
                    ..
                } => bytes.as_ref().map(Vec::len),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            carried,
            [
                Some(EACH),
                None,
                None,
                Some(MAX_PICTURE_BYTES_PER_FRAME / 4)
            ]
        );
    }

    #[test]
    fn gpui_text_keeps_style_refinement_on_the_wire() {
        let text = Node::Text {
            id: Some(ElementIdWire::Name("text".into())),
            style: gpui::StyleRefinement::default(),
            content: "huge".into(),
            heading: None,
            live: None,
        };
        assert_eq!(decode::<Node>(&encode(&text)).unwrap(), text);
    }
}
