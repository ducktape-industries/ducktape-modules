//! The video call media wire: fragmentation of encoded (VP8) frames onto
//! data-plane datagrams and per-sender reassembly.
//!
//! **Consensus never carries media.** This module is deliberately pure and
//! synchronous: no async, no store, nothing that touches consensus state.
//!
//! ```text
//! encoded VP8 frame → fragment_frame → datagrams on Service::Video
//! datagrams (per peer) → Reassembler::insert → CompleteFrame | drop
//! ```
//!
//! `frame` fragments/reassembles-header-only; `assembly` owns per-sender
//! reassembly state (the hub keys one [`assembly::Reassembler`] per peer).
//! The Core crate's `control` (ctl-flow wire) and `codec` (JPEG picture
//! codec) are not here: control is a later unit, the pixel codec stays
//! native by ruling.

pub mod assembly;
pub mod frame;

pub use assembly::{Assembly, CompleteFrame, Reassembler};
pub use frame::{
    FLAG_KEYFRAME, MAX_FRAGMENT_PAYLOAD, MAX_FRAGS, MAX_FRAME_BYTES, VIDEO_HEADER_LEN, VideoError,
    VideoHeader, decode_fragment, encode_fragment, fragment_frame,
};
