//! the wasm port of this module, built the ADAPTER way: the native crate is
//! compiled to wasm32 unmodified and adapted to the `ducktape:module` world
//! through `ducktape-module-sdk`, so the module's logic is single-sourced (a
//! behavior change in the native crate IS the wasm change). the packaging
//! cdylib around this port is synthesized by `guest-builder` — this module is
//! the whole of the guest's hand-written surface.
//!
//! STORE-BACKED, like chat: the port injects [`WitStore`], the adapter's
//! `MerkleStore` over the wit `state-*` imports, and the real qmdb store
//! stays host-side. there is no per-dispatch snapshot: the store IS the state
//! and the wasm root is the store's merkle root, so this port is
//! root-continuous with the native module. the chat `Access` read and every
//! identity resolution are host-routed SIBLING reads inside the guest,
//! resolved by the runtime's memoized replay. equivalence is pinned block by
//! block by `tests/wasm_parity.rs`.

use crate::Call;

/// the genesis-constant id this module registers under.
const MODULE_ID: &str = "call";
/// the chat sibling that owns every channel — the one `Access` read a join
/// makes and the origin whose archive event clears a roster. genesis config
/// compiled into the guest; drift here would be a consensus fork.
const CHAT_ID: &str = "chat";
/// the sibling every external key resolves through (`OfKey`).
const IDENTITY_ID: &str = "identity";

use ducktape_module_sdk::WitStore;

ducktape_module_sdk::store_guest! {
    id: MODULE_ID,
    module: Call,
    shape: ducktape_module_sdk::store_shape(),
    new: Call::new(MODULE_ID, CHAT_ID, Box::new(WitStore)).with_identity(IDENTITY_ID),
}
