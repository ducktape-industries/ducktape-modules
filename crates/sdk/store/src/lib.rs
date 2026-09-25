//! Typed storage over `guest`'s contexts, optional: `const` [`Map`]/[`Set`]/
//! [`Item`] descriptors whose keys encode through [`KeyCodec`] and whose
//! methods take the context (`CHANNELS.get(ctx, &id)`, `CHANNELS.put(ctx,
//! &id, &row)`), and the bounded, resumable [`Page`]/[`PageReply`]. A read
//! takes `&QueryCtx`, so an `&ExecCtx` serves it too; a write takes
//! `&ExecCtx`.

mod key;
mod page;
mod table;

pub use key::KeyCodec;
pub use page::{Cursor, Listing, Page, PageReply};
pub use table::{Item, Map, Set};
