//! The voice media engine: Opus over the data plane's datagram class.
//!
//! Wire surface (this module root + [`media`]): the 10-byte media header
//! (epoch, seq, timestamp) carried inside a data-plane datagram on
//! `Service::Voice`, payload = one 20 ms Opus frame. Speakers are identified
//! by the transport-authenticated `PeerId` — the plane's WireGuard identity
//! binding — so there is no SSRC and no media-level identity to spoof.
//!
//! ```text
//! pcm 20ms frame → Opus encode (VOIP) → media header → (host fans out)
//! per speaker:  datagrams → jitter buffer → Opus decode | gap→silence
//! playout tick: sum decoded speakers → one mixed pcm frame
//! ```
//!
//! Codec is pure-Rust `opus-rs`, which has no FEC-decode and no PLC, so a lost
//! frame is rendered as silence, not concealed — the jitter buffer still
//! reports the gap so a concealment-capable codec could fill it later.

pub mod codec;
pub mod engine;
pub mod jitter;
pub mod media;

pub use codec::{CodecError, MAX_ENCODED, VoiceDecoder, VoiceEncoder};
pub use engine::{SpeakerStats, VoiceConfig, VoiceEngine};
pub use jitter::{JitterStats, MinimalJitter, PlayoutStep};
pub use media::{MediaError, MediaHeader};

/// Voice runs at Opus's native rate, mono.
pub const SAMPLE_RATE: u32 = 48_000;
/// Samples per frame: 48 kHz × 20 ms — Opus's sweet spot and the packet
/// cadence.
pub const FRAME_SAMPLES: usize = 960;
