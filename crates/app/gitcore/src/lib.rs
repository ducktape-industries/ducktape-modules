// gitcore: git objects, packs, walks, diffs, merges and the server side of the wire protocol, over one host-supplied store.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod diff;
mod error;
pub mod merge;
mod object;
mod oid;
pub mod pack;
pub mod server;
mod store;
pub mod walk;
pub mod wire;

pub use error::{Error, Result};
pub use object::{Commit, Kind, Mode, Object, Signature, Tag, Tree, TreeEntry};
pub use oid::{oid_of, Hash, Oid};
pub use pack::Limits;
pub use store::{MemoryObjects, Objects};
