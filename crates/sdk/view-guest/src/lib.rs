//! Renderer-independent execution of dynamically loaded WASM views.
pub use gpui::prelude::FluentBuilder;
extern crate self as ducktape_view_guest;

pub use gpui::{
    hsla, px, rems, rgb, Anchor, AnchoredFitMode, AnchoredPositionMode, ClickEvent, CursorStyle,
    Edges, ElementId, FileDropEvent, FontStyle, FontWeight, Global, HighlightStyle,
    HoverListenerMode, Hsla, KeyDownEvent, KeyUpEvent, ListHorizontalSizingBehavior,
    ListSizingBehavior, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MousePressureEvent, MouseUpEvent, ObjectFit, PinchEvent, Pixels, Point,
    Resource, Role, ScrollStrategy, ScrollWheelEvent, SharedString, StrikethroughStyle,
    StyleRefinement, Styled, TextRun, TextStyle, UnderlineStyle, WindowControlArea,
};
pub use view_guest_derive::IntoElement;
pub use view_wire as wire;
mod theme;
pub use theme::Theme;
mod behavior;
mod element;
mod interactivity;
mod list;
mod primitives;
mod rich_text;
mod surface;
mod view_element;
pub use behavior::{modal_overlay, resize_handle, sensor, ModalOverlay, ResizeHandle, Sensor};
pub use element::{
    div, uniform_list, AnyElement, Div, Element, Input, IntoElement, Lowering, ParentElement,
    RenderOnce, UniformList, UniformListScrollHandle,
};
pub use interactivity::{
    FocusHandle, InteractiveElement, Interactivity, Stateful, StatefulInteractiveElement,
};
pub use list::{list, FollowMode, List, ListAlignment, ListOffset, ListScrollEvent, ListState};
pub use primitives::{
    anchored, canvas, deferred, img, svg, Anchored, Canvas, Deferred, ImageSource, ImageStyle, Img,
    StyledImage, Svg, Transformation,
};
pub use rich_text::{InteractiveText, StyledText};
pub use surface::{surface, Surface};
pub use view_element::{AnyView, ViewElement};

/// Traits and primitives used to compose guest GPUI elements.
pub mod prelude {
    pub use crate::{
        anchored, canvas, deferred, div, hsla, img, list, modal_overlay, px, rems, resize_handle,
        rgb, sensor, surface, svg, uniform_list, AnyElement, AnyView, App, ClickEvent, Context,
        Element, ElementId, FileDropEvent, FluentBuilder, FocusHandle, FollowMode, Global,
        HoverListenerMode, Hsla, Input, InteractiveElement, InteractiveText, IntoElement,
        KeyDownEvent, KeyUpEvent, List, ListAlignment, ListHorizontalSizingBehavior, ListOffset,
        ListScrollEvent, ListSizingBehavior, ListState, ModifiersChangedEvent, MouseButton,
        MouseDownEvent, MouseExitEvent, MouseMoveEvent, MousePressureEvent, MouseUpEvent,
        ParentElement, PinchEvent, Pixels, Render, RenderOnce, Role, ScrollStrategy,
        ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, StyledImage,
        StyledText, Theme, UniformListScrollHandle, Window, WindowControlArea,
    };
}
mod editor;
mod editor_binding;
mod editor_documents;
mod editor_element;
pub use editor::Editor;
pub use editor_binding::{
    EditorBinding, EditorInteractionRequest, EditorKeyRequest, EditorRichRequest, EditorStateView,
    EditorTransaction, EditorTransactionEvent,
};
pub use editor_documents::EditorDocumentUpdate;
pub use editor_element::{EditorElement, EditorElementEvent};
pub mod composer;
pub mod design;
pub mod host;
pub mod store;
pub mod testing;
pub mod widget;
pub mod window;

mod snapshot;
pub mod view;
pub use view::{Declared, Loaded, Render, View};
pub use wire::doors;
mod context;
pub use context::{App, AsyncApp, Context, Entity, Released, WeakEntity};
mod executor;
pub use executor::Task;
pub use host::Host;
pub use window::Window;
#[cfg(test)]
mod behavior_tests;
mod slots;
use context::Callback;

mod driver;
pub use driver::Driver;

const fn digits(number: u32) -> usize {
    match number.checked_ilog10() {
        Some(log) => log as usize + 1,
        None => 1,
    }
}

/// The length of [`manifest_bytes`] over `text` and `preferred_size`.
pub const fn manifest_len(text: &str, preferred_size: &str) -> usize {
    text.len()
        + preferred_size.len()
        + 1
        + digits(wire::WIRE_EPOCH)
        + 1
        + digits(wire::doors::DOORS_REVISION)
}

/// Appends the preferred window size, the current wire epoch and the doors
/// revision at compile time.
pub const fn manifest_bytes<const N: usize>(text: &str, preferred_size: &str) -> [u8; N] {
    let bytes = text.as_bytes();
    let size = preferred_size.as_bytes();
    assert!(N == manifest_len(text, preferred_size));
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
    let mut end = N;
    let mut number = wire::doors::DOORS_REVISION;
    let mut last = digits(number);
    let mut line = 0;
    while line < 2 {
        let mut left = last;
        while left > 0 {
            end -= 1;
            out[end] = b'0' + (number % 10) as u8;
            number /= 10;
            left -= 1;
        }
        end -= 1;
        out[end] = b'\n';
        number = wire::WIRE_EPOCH;
        last = digits(number);
        line += 1;
    }
    out
}

/// The manifest section and the wasm32 exports ([`wire::abi`]) for a view. `export_view!` invokes this internally.
/// Each capability must be one of [`wire::doors::CAPABILITIES`], the
/// `<capability>` half of the door kinds the view asks through.
#[macro_export]
macro_rules! export_driver {
    ($app:ty, $name:expr, $description:expr, [$($capability:literal),* $(,)?]) => {
        $(const _: () = assert!(
            $crate::wire::doors::is_capability($capability),
            concat!("`", $capability, "` is not a door capability: see view_wire::doors::CAPABILITIES")
        );)*
        impl $crate::Declared for $app {
            const CAPABILITIES: &'static [&'static str] = &[$($capability),*];
        }
        const MANIFEST: &str = concat!("ducktape.view.manifest.v2\n", $name, "\n", $description, "\n" $(, $capability, ",")*, "\n");

        #[cfg_attr(target_arch = "wasm32", unsafe(link_section = "ducktape.view.manifest"))]
        #[used]
        static MANIFEST_SECTION: [u8; $crate::manifest_len(MANIFEST, <$app as $crate::View>::PREFERRED_WINDOW_SIZE)] =
            $crate::manifest_bytes(MANIFEST, <$app as $crate::View>::PREFERRED_WINDOW_SIZE);

        #[cfg(target_arch = "wasm32")]
        mod wasm_exports {
            use super::*;

            #[unsafe(export_name = "alloc")]
            extern "C" fn alloc(len: u32) -> u32 {
                $crate::exports::alloc(len)
            }

            #[unsafe(export_name = "init")]
            extern "C" fn init(_macos: u32) {
                $crate::exports::init::<$app>()
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
            extern "C" fn restore(ptr: u32, len: u32, _macos: u32) -> u64 {
                $crate::exports::restore::<$app>(ptr, len)
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

    use crate::{wire, Driver, View};

    #[link(wasm_import_module = "ducktape_view")]
    unsafe extern "C" {
        fn panicked(ptr: u32, len: u32);
    }

    /// The most a panic message may carry across the `panicked` import. A host
    /// shows one line of it, and every byte over that is one the host lifts out
    /// of guest memory before it can refuse anything — so the message is cut
    /// here, where the guest still owns it, on a char boundary.
    const MAX_PANIC_BYTES: usize = 1024;

    /// The line the panic hook hands the host: the payload and where it came
    /// from, cut to [`MAX_PANIC_BYTES`].
    fn panic_line(message: &str, at: &str) -> String {
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

    pub fn init<A: View>() {
        install_panic_hook();
        let driver = Driver::<A>::new();
        DRIVER.set(Some(Box::new(driver)));
    }

    pub fn tick<A: View>(ptr: u32, len: u32) -> u64 {
        let events: Vec<wire::Event> =
            wire::decode(&take(ptr, len)).expect("invalid host event frame");
        let frame = driver::<A, _>(|driver| driver.tick_wire(events));
        // The host keeps the tree it has, or patches it; the whole tree
        // crosses only when neither will do (the driver leaves it out then).
        answer(wire::encode(&frame))
    }

    pub fn snapshot<A: View>() -> u64 {
        answer(wire::abi::encode_result(driver::<A, _>(|driver| {
            driver.snapshot()
        })))
    }

    /// A refused state leaves the driver that was there in place.
    pub fn restore<A: View>(ptr: u32, len: u32) -> u64 {
        install_panic_hook();
        let restored = Driver::<A>::from_snapshot(&take(ptr, len)).map(|driver| {
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
            let line = panic_line(message, &at);
            unsafe { panicked(line.as_ptr() as u32, line.len() as u32) };
        }));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lifecycle_tests;
