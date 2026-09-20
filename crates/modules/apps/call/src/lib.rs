//! The call media guest: the real-time media planes a huddle rides,
//! entirely OFF consensus, as pure state machines.
//!
//! Ported from ducktape `crates/services/media` (dfa41ce1): the same wire
//! layouts, the same codec contract, the same jitter/mix/reassembly
//! decisions. What is NOT here is everything a wasm guest cannot own — the
//! data-plane flow, the pump task, the tokio clock, the random media epoch
//! and the tracing sink. Each became an explicit input: the host hands the
//! engine datagrams ([`voice::VoiceEngine::receive`]) and takes back frames
//! ([`voice::VoiceEngine::encode_frame`]), ticks playout at its own 20 ms
//! cadence, and chooses the epoch at construction.
//!
//! Three planes, one crate:
//! - [`voice`]: Opus encode, per-speaker jitter buffering, mixed playout.
//! - [`video`]: VP8 frame fragmentation/reassembly over the video flow.
//! - [`call_wire`]: the `/v1/call/ws` binary framing the webview leg speaks.

pub mod call_wire;
#[cfg(any(test, feature = "selftest"))]
pub mod selftest;
pub mod video;
pub mod voice;

/// Largest data-plane datagram payload: the plane's 1372-byte frame
/// (overlay MTU 1420 − IPv6 40 − UDP 8) minus its 9-byte service+flow
/// header. Copied from `data_plane::MAX_DATAGRAM_PAYLOAD` so the media
/// frames sized against it fit one datagram without linking the plane.
pub const MAX_DATAGRAM_PAYLOAD: usize = 1372 - 9;

/// A transport-authenticated peer: the raw 32-byte node key the data plane
/// binds every datagram to. The same bytes as `data_plane::PeerId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub [u8; 32]);
