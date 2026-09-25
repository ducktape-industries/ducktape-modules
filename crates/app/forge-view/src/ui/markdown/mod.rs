//! Markdown as a document: the one way this view reads it (a README, a
//! markdown file, a change's body).
//!
//! - [`parse`] turns text into [`Block`]s, once per blob (`BlobCache`).
//! - [`render`] / [`render_blocks`] draw them; a pressed link calls the
//!   caller's [`OnLink`] with its raw destination.
//! - [`target`] resolves that destination: the web through the host, or a
//!   file of the repository, relative to the document's folder, its `%XX`
//!   escapes read by ducklink.
mod links;
mod parse;
mod render;

pub(crate) use links::{Target, target};
pub(crate) use parse::{Block, parse};
pub(crate) use render::{OnLink, render, render_blocks};
