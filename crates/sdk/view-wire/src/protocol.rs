use crate::*;
use serde::{Deserialize, Serialize};

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
        /// Native InteractiveText byte index; ordinary tooltips use None.
        character_index: Option<u32>,
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
        #[serde(with = "response")]
        result: Result<Vec<u8>, Error>,
        done: bool,
    },
    /// The host no longer holds the tree the guest is patching — a patch it
    /// could not apply, a tree it dropped — and wants the next frame whole.
    Resync,
}

/// Why a request failed, as the guest gets it: the module SDK's own
/// [`Error`] — a stable snake_case [`code`] to branch on and the refusing
/// module's `message`, verbatim — so an error a module wrote and one the
/// host wrote are the same type end to end. `Display` writes
/// `code: message`; a screen that wants the message alone reads it.
pub use ::guest::{Error, code};

/// `Error` derives borsh only (it is the module SDK's type), so the places it
/// crosses the MessagePack layer, [`Event::Response`] and a method's
/// `Vec<Error>`, spell its serde shape here: the same
/// `{"Ok": bytes} | {"Err": {"code", "message"}}` a derived `Result` writes.
#[derive(Serialize, Deserialize)]
#[serde(remote = "::guest::Error")]
struct ErrorDef {
    code: String,
    message: String,
}

mod response {
    use super::ErrorDef;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    enum ResultDef {
        Ok(Vec<u8>),
        Err(#[serde(with = "ErrorDef")] ::guest::Error),
    }

    pub fn serialize<S: serde::Serializer>(
        result: &Result<Vec<u8>, ::guest::Error>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match result {
            Ok(bytes) => ResultDef::Ok(bytes.clone()),
            Err(error) => ResultDef::Err(error.clone()),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Result<Vec<u8>, ::guest::Error>, D::Error> {
        Ok(match ResultDef::deserialize(deserializer)? {
            ResultDef::Ok(bytes) => Ok(bytes),
            ResultDef::Err(error) => Err(error),
        })
    }
}

/// `#[serde(with = "errors")]` for a `Vec<Error>`: each one as [`ErrorDef`].
pub(crate) mod errors {
    use super::ErrorDef;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Each(#[serde(with = "ErrorDef")] ::guest::Error);

    pub fn serialize<S: serde::Serializer>(
        errors: &[::guest::Error],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(errors.iter().cloned().map(Each))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<::guest::Error>, D::Error> {
        Ok(Vec::<Each>::deserialize(deserializer)?
            .into_iter()
            .map(|each| each.0)
            .collect())
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Axis {
    Column,
    Row,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScrollDirection {
    Vertical,
    Horizontal,
    Both,
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
