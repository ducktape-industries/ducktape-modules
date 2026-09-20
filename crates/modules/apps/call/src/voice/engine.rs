//! The engine controller: one encoder for the local speaker, one lane
//! (jitter buffer + stateful decoder) per remote speaker, client-side mix.
//!
//! Wiring, guest edition: the host owns the datagram flow, the pump task and
//! the audio clock, and drives this state machine from all three —
//! [`VoiceEngine::receive`] per datagram that reached it (the roster check
//! the Core pump made against the live `watch` is the host's, before this
//! call), [`VoiceEngine::encode_frame`] per captured 20 ms frame (the host
//! fans the returned datagram out), [`VoiceEngine::playout`] per output
//! tick. Speakers are keyed by transport-authenticated `PeerId`; the mix is
//! a saturating sum of whatever each lane's jitter buffer decided.
//!
//! Lanes live in a `BTreeMap`, not a `HashMap`: the mix is order-independent
//! either way, and a guest must not need a random hash seed.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use super::FRAME_SAMPLES;
use super::codec::{CodecError, VoiceDecoder, VoiceEncoder};
use super::jitter::{JitterStats, MinimalJitter, PlayoutStep};
use super::media::{self, MediaError, MediaHeader};
use crate::PeerId;

#[derive(Clone, Copy, Debug)]
pub struct VoiceConfig {
    pub bitrate_bits_per_sec: i32,
    /// Initial jitter cushion, frames (x20 ms).
    pub prefill_frames: usize,
    /// Cushion ceiling the buffer may grow to under underruns.
    pub max_depth_frames: usize,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            bitrate_bits_per_sec: 32_000,
            prefill_frames: 2,
            max_depth_frames: 6,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    Media(#[from] MediaError),
}

#[derive(Clone, Copy, Debug)]
pub struct SpeakerStats {
    pub peer: PeerId,
    pub jitter: JitterStats,
    pub decode_errors: u64,
    /// Packets this peer sent that the codec boundary refused outright
    /// (off-contract TOC, too short, or an unwind out of the library).
    pub bad_packets: u64,
    /// Times this peer restarted their media (new epoch) on the one lane —
    /// what Core logged first-and-every-hundredth; the guest keeps the count.
    pub epoch_changes: u64,
}

struct Lane {
    jitter: MinimalJitter,
    decoder: VoiceDecoder,
    decode_errors: u64,
    bad_packets: u64,
    /// which of the sender's engines this lane is following.
    epoch: u32,
    epoch_changes: u64,
}

impl Lane {
    fn new(epoch: u32, decoder: VoiceDecoder, config: VoiceConfig) -> Self {
        Lane {
            jitter: MinimalJitter::new(config.prefill_frames, config.max_depth_frames),
            decoder,
            decode_errors: 0,
            bad_packets: 0,
            epoch,
            epoch_changes: 0,
        }
    }

    /// The peer restarted their media without leaving the roster. Their seq
    /// went back to 0, so a jitter buffer anchored at the old high seq would
    /// count the whole new stream late: start the lane over.
    fn follow_new_epoch(&mut self, epoch: u32, decoder: VoiceDecoder, config: VoiceConfig) {
        self.jitter = MinimalJitter::new(config.prefill_frames, config.max_depth_frames);
        self.decoder = decoder;
        self.epoch = epoch;
        self.epoch_changes += 1;
    }
}

/// One voice channel's engine: the local encoder plus one lane per remote
/// speaker, over whatever flow the host pumps into it.
pub struct VoiceEngine {
    encoder: VoiceEncoder,
    /// this engine instance, stamped on every media and video frame it sends
    /// so a receiver can tell a restart from stale traffic.
    epoch: u32,
    seq: u16,
    timestamp: u32,
    config: VoiceConfig,
    lanes: BTreeMap<PeerId, Lane>,
    malformed: u64,
}

impl VoiceEngine {
    /// `epoch` is the host's one random value per engine instance (Core drew
    /// it with `rand::random()`); a guest has no entropy of its own.
    pub fn new(config: VoiceConfig, epoch: u32) -> Result<Self, CodecError> {
        Ok(VoiceEngine {
            encoder: VoiceEncoder::new(config.bitrate_bits_per_sec)?,
            epoch,
            seq: 0,
            timestamp: 0,
            config,
            lanes: BTreeMap::new(),
            malformed: 0,
        })
    }

    /// Encode one captured 20 ms frame into the datagram the host fans out
    /// to the channel's other members. Advances seq/timestamp.
    pub fn encode_frame(&mut self, pcm: &[i16; FRAME_SAMPLES]) -> Result<Vec<u8>, EngineError> {
        let payload = self.encoder.encode(pcm)?;
        let frame = media::encode_frame(
            MediaHeader {
                epoch: self.epoch,
                seq: self.seq,
                timestamp: self.timestamp,
            },
            &payload,
        )?;
        self.seq = self.seq.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(FRAME_SAMPLES as u32);
        Ok(frame)
    }

    /// One datagram from an ADMITTED peer into its speaker lane (lanes appear
    /// on first frame from a peer). The plane authenticated `peer`, and the
    /// host re-checked the live roster before calling: a peer already kicked
    /// from the huddle must never open or feed a lane.
    pub fn receive(&mut self, peer: PeerId, bytes: &[u8]) {
        let Ok((header, payload)) = media::decode_frame(bytes) else {
            self.malformed += 1;
            return;
        };
        let config = self.config;
        let lane = match self.lanes.entry(peer) {
            Entry::Occupied(existing) => existing.into_mut(),
            Entry::Vacant(vacant) => {
                let Ok(decoder) = VoiceDecoder::new() else {
                    self.malformed += 1;
                    return;
                };
                vacant.insert(Lane::new(header.epoch, decoder, config))
            }
        };
        if lane.epoch != header.epoch {
            let Ok(decoder) = VoiceDecoder::new() else {
                self.malformed += 1;
                return;
            };
            lane.follow_new_epoch(header.epoch, decoder, config);
        }
        lane.jitter.insert(header.seq, payload.to_vec());
    }

    /// One 20 ms output tick: step every speaker's jitter buffer, decode
    /// present frames, and mix. A gap contributes silence (the codec offers
    /// no concealment). Call at the frame cadence; returns silence while no
    /// speaker has playable audio.
    pub fn playout(&mut self) -> [i16; FRAME_SAMPLES] {
        let mut mix = [0i32; FRAME_SAMPLES];
        for lane in self.lanes.values_mut() {
            let decoded = match lane.jitter.tick() {
                // Buffering and Gap both render as silence: nothing to add.
                PlayoutStep::Buffering | PlayoutStep::Gap => None,
                PlayoutStep::Frame(payload) => Some(lane.decoder.decode(&payload)),
            };
            let pcm = match decoded {
                None => continue,
                Some(Ok(pcm)) => pcm,
                // A peer's undecodable payload must not silence the rest of
                // the mix: count it and keep going. A refused packet is the
                // hostile-or-broken case.
                Some(Err(CodecError::BadPacket(_))) => {
                    lane.bad_packets += 1;
                    continue;
                }
                Some(Err(CodecError::Opus(_))) => {
                    lane.decode_errors += 1;
                    continue;
                }
            };
            for (mixed, sample) in mix.iter_mut().zip(pcm) {
                *mixed += i32::from(sample);
            }
        }
        let mut out = [0i16; FRAME_SAMPLES];
        for (out_sample, mixed) in out.iter_mut().zip(mix) {
            *out_sample = mixed.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
        out
    }

    /// Drop a speaker's lane. Call it when the peer leaves the roster: their
    /// buffered audio and decoder state are dead weight from that moment, and
    /// a lane per departed peer accumulates for the life of the call. A peer
    /// who comes back is re-anchored by their media epoch, not by this.
    /// Returns whether a lane was there to drop.
    pub fn forget_peer(&mut self, peer: PeerId) -> bool {
        self.lanes.remove(&peer).is_some()
    }

    pub fn speaker_stats(&self) -> Vec<SpeakerStats> {
        self.lanes
            .iter()
            .map(|(peer, lane)| SpeakerStats {
                peer: *peer,
                jitter: lane.jitter.stats(),
                decode_errors: lane.decode_errors,
                bad_packets: lane.bad_packets,
                epoch_changes: lane.epoch_changes,
            })
            .collect()
    }

    /// This engine instance. The video plane stamps the same value, so both
    /// of a peer's streams restart together.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Datagrams that failed media decoding (truncated).
    pub fn malformed_frames(&self) -> u64 {
        self.malformed
    }
}

#[cfg(any(test, feature = "selftest"))]
pub mod tests {
    use super::super::codec::tests::rms;
    use super::*;

    fn peer(n: u8) -> PeerId {
        PeerId([n; 32])
    }

    fn tone(freq_hz: f32, tick: usize) -> [i16; FRAME_SAMPLES] {
        let mut pcm = [0i16; FRAME_SAMPLES];
        for (i, sample) in pcm.iter_mut().enumerate() {
            let t = (tick * FRAME_SAMPLES + i) as f32 / super::super::SAMPLE_RATE as f32;
            *sample = ((t * freq_hz * std::f32::consts::TAU).sin() * 8000.0) as i16;
        }
        pcm
    }

    fn config() -> VoiceConfig {
        VoiceConfig {
            prefill_frames: 1,
            max_depth_frames: 4,
            ..VoiceConfig::default()
        }
    }

    /// Core `second_speaker_raises_mix_energy`, without the sim link: C only
    /// listens; A speaks throughout, B joins halfway.
    #[cfg_attr(test, test)]
    pub fn second_speaker_raises_mix_energy() {
        let mut a = VoiceEngine::new(config(), 1).unwrap();
        let mut b = VoiceEngine::new(config(), 2).unwrap();
        let mut c = VoiceEngine::new(config(), 3).unwrap();
        const TICKS: usize = 100;
        let mut listener_rms = Vec::with_capacity(TICKS);
        for tick in 0..TICKS {
            let frame = a.encode_frame(&tone(220.0, tick)).unwrap();
            c.receive(peer(1), &frame);
            if tick >= 50 {
                let frame = b.encode_frame(&tone(523.0, tick)).unwrap();
                c.receive(peer(2), &frame);
            }
            listener_rms.push(rms(&c.playout()));
        }
        let solo: f64 = listener_rms[20..45].iter().sum::<f64>() / 25.0;
        let duet: f64 = listener_rms[70..95].iter().sum::<f64>() / 25.0;
        // Two uncorrelated speakers ≈ double power (~1.41x rms); well above
        // 1.2x proves B's audio is genuinely in C's mix, not just present.
        assert!(
            duet > solo * 1.2,
            "second speaker missing from mix: solo {solo:.0}, duet {duet:.0}"
        );
        assert_eq!(c.speaker_stats().len(), 2, "listener tracks both speakers");
        assert_eq!(c.malformed_frames(), 0);
    }

    /// Core `a_new_epoch_reopens_the_lane_without_a_roster_departure`: the
    /// restarted peer's seq goes back to 0 on a fresh epoch and the one lane
    /// keeps playing it instead of calling the whole stream late.
    #[cfg_attr(test, test)]
    pub fn a_new_epoch_reopens_the_lane_without_a_roster_departure() {
        let mut a = VoiceEngine::new(config(), 9).unwrap();
        let mut b = VoiceEngine::new(config(), 100).unwrap();
        for tick in 0..3 {
            let frame = b.encode_frame(&tone(440.0, tick)).unwrap();
            a.receive(peer(2), &frame);
            a.playout();
        }
        assert_eq!(a.speaker_stats()[0].jitter.played, 3);
        // B restarts: a new engine, new epoch, seq back at 0.
        let mut b = VoiceEngine::new(config(), 101).unwrap();
        for tick in 0..4 {
            let frame = b.encode_frame(&tone(440.0, tick)).unwrap();
            a.receive(peer(2), &frame);
            a.playout();
        }
        let stats = a.speaker_stats();
        assert_eq!(stats.len(), 1, "the restart reuses the peer's one lane");
        assert_eq!(stats[0].epoch_changes, 1);
        assert_eq!(stats[0].jitter.late_dropped, 0);
        assert_eq!(
            stats[0].jitter.played, 4,
            "restarted peer inaudible: {:?}",
            stats[0].jitter
        );
        assert_eq!(stats[0].bad_packets, 0);
    }

    /// Without the epoch (same epoch, seq restarted) the lane counts the new
    /// stream late — the failure the epoch exists to prevent.
    #[cfg_attr(test, test)]
    pub fn same_epoch_seq_restart_is_counted_late() {
        let mut a = VoiceEngine::new(config(), 9).unwrap();
        let mut b = VoiceEngine::new(config(), 100).unwrap();
        for tick in 0..3 {
            let frame = b.encode_frame(&tone(440.0, tick)).unwrap();
            a.receive(peer(2), &frame);
            a.playout();
        }
        let mut b = VoiceEngine::new(config(), 100).unwrap();
        let frame = b.encode_frame(&tone(440.0, 0)).unwrap();
        a.receive(peer(2), &frame);
        assert_eq!(a.speaker_stats()[0].jitter.late_dropped, 1);
    }

    #[cfg_attr(test, test)]
    pub fn forgetting_a_departed_peer_drops_the_lane() {
        let mut a = VoiceEngine::new(config(), 9).unwrap();
        let mut b = VoiceEngine::new(config(), 100).unwrap();
        let frame = b.encode_frame(&tone(440.0, 0)).unwrap();
        a.receive(peer(2), &frame);
        assert_eq!(a.speaker_stats().len(), 1);
        assert!(a.forget_peer(peer(2)));
        assert!(!a.forget_peer(peer(2)));
        assert!(a.speaker_stats().is_empty());
        assert_eq!(a.playout(), [0i16; FRAME_SAMPLES]);
    }

    /// A hostile payload behind a valid media header costs that peer one
    /// `bad_packets`, never the mix; a truncated datagram is `malformed`.
    #[cfg_attr(test, test)]
    pub fn hostile_and_truncated_datagrams_are_counted_not_fatal() {
        let mut a = VoiceEngine::new(config(), 9).unwrap();
        let exploit = media::encode_frame(
            MediaHeader {
                epoch: 5,
                seq: 0,
                timestamp: 0,
            },
            &[0x58, 0x00],
        )
        .unwrap();
        a.receive(peer(2), &exploit);
        a.receive(peer(2), &[1, 2, 3]);
        assert_eq!(a.playout(), [0i16; FRAME_SAMPLES]);
        let stats = a.speaker_stats();
        assert_eq!(stats[0].bad_packets, 1);
        assert_eq!(stats[0].decode_errors, 0);
        assert_eq!(a.malformed_frames(), 1);
    }

    /// The sender stamps its epoch and a per-frame seq/timestamp.
    #[cfg_attr(test, test)]
    pub fn sent_frames_carry_epoch_seq_and_timestamp() {
        let mut a = VoiceEngine::new(config(), 0xDEAD_BEEF).unwrap();
        assert_eq!(a.epoch(), 0xDEAD_BEEF);
        for expected_seq in 0..3u16 {
            let frame = a.encode_frame(&tone(440.0, expected_seq as usize)).unwrap();
            let (header, payload) = media::decode_frame(&frame).unwrap();
            assert_eq!(header.epoch, 0xDEAD_BEEF);
            assert_eq!(header.seq, expected_seq);
            assert_eq!(
                header.timestamp,
                u32::from(expected_seq) * FRAME_SAMPLES as u32
            );
            assert!(!payload.is_empty() && payload.len() <= super::super::MAX_ENCODED);
        }
    }
}
