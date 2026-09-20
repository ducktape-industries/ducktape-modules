//! The presence beacon, in both of its framings. On the client wire it is
//! json text (`{"type":"beacon",…}` in, `peer_beacon` out); on the mesh it
//! is the Core call-control datagram `[2][muted][camera_on][sharing]
//! [speaking]` (`media_service::video::control`, tag 2 — the keyframe-request
//! and rate-hint tags are not ported: the client contract carries whole
//! pictures, so there is no decoder to sync and no ladder to hint).
//!
//! On the voice lane a beacon shares the peer's one datagram stream with
//! media frames; the two are told apart by length — a beacon is
//! [`BEACON_LEN`] bytes, a media frame carries at least its 10-byte header.

use serde::{Deserialize, Serialize};

const TAG_BEACON: u8 = 2;
pub const BEACON_LEN: usize = 5;

/// 1 Hz presence + ephemeral state (drives tiles, NOT consensus). `sharing`
/// marks the video lane as a screen share (vs the camera); `speaking` is the
/// sender's own voice gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Beacon {
    pub muted: bool,
    pub camera_on: bool,
    pub sharing: bool,
    pub speaking: bool,
}

impl Beacon {
    pub fn encode(&self) -> Vec<u8> {
        vec![
            TAG_BEACON,
            self.muted as u8,
            self.camera_on as u8,
            self.sharing as u8,
            self.speaking as u8,
        ]
    }

    /// `None` on the wrong tag or length — a control datagram from the mesh
    /// is dropped, never trusted.
    pub fn decode(frame: &[u8]) -> Option<Beacon> {
        match frame {
            [TAG_BEACON, muted, camera_on, sharing, speaking] => Some(Beacon {
                muted: *muted != 0,
                camera_on: *camera_on != 0,
                sharing: *sharing != 0,
                speaking: *speaking != 0,
            }),
            _ => None,
        }
    }
}

#[cfg(any(test, feature = "selftest"))]
pub mod tests {
    use super::*;

    #[cfg_attr(test, test)]
    pub fn beacon_round_trips_and_rejects_other_frames() {
        let beacon = Beacon {
            muted: true,
            camera_on: false,
            sharing: true,
            speaking: false,
        };
        assert_eq!(beacon.encode(), [2, 1, 0, 1, 0]);
        assert_eq!(Beacon::decode(&beacon.encode()), Some(beacon));
        assert_eq!(Beacon::decode(&[2, 1, 0, 1]), None); // truncated
        assert_eq!(Beacon::decode(&[1]), None); // keyframe request: not ported
        assert_eq!(Beacon::decode(&[3, 0, 0, 1, 2]), None); // rate hint
    }
}
