// forge: a git server as a ducktape program, `gitcore` (objects, packs, walks, diffs, the wire) over `store`: the rules run natively over `store::Memory` (tests, fixtures), and the `program` feature adds the wasm32 program over the host.

mod change_queries;
mod changes;
mod contract;
mod diffs;
mod discussion;
mod ops;
#[cfg(feature = "program")]
mod program;
mod queries;
mod read_contract;
mod reads;
mod repo;
mod review_contract;
mod store;

pub use contract::*;
pub use ops::{execute, init};
pub use queries::query;
