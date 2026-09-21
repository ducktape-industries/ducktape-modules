//! Real sibling guests keep integration coverage without native module dependencies.

#![allow(dead_code)] // Each integration test uses a subset of these guests.

use sdk_testkit::MemStore;
use wasm_host::WasmModule;

pub fn chat() -> WasmModule {
    WasmModule::with_store(
        "chat",
        include_bytes!("../../../chat/component.wasm"),
        Box::new(MemStore::new()),
    )
    .expect("load committed Chat guest")
}

pub fn tasks() -> WasmModule {
    WasmModule::with_store(
        "tasks",
        include_bytes!("../../../tasks/component.wasm"),
        Box::new(MemStore::new()),
    )
    .expect("load committed Tasks guest")
}

pub fn inbox() -> WasmModule {
    WasmModule::with_store(
        "inbox",
        include_bytes!("../../../inbox/component.wasm"),
        Box::new(MemStore::new()),
    )
    .expect("load committed Inbox guest")
}
