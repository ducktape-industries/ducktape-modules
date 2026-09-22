// git: objects, packs, walks, diffs and the server side of the wire protocol, over one host-supplied store. What the forge program needs to verify a push and answer a fetch; merging is the git client's.

pub mod diff;
mod error;
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
