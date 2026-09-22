//! The widget tree itself: the [`Node`] enum every frame carries, and the
//! walks over it a host and the differ share.

use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ButtonContent {
    Label(String),
    #[serde(deserialize_with = "decode_child")]
    Child(Box<Node>),
}

/// What a [`Node::MouseArea`] or a [`Node::Button`] is to assistive
/// technology. Every other interactive node's variant is its role, and a
/// button without one is a button.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Button,
    Link,
    Tab,
    MenuItem,
    Row,
    Checkbox,
    Switch,
}

/// How assistive technology announces a change to a [`Node::Text`] it is
/// not focused on.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Live {
    /// When the reader is idle.
    Polite,
    /// At once, interrupting.
    Assertive,
}

/// The reference point used by the native GPUI anchored element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    TopCenter,
    BottomCenter,
    LeftCenter,
    RightCenter,
}

/// How an anchored child is kept inside the host viewport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnchoredFitMode {
    SnapToWindow,
    SnapToWindowWithMargin([f32; 4]),
    SwitchAnchor,
}

/// Coordinate space for an anchored position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchoredPositionMode {
    Window,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageObjectFit {
    Fill,
    Contain,
    Cover,
    ScaleDown,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageStyle {
    pub grayscale: bool,
    pub object_fit: ImageObjectFit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SvgSource {
    None,
    Data { hash: u64, bytes: Option<Vec<u8>> },
    Asset(String),
    External(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SvgTransformation {
    pub scale: [f32; 2],
    pub translate: [f32; 2],
    pub rotate: f32,
}

/// One widget. `key` is the node's identity across frames — the
/// accessibility path the compiler already computes (`App/content/count`)
/// — which the host uses for widget state (focus, caret, scroll) and for
/// the accessibility tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Node {
    /// A payload encoded and painted by the host.
    Qr { key: String, code: Qr, style: gpui::StyleRefinement },
    /// One native GPUI paragraph with optional interactive byte ranges.
    RichText {
        id: Option<ElementIdWire>,
        style: gpui::StyleRefinement,
        text: String,
        runs: RichTextRuns,
        font_family_overrides: Vec<(std::ops::Range<usize>, gpui::SharedString)>,
        clickable_ranges: Vec<std::ops::Range<usize>>,
        on_click: Option<u32>,
        on_hover: Option<u32>,
    },
    /// A native GPUI anchored element. The host owns fitting and clipping.
    Anchored {
        anchor: Anchor,
        fit: AnchoredFitMode,
        position: Option<[f32; 2]>,
        position_mode: AnchoredPositionMode,
        offset: Option<[f32; 2]>,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// Floating content the host offsets from its own origin.
    Float {
        key: String,
        x: f32,
        y: f32,
        scale: f32,
        shadow: Shadow,
        radius: Option<[f32; 4]>,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    /// A GPUI uniform-height list. The host owns the native viewport; the
    /// guest carries only the row indices the host has requested.
    UniformList {
        id: ElementIdWire,
        path: Vec<ElementIdWire>,
        route: u32,
        style: gpui::StyleRefinement,
        interactivity: Interactivity,
        count: usize,
        measure_index: usize,
        sizing: crate::list::UniformListSizing,
        horizontal_sizing: crate::list::UniformListHorizontalSizing,
        y_flipped: bool,
        scroll_request: Option<crate::list::UniformListScrollRequest>,
        #[serde(deserialize_with = "list::decode_indices")]
        indices: Vec<u32>,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// A native variable-height GPUI list with a bounded frame-owned row window.
    List {
        state: u64,
        #[serde(deserialize_with = "list::decode_path")]
        path: Vec<ElementIdWire>,
        item_count: usize,
        alignment: ListAlignment,
        overdraw: f32,
        sizing: ListSizingBehavior,
        following_tail: bool,
        revision: u64,
        #[serde(deserialize_with = "list::decode_commands")]
        commands: Vec<ListCommand>,
        request_handler: u32,
        scroll_handler: Option<u32>,
        range_start: usize,
        style: gpui::StyleRefinement,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    Container {
        /// Native GPUI identity, retained as a tagged adapter on the wire.
        id: Option<ElementIdWire>,
        /// The real GPUI style refinement, applied by the host's native Div.
        style: gpui::StyleRefinement,
        interactivity: Interactivity,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// A grabbed divider: local movement deltas and native cursor; one child.
    ResizeHandle {
        id: ElementIdWire,
        style: gpui::StyleRefinement,
        on_press: Option<u32>,
        on_release: Option<u32>,
        on_drag: Option<u32>,
        cursor: Option<mouse::Cursor>,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    /// A region that reports what the pointer does over its one child. The
    /// discrete routes carry per-frame message indices like a button's
    /// `on_press`; `on_move` and `on_press_at` carry a handler index the
    /// host answers with [`Event::Pointer`], `on_scroll` one it answers
    /// with [`Event::Scroll`]. The node paints nothing of its own.
    MouseArea {
        id: ElementIdWire,
        /// `None` is an area assistive technology does not announce.
        role: Option<Role>,
        /// The accessible name of an area no text inside names.
        label: Option<String>,
        expanded: Option<bool>,
        selected: Option<bool>,
        checked: Option<bool>,
        on_press: Option<u32>,
        on_release: Option<u32>,
        on_double_click: Option<u32>,
        on_right_press: Option<u32>,
        on_right_release: Option<u32>,
        on_middle_press: Option<u32>,
        on_middle_release: Option<u32>,
        on_enter: Option<u32>,
        on_exit: Option<u32>,
        on_move: Option<u32>,
        /// Fires for a left press even when the child took it — a button
        /// inside the area — where `on_press` does not.
        on_press_at: Option<u32>,
        on_scroll: Option<u32>,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    Tooltip {
        key: String,
        position: TooltipPosition,
        gap: f32,
        padding: f32,
        delay_ms: u64,
        snap: bool,
        style: TooltipStyle,
        /// Content followed by tip; extra children are discarded by sanitization.
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// Supplies widget-local dimensions to descendant container conditions.
    Responsive {
        id: ElementIdWire,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    /// A guest-memoized subtree. Generation changes whenever cached content or
    /// its callable routes are rebuilt, including a rebuild after eviction.
    Lazy {
        key: String,
        generation: u64,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    /// A deferred draw. Unlike [`Node::Lazy`], this is never a guest cache.
    Deferred {
        priority: usize,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    /// Splices selected children into the surrounding layout. It adds no box.
    When {
        key: String,
        condition: ContainerQuery,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// Watches its child's laid-out size. `on_show` hears the size when the
    /// child first comes into view (within `anticipate` pixels of it),
    /// `on_resize` every change after, both as [`Event::Size`]; `on_hide`
    /// is the message for leaving view. `delay` is milliseconds a size
    /// must hold before it is reported.
    Sensor {
        id: ElementIdWire,
        style: gpui::StyleRefinement,
        /// Copied continuity value for `key=`, independent of widget identity.
        reset: Option<SurfaceValue>,
        on_show: Option<u32>,
        on_resize: Option<u32>,
        on_hide: Option<u32>,
        anticipate: Option<f32>,
        delay: Option<f32>,
        #[serde(deserialize_with = "decode_child")]
        child: Box<Node>,
    },
    Scroll {
        on_scroll: Option<u32>,
        virtual_rows: bool,
        id: ElementIdWire,
        direction: ScrollDirection,
        width: Option<Length>,
        height: Option<Length>,
        /// No scroll bar is drawn; the content still scrolls.
        bar_hidden: bool,
        bar_width: Option<f32>,
        bar_margin: Option<f32>,
        scroller_width: Option<f32>,
        /// Space between the bar and the content, which shrinks the content.
        bar_spacing: Option<f32>,
        anchor_x: ScrollAnchor,
        anchor_y: ScrollAnchor,
        /// Follow content that grows while the reader sits at the end.
        auto_scroll: bool,
        background: Option<Rgba>,
        border: Option<Border>,
        #[serde(deserialize_with = "decode_child")]
        content: Box<Node>,
    },
    Text {
        id: Option<ElementIdWire>,
        style: gpui::StyleRefinement,
        content: String,
        /// A heading's level, 1 to 6; the sanitizer makes any other `None`.
        heading: Option<u8>,
        /// `None` is text whose changes are not announced.
        live: Option<Live>,
    },
    /// A raster picture sent once per typed content hash.
    Image {
        id: Option<ElementIdWire>,
        hash: u64,
        data: Option<ImageData>,
        label: Option<String>,
        image_style: ImageStyle,
        loading: bool,
        fallback: bool,
        #[serde(deserialize_with = "decode_children")]
        state_children: Vec<Node>,
        style: gpui::StyleRefinement,
        interactivity: Interactivity,
    },
    /// A native zoom/pan viewer sharing the raster picture cache and budgets.
    ImageViewer {
        id: ElementIdWire,
        hash: u64,
        data: Option<ImageData>,
        label: Option<String>,
        fit: Option<ContentFit>,
        style: gpui::StyleRefinement,
        options: ViewerOptions,
    },
    /// A vector picture. Its bytes cross ONCE: the frame that first shows a
    /// picture carries them under `hash`, and every frame after — a changed
    /// tree re-sends every node — names the hash alone. The host keeps what
    /// it decoded by hash for as long as the guest runs; a hash it has not
    /// seen draws as empty space of the node's size.
    Svg {
        id: Option<ElementIdWire>,
        source: SvgSource,
        transformation: SvgTransformation,
        label: Option<String>,
        style: gpui::StyleRefinement,
        interactivity: Interactivity,
    },
    Input {
        options: InputOptions,
        id: ElementIdWire,
        placeholder: String,
        /// Copied document state, adopted by reset and host observation revision.
        value: String,
        on_input: u32,
        on_submit: Option<u32>,
        secure: bool,
        style: gpui::StyleRefinement,
    },
    /// A multiline text editor. The host owns the `text_editor::Content` —
    /// native widget interaction — and the guest sees document state, unlike
    /// [`Node::Input`]. Presentation crosses as copied data.
    Editor {
        options: Box<EditorOptions>,
        id: ElementIdWire,
        style: gpui::StyleRefinement,
        placeholder: String,
        /// The accessible name.
        label: Option<String>,
        /// A shared logical document; its bytes travel only through a requested transfer.
        document: editor_document::EditorDocumentRef,
        /// Mutable guest state route, present even while editing is disabled.
        on_document: u32,
        editable: bool,
    },
    Button {
        key: String,
        content: ButtonContent,
        /// The accessible name of a button whose content is not a plain
        /// label.
        label: Option<String>,
        /// `None` is a button.
        role: Option<Role>,
        checked: Option<bool>,
        expanded: Option<bool>,
        selected: Option<bool>,
        description: Option<String>,
        /// `None` is a disabled button.
        on_press: Option<u32>,
        style: gpui::StyleRefinement,
    },
    Space {
        width: Option<Length>,
        height: Option<Length>,
    },
    Rule {
        key: String,
        axis: Axis,
        thickness: f32,
        color: Option<Rgba>,
        /// The theme's weak rule colour instead of its strong one, under
        /// `color` when both are given.
        weak: bool,
        /// top-left, top-right, bottom-right, bottom-left.
        radius: Option<[f32; 4]>,
        /// Round the rule to whole pixels; `None` is the host's default.
        snap: Option<bool>,
    },
    /// A checkbox or a toggler: a labelled bool.
    Toggle {
        key: String,
        kind: ToggleKind,
        label: String,
        checked: bool,
        /// `None` is a disabled control.
        on_toggle: Option<u32>,
        style: gpui::StyleRefinement,
    },
    /// One radio button. Its value is the guest's business: selecting it
    /// sends the message the guest queued for it.
    Radio {
        key: String,
        label: String,
        selected: bool,
        on_select: u32,
        style: gpui::StyleRefinement,
    },
    Slider {
        id: ElementIdWire,
        /// The accessible name.
        label: Option<String>,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        on_change: u32,
        on_release: Option<u32>,
        axis: Axis,
        style: gpui::StyleRefinement,
    },
    ComboBox {
        id: ElementIdWire,
        state_key: String,
        options: Vec<String>,
        selected: Option<u32>,
        reset: u64,
        placeholder: String,
        /// The accessible name.
        label: Option<String>,
        on_select: u32,
        style: gpui::StyleRefinement,
        settings: Box<ComboOptions>,
    },
    PickList {
        settings: Box<PickOptions>,
        id: ElementIdWire,
        /// Every option as the guest shows it; the host answers with an
        /// index into this list.
        options: Vec<String>,
        selected: Option<u32>,
        placeholder: Option<String>,
        /// The accessible name.
        label: Option<String>,
        on_select: u32,
        style: gpui::StyleRefinement,
    },
    Progress {
        key: String,
        value: f32,
        min: f32,
        max: f32,
        axis: Axis,
        style: gpui::StyleRefinement,
    },
    /// A base plus an optional modal layer. Closing removes the second child.
    Overlay {
        id: ElementIdWire,
        /// The accessible name of the dialog; the variant is its role.
        label: Option<String>,
        style: gpui::StyleRefinement,
        on_dismiss: Option<u32>,
        #[serde(deserialize_with = "decode_children")]
        children: Vec<Node>,
    },
    /// Bounded geometry painted by the host, in widget-local coordinates.
    Canvas {
        style: gpui::StyleRefinement,
        #[serde(deserialize_with = "canvas::decode_parts")]
        commands: Vec<CanvasCommand>,
    },
    /// A region the host paints itself: `name` picks a surface the
    /// embedding host registered, `args` are the typed values the guest
    /// hands it; `on_event` routes a returned value to its handler. The guest
    /// never sees what is drawn there, and the host repaints it on its own clock — a live video tile, a sweeping hand —
    /// without a guest tick. A name the host has not registered renders as
    /// a visible placeholder. It takes the size its parent gives it: wrap it
    /// in a sized [`Node::Container`] to set one.
    Surface {
        id: ElementIdWire,
        style: gpui::StyleRefinement,
        name: String,
        args: Vec<SurfaceValue>,
        on_event: Option<u32>,
    },
}

impl Node {
    /// The node an empty view renders as.
    pub fn empty() -> Self {
        Self::Space {
            width: None,
            height: None,
        }
    }

    /// Hashes the current copied subtree without allocating an encoded buffer.
    /// A host uses this after sanitization: shared frame budgets may change
    /// content even when a guest memo generation stays the same.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::Hasher;
        struct Sink(std::hash::DefaultHasher);
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.write(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(std::hash::DefaultHasher::new());
        rmp_serde::encode::write_named(&mut sink, self).expect("node fingerprint sink cannot fail");
        sink.0.finish()
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Container { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Svg { id, .. }
            | Self::RichText { id, .. } => id.as_ref().and_then(ElementIdWire::name),
            Self::Input { id, .. }
            | Self::Editor { id, .. }
            | Self::UniformList { id, .. }
            | Self::ResizeHandle { id, .. }
            | Self::MouseArea { id, .. }
            | Self::Sensor { id, .. }
            | Self::Responsive { id, .. }
            | Self::Scroll { id, .. }
            | Self::Overlay { id, .. }
            | Self::ImageViewer { id, .. }
            | Self::Slider { id, .. }
            | Self::PickList { id, .. }
            | Self::ComboBox { id, .. }
            | Self::Surface { id, .. } => id.name(),
            Self::Float { key, .. }
            | Self::Lazy { key, .. }
            | Self::When { key, .. }
            | Self::Qr { key, .. }
            | Self::Button { key, .. }
            | Self::Rule { key, .. }
            | Self::Toggle { key, .. }
            | Self::Radio { key, .. }
            | Self::Progress { key, .. }
            | Self::Tooltip { key, .. } => Some(key),
            Self::List { .. }
            | Self::Space { .. }
            | Self::Anchored { .. }
            | Self::Deferred { .. }
            | Self::Canvas { .. } => None,
        }
    }

    /// The node's identity without reducing a typed GPUI ID to text.
    pub fn identity(&self) -> Option<IdentityKeyRef<'_>> {
        match self {
            Self::Container { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Svg { id, .. }
            | Self::RichText { id, .. } => id.as_ref().map(IdentityKeyRef::Element),
            Self::Input { id, .. }
            | Self::Editor { id, .. }
            | Self::UniformList { id, .. }
            | Self::ResizeHandle { id, .. }
            | Self::MouseArea { id, .. }
            | Self::Sensor { id, .. }
            | Self::Responsive { id, .. }
            | Self::Scroll { id, .. }
            | Self::Overlay { id, .. }
            | Self::ImageViewer { id, .. }
            | Self::Slider { id, .. }
            | Self::PickList { id, .. }
            | Self::ComboBox { id, .. }
            | Self::Surface { id, .. } => Some(IdentityKeyRef::Element(id)),
            Self::Float { key, .. }
            | Self::Lazy { key, .. }
            | Self::When { key, .. }
            | Self::Qr { key, .. }
            | Self::Button { key, .. }
            | Self::Rule { key, .. }
            | Self::Toggle { key, .. }
            | Self::Radio { key, .. }
            | Self::Progress { key, .. }
            | Self::Tooltip { key, .. } => Some(IdentityKeyRef::Legacy(key)),
            Self::List { .. }
            | Self::Space { .. }
            | Self::Anchored { .. }
            | Self::Deferred { .. }
            | Self::Canvas { .. } => None,
        }
    }

    /// The node's children in order. One arm per variant, here and in
    /// [`Node::children_mut`] and [`Node::child_list_mut`]: everything that
    /// walks, diffs or patches a tree goes through these three, so a new
    /// variant is a new arm in each and nothing else.
    pub fn children(&self) -> &[Node] {
        match self {
            Self::Container { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Overlay { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            } => children,
            Self::Float { content, .. }
            | Self::Responsive { content, .. }
            | Self::Lazy { content, .. }
            | Self::Deferred { content, .. }
            | Self::Sensor { child: content, .. }
            | Self::ResizeHandle { content, .. }
            | Self::MouseArea { content, .. }
            | Self::Scroll { content, .. } => std::slice::from_ref(content),
            Self::Button {
                content: ButtonContent::Child(child),
                ..
            } => std::slice::from_ref(child),
            Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => &[],
        }
    }

    /// Runs `visit` on every node in the tree, depth first, this one first.
    pub fn for_each_mut(&mut self, visit: &mut impl FnMut(&mut Node)) {
        visit(self);
        for child in self.children_mut() {
            child.for_each_mut(visit);
        }
    }

    pub fn children_mut(&mut self) -> &mut [Node] {
        match self {
            Self::Container { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Overlay { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            } => children,
            Self::Float { content, .. }
            | Self::Responsive { content, .. }
            | Self::Lazy { content, .. }
            | Self::Deferred { content, .. }
            | Self::Sensor { child: content, .. }
            | Self::ResizeHandle { content, .. }
            | Self::MouseArea { content, .. }
            | Self::Scroll { content, .. } => std::slice::from_mut(content),
            Self::Button {
                content: ButtonContent::Child(child),
                ..
            } => std::slice::from_mut(child),
            Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => &mut [],
        }
    }

    /// The children as a list that can grow and shrink, for the variants
    /// that hold one; a fixed-arity node (a container's one content) has
    /// none, and no patch may insert into, remove from or move within it.
    pub fn child_list_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Self::Container { children, .. }
            | Self::List { children, .. }
            | Self::UniformList { children, .. }
            | Self::When { children, .. }
            | Self::Tooltip { children, .. }
            | Self::Anchored { children, .. }
            | Self::Image {
                state_children: children,
                ..
            }
            | Self::Overlay { children, .. } => Some(children),
            Self::Float { .. }
            | Self::Responsive { .. }
            | Self::Lazy { .. }
            | Self::Deferred { .. }
            | Self::Sensor { .. }
            | Self::ResizeHandle { .. }
            | Self::MouseArea { .. }
            | Self::Scroll { .. }
            | Self::Button { .. }
            | Self::Qr { .. }
            | Self::RichText { .. }
            | Self::Text { .. }
            | Self::Input { .. }
            | Self::Editor { .. }
            | Self::Space { .. }
            | Self::Rule { .. }
            | Self::Toggle { .. }
            | Self::Radio { .. }
            | Self::Slider { .. }
            | Self::PickList { .. }
            | Self::ComboBox { .. }
            | Self::Progress { .. }
            | Self::Svg { .. }
            | Self::ImageViewer { .. }
            | Self::Canvas { .. }
            | Self::Surface { .. } => None,
        }
    }

    /// Every node in the tree, depth first, this one included.
    pub fn count(&self) -> usize {
        1 + self.children().iter().map(Node::count).sum::<usize>()
    }
}
