//! Authoring bindings for the separate ducktape:lane/media realtime world.
#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        world: "media",
        path: "wit",
        pub_export_macro: true,
        export_macro_name: "export_media",
        default_bindings_module: "ducktape_lane_sdk::bindings",
    });
}

pub use bindings::ducktape::lane::{host, types};
pub use bindings::{export_media, Guest};

/// Transport peer keys contain 32 bytes.
pub const PEER_LEN: usize = 32;
