//! Shared workspace canvases. Geometry is integer world coordinates; camera,
//! selection and unfinished gestures stay on the editing device.
// This module owns its board records, reducer and codecs. Keep the public
// re-export for existing internal/test call sites.
mod wire;
pub use wire::*;
#[cfg(feature = "native")]
mod module;
#[cfg(feature = "native")]
pub use module::Boards;
#[cfg(feature = "guest")]
mod guest;
