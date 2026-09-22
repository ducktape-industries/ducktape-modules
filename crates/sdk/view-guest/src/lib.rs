//! Renderer-independent execution of dynamically loaded WASM views.
pub use view_wire as wire;
pub use gpui::{
    ClickEvent, ElementId, Global, Hsla, Role, SharedString, StyleRefinement, Styled, px, rems, rgb,
};
pub use view_guest_derive::IntoElement;
pub use gpui::prelude::FluentBuilder;
mod theme;
pub use theme::Theme;
mod element;
mod interactivity;
pub use interactivity::{Interactivity, InteractiveElement, Stateful, StatefulInteractiveElement};
pub use element::{
    anchored, canvas, deferred, div, img, svg, uniform_list, AnyElement, Anchored, Canvas, Deferred, Div,
    Img, IntoElement, Lowering, ParentElement, RenderOnce, Svg, UniformList,
};

/// Traits and primitives used to compose guest GPUI elements.
pub mod prelude {
    pub use crate::{
        AnyElement, App, ClickEvent, Context, ElementId, FluentBuilder, Global, Hsla, InteractiveElement,
        IntoElement, ParentElement, Render, RenderOnce, Role, SharedString, StatefulInteractiveElement,
        Styled, Theme, Window, anchored, canvas, deferred, div, img, px, rems, rgb, svg, uniform_list,
    };
}
mod editor;
mod editor_binding;
mod editor_documents;
pub use editor::Editor;
pub use editor_binding::{
    EditorBinding, EditorInteractionRequest, EditorKeyRequest, EditorRichRequest, EditorStateView,
    EditorTransaction, EditorTransactionEvent,
};
pub use editor_documents::EditorDocumentUpdate;
pub mod caps;
pub mod composer;
pub mod host;
pub mod testing;
pub mod widget;
pub mod window;

mod snapshot;
pub mod view;
pub use view::{Loaded, Render, View};
pub mod capabilities;
pub use capabilities::*;
mod context;
pub use context::{App, AsyncApp, Context, Entity, Released, WeakEntity};
mod executor;
pub use executor::Task;
pub use host::Host;
pub use window::Window;
mod slots;
use context::Callback;

const MAX_ROUNDS: usize = 8;

pub struct Driver<V: View> {
    app: App,
    entity: Entity<V>,
    last_root: Option<wire::Node>,
    busy: bool,
}
impl<V: View> Drop for Driver<V> {
    fn drop(&mut self) {
        self.app.inner.alive.set(false);
        self.app.inner.tasks.borrow_mut().clear();
    }
}
impl<V: View> Default for Driver<V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<V: View> Driver<V> {
    pub fn new() -> Self {
        Self::with_macos(cfg!(target_os = "macos"))
    }
    pub fn with_macos(macos: bool) -> Self {
        Self::initialize(macos, None).expect("view initializes")
    }
    pub(crate) fn initialize(macos: bool, restored: Option<V>) -> Result<Self, String> {
        Self::initialize_in(App::new(macos), restored)
    }
    pub(crate) fn initialize_in(mut app: App, restored: Option<V>) -> Result<Self, String> {
        let entity = Entity::reserve(&app);
        let mut window = app.window();
        let mut cx = Context {
            app: &mut app,
            entity: entity.clone(),
        };
        let value = match restored {
            Some(mut value) => {
                value.restored(&mut window, &mut cx);
                value
            }
            None => V::new(&mut window, &mut cx),
        };
        *entity.value.borrow_mut() = Some(value);
        Ok(Self {
            app,
            entity,
            last_root: None,
            busy: false,
        })
    }
    pub fn entity(&self) -> Entity<V> {
        self.entity.clone()
    }
    pub fn host(&self) -> Host {
        self.app.host()
    }
    pub(crate) fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }
    pub fn tick(&mut self, events: Vec<wire::Event>) -> wire::Frame {
        self.busy = false;
        self.settle();
        for event in events {
            let editor_event = matches!(
                &event,
                wire::Event::EditorDocument { .. }
                    | wire::Event::EditorRequest { .. }
                    | wire::Event::EditorTransaction { .. }
            );
            let message = match event {
                wire::Event::Observation { .. }
                | wire::Event::Mouse { .. }
                | wire::Event::Keyboard { .. } => None,
                wire::Event::Message(index) => {
                    slots::take_message::<Callback<V>>(&self.app.inner.slots, index)
                }
                wire::Event::Click { handler, event } => {
                    let slots = self.app.inner.slots.clone();
                    let mut window = self.app.window();
                    slots::run_click(&slots, handler, &event.into(), &mut window, &mut self.app);
                    None
                }
                wire::Event::Surface { handler, value } => slots::run_handler::<
                    wire::SurfaceValue,
                    Callback<V>,
                >(
                    &self.app.inner.slots, handler, value
                ),
                wire::Event::Input { handler, text } => {
                    slots::run_handler::<String, Callback<V>>(&self.app.inner.slots, handler, text)
                }
                wire::Event::EditorDocument { handler, message } => {
                    use wire::editor_document::EditorDocumentMessage;
                    if matches!(
                        message,
                        EditorDocumentMessage::Acknowledged { .. }
                            | EditorDocumentMessage::Failed { .. }
                    ) {
                        slots::finish_editor_transfer(&self.app.inner.slots, message.id());
                        continue;
                    }
                    slots::run_handler::<wire::editor_document::EditorDocumentMessage, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        message,
                    )
                }
                wire::Event::EditorRequest { handler, request } => {
                    slots::run_handler::<wire::EditorRequest, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        request,
                    )
                }
                wire::Event::EditorTransaction { handler, event } => {
                    if let wire::EditorTransactionEvent::Fault { id, .. }
                    | wire::EditorTransactionEvent::Cancelled { id, .. } = &event
                    {
                        slots::finish_editor_transfer(
                            &self.app.inner.slots,
                            &wire::editor_document::EditorTransferId {
                                instance: id.instance,
                                document: id.document.clone(),
                                reset: id.reset,
                                serial: id.sequence,
                                attempt: id.attempt,
                            },
                        );
                    }
                    if let wire::EditorTransactionEvent::Cancelled { id, .. } = &event {
                        if !slots::editor_matches_pending(&self.app.inner.slots, id) {
                            continue;
                        }
                        slots::editor_acknowledge(&self.app.inner.slots, &event);
                    }
                    slots::run_handler::<wire::EditorTransactionEvent, Callback<V>>(
                        &self.app.inner.slots,
                        handler,
                        event,
                    )
                }
                wire::Event::Toggle { handler, on } => {
                    slots::run_handler::<bool, Callback<V>>(&self.app.inner.slots, handler, on)
                }
                wire::Event::Slide { handler, value } => {
                    slots::run_handler::<f32, Callback<V>>(&self.app.inner.slots, handler, value)
                }
                wire::Event::Select { handler, index } => {
                    slots::run_handler::<u32, Callback<V>>(&self.app.inner.slots, handler, index)
                }
                wire::Event::Size {
                    handler,
                    width,
                    height,
                } => slots::run_handler::<(f32, f32), Callback<V>>(
                    &self.app.inner.slots,
                    handler,
                    (width, height),
                ),
                wire::Event::Drag { handler, dx, dy } => slots::run_handler::<
                    (f64, f64),
                    Callback<V>,
                >(
                    &self.app.inner.slots, handler, (dx, dy)
                ),
                wire::Event::Pointer { handler, x, y } => slots::run_handler::<
                    (f32, f32),
                    Callback<V>,
                >(
                    &self.app.inner.slots, handler, (x, y)
                ),
                wire::Event::Scroll {
                    handler,
                    dx,
                    dy,
                    pixels,
                } => slots::run_handler::<(f32, f32, bool), Callback<V>>(
                    &self.app.inner.slots,
                    handler,
                    (dx, dy, pixels),
                ),
                wire::Event::ScrollOffset {
                    handler,
                    x,
                    y,
                    relative_x,
                    relative_y,
                } => slots::run_handler::<(f32, f32, f32, f32), Callback<V>>(
                    &self.app.inner.slots,
                    handler,
                    (x, y, relative_x, relative_y),
                ),
                wire::Event::Theme { dark } => {
                    self.app
                        .set_global(if dark { Theme::dark() } else { Theme::light() });
                    self.app.notify();
                    None
                }
                wire::Event::Response { id, result, done } => {
                    self.app.host().fulfill(id, result, done);
                    // Response order is semantic: queued hidden data must be
                    // applied before a later visibility notification.
                    self.settle();
                    None
                }
                // The host dropped the tree the patches build on.
                wire::Event::Resync => {
                    self.last_root = None;
                    self.app.notify();
                    None
                }
            };
            if let Some(callback) = message {
                self.entity.clone().update_app(&mut self.app, |v, w, cx| {
                    callback(v, w, cx);
                    if editor_event {
                        cx.notify();
                    }
                });
                self.settle();
            }
        }
        self.settle();
        let render = self.app.inner.dirty.replace(false)
            || self.last_root.is_none()
            || slots::editor_transferring(&self.app.inner.slots);
        let mut root = if render {
            slots::reset(&self.app.inner.slots);
            let mut window = self.app.window();
            let element = {
                let mut cx = Context {
                    app: &mut self.app,
                    entity: self.entity.clone(),
                };
                self.entity
                    .value
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .render(&mut window, &mut cx)
            };
            element.into_node(&mut Lowering::new(&mut window, &mut self.app))
        } else {
            self.last_root.clone().expect("rendered tree")
        };
        self.busy |= self.app.inner.dirty.get() || executor::ready(&self.app.inner.tasks.borrow());
        let unchanged = self.last_root.as_ref() == Some(&root);
        let mut patches = Vec::new();
        if !unchanged {
            // Patches against the last tree, unless there is none — a first
            // frame or a resync. Property changes remain patches even for
            // tiny trees; replacing their identity would discard native state.
            // Structural edits may use a whole tree when that is smaller.
            if let Some(last) = &mut self.last_root {
                patches = wire::diff(last, &mut root);
                let only_props = patches.iter().all(|patch| matches!(patch, wire::Patch::Props { .. }));
                if patches.len() > wire::MAX_PATCHES
                    || (!only_props && wire::encoded_size(&patches) >= wire::encoded_size(&root))
                {
                    patches.clear();
                }
            }
            // Remembered without the picture bytes this frame carried: the
            // next view names those pictures by hash alone, and that is
            // the same tree — and the tree the host keeps, which drops the
            // bytes the same way once it has the pictures.
            let mut kept = root.clone();
            kept.for_each_mut(&mut |node| match node {
                wire::Node::Svg { bytes, .. } => *bytes = None,
                wire::Node::Image { data, .. } | wire::Node::ImageViewer { data, .. } => {
                    *data = None
                }
                _ => {}
            });
            self.last_root = Some(kept);
        }
        let editor_decisions = slots::take_editor_responses(&self.app.inner.slots);
        self.busy |= slots::editor_responses_ready(&self.app.inner.slots);
        wire::Frame {
            upstream_sanitization: Default::default(),
            editor_decisions,
            editor_documents: slots::take_editor_documents(&self.app.inner.slots),
            mouse_interest: slots::mouse_interest(&self.app.inner.slots),
            event_interest: slots::event_interest(&self.app.inner.slots),
            root: Some(root),
            patches,
            requests: self.app.host().drain_outbox(),
            cancels: self.app.host().drain_cancels(),
            unchanged,
            busy: self.busy,
        }
    }

    fn settle(&mut self) {
        for _ in 0..MAX_ROUNDS {
            let mut tasks = std::mem::take(&mut *self.app.inner.tasks.borrow_mut());
            let cut_short = executor::poll(&mut tasks);
            self.app.refresh_globals();
            let added = !self.app.inner.tasks.borrow().is_empty();
            tasks.append(&mut self.app.inner.tasks.borrow_mut());
            *self.app.inner.tasks.borrow_mut() = tasks;
            self.busy |= cut_short;
            if !added {
                return;
            }
        }
        self.busy = true;
    }
}
/// The most a panic message may carry across the `panicked` import. A host
/// shows one line of it, and every byte over that is one the host lifts out
/// of guest memory before it can refuse anything — so the message is cut
/// here, where the guest still owns it, on a char boundary.
pub const MAX_PANIC_BYTES: usize = 1024;

/// The line the panic hook hands the host: the payload and where it came
/// from, cut to [`MAX_PANIC_BYTES`].
pub fn panic_line(message: &str, at: &str) -> String {
    let mut line = format!("{message} at {at}");
    if line.len() > MAX_PANIC_BYTES {
        let cut = (0..=MAX_PANIC_BYTES)
            .rev()
            .find(|at| line.is_char_boundary(*at))
            .unwrap_or(0);
        line.truncate(cut);
    }
    line
}

/// Appends the preferred window size and current wire epoch at compile time.
pub const fn manifest_bytes<const N: usize>(text: &str, preferred_size: &str) -> [u8; N] {
    let bytes = text.as_bytes();
    let size = preferred_size.as_bytes();
    assert!(N == bytes.len() + size.len() + 1 + wire::WIRE_EPOCH.ilog10() as usize + 1);
    let mut out = [0u8; N];
    let mut i = 0;
    while i < bytes.len() + size.len() {
        out[i] = if i < bytes.len() {
            bytes[i]
        } else {
            size[i - bytes.len()]
        };
        i += 1;
    }
    out[i] = b'\n';
    let mut epoch = wire::WIRE_EPOCH;
    let mut end = N;
    while end > i + 1 {
        end -= 1;
        out[end] = b'0' + (epoch % 10) as u8;
        epoch /= 10;
    }
    out
}

/// The manifest section and the wasm32 exports ([`wire::abi`]) for a view. `export_view!` invokes this internally.
#[macro_export]
macro_rules! export_driver {
    ($app:ty, $name:expr, $description:expr, [$($capability:literal),* $(,)?]) => {
        const MANIFEST: &str = concat!("ducktape.view.manifest.v1\n", $name, "\n", $description, "\n" $(, $capability, ",")*, "\n");

        #[cfg_attr(target_arch = "wasm32", unsafe(link_section = "ducktape.view.manifest"))]
        #[used]
        static MANIFEST_SECTION: [u8; MANIFEST.len() + <$app as $crate::View>::PREFERRED_WINDOW_SIZE.len() + 2 + $crate::wire::WIRE_EPOCH.ilog10() as usize] =
            $crate::manifest_bytes(MANIFEST, <$app as $crate::View>::PREFERRED_WINDOW_SIZE);

        #[cfg(target_arch = "wasm32")]
        mod wasm_exports {
            use super::*;

            #[unsafe(export_name = "alloc")]
            extern "C" fn alloc(len: u32) -> u32 {
                $crate::exports::alloc(len)
            }

            #[unsafe(export_name = "init")]
            extern "C" fn init(macos: u32) {
                $crate::exports::init::<$app>(macos)
            }

            #[unsafe(export_name = "tick")]
            extern "C" fn tick(ptr: u32, len: u32) -> u64 {
                $crate::exports::tick::<$app>(ptr, len)
            }

            #[unsafe(export_name = "snapshot")]
            extern "C" fn snapshot() -> u64 {
                $crate::exports::snapshot::<$app>()
            }

            #[unsafe(export_name = "restore")]
            extern "C" fn restore(ptr: u32, len: u32, macos: u32) -> u64 {
                $crate::exports::restore::<$app>(ptr, len, macos)
            }
        }
    };
}

/// The guest's half of [`wire::abi`]: what `export_driver!` builds the five
/// exports from. A module runs one app, so its driver lives here.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod exports {
    use std::any::Any;
    use std::cell::RefCell;

    use crate::{Driver, View, wire};

    #[link(wasm_import_module = "ducktape_view")]
    unsafe extern "C" {
        fn panicked(ptr: u32, len: u32);
    }

    thread_local! {
        // The last answer, kept until the next export is entered: the host
        // copies it out before it calls again.
        static ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static DRIVER: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    }

    fn driver<A: View, R>(run: impl FnOnce(&mut Driver<A>) -> R) -> R {
        DRIVER.with_borrow_mut(|driver| {
            run(driver
                .as_mut()
                .and_then(|driver| driver.downcast_mut())
                .expect("init or restore first"))
        })
    }

    pub fn init<A: View>(macos: u32) {
        install_panic_hook();
        let driver = Driver::<A>::with_macos(macos != 0);
        DRIVER.set(Some(Box::new(driver)));
    }

    pub fn tick<A: View>(ptr: u32, len: u32) -> u64 {
        let events: Vec<wire::Event> =
            wire::decode(&take(ptr, len)).expect("invalid host event frame");
        let mut frame = driver::<A, _>(|driver| driver.tick(events));
        // The host keeps the tree it has, or patches it; the whole tree
        // crosses only when neither will do.
        if frame.unchanged || !frame.patches.is_empty() {
            frame.root = None;
        }
        answer(wire::encode(&frame))
    }

    pub fn snapshot<A: View>() -> u64 {
        answer(wire::abi::encode_result(driver::<A, _>(|driver| {
            driver.snapshot()
        })))
    }

    /// A refused state leaves the driver that was there in place.
    pub fn restore<A: View>(ptr: u32, len: u32, macos: u32) -> u64 {
        install_panic_hook();
        let restored = Driver::<A>::from_snapshot(&take(ptr, len), macos != 0).map(|driver| {
            DRIVER.set(Some(Box::new(driver)));
            Vec::new()
        });
        answer(wire::abi::encode_result(restored))
    }

    /// A buffer the host fills and the next export [`take`]s. `0` for none.
    pub fn alloc(len: u32) -> u32 {
        if len == 0 {
            return 0;
        }
        Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8 as u32
    }

    /// The argument the host wrote into what [`alloc`] gave it.
    fn take(ptr: u32, len: u32) -> Vec<u8> {
        if len == 0 {
            return Vec::new();
        }
        let bytes = std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize);
        unsafe { Box::from_raw(bytes) }.into_vec()
    }

    fn answer(bytes: Vec<u8>) -> u64 {
        ANSWER.with_borrow_mut(|answer| {
            *answer = bytes;
            wire::abi::pack(answer.as_ptr() as u32, answer.len() as u32)
        })
    }

    /// A trapped instance can never be entered again, so the message leaves
    /// through the host's import before the abort that follows the hook.
    fn install_panic_hook() {
        std::panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(|text| text.as_str()))
                .unwrap_or("panicked");
            let at = info
                .location()
                .map(|location| format!("{}:{}", location.file(), location.line()))
                .unwrap_or_else(|| "unknown".into());
            let line = crate::panic_line(message, &at);
            unsafe { panicked(line.as_ptr() as u32, line.len() as u32) };
        }));
    }
}

mod combo;
pub use combo::Combo;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lifecycle_tests;
