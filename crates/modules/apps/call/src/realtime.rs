//! The realtime shim: the call hub as one sans-I/O step function over the
//! `ducktape:lane@0.1.0` `media` world. Core's dedicated executor hands it
//! its bound lanes once ([`Call::new`]) and then one event at a time with
//! the wall clock ([`Call::step`]); every send, admission change and close
//! comes back as an ordered effect. No host import is ever called.
//!
//! What it reproduces, from the hub this replaces (Core 99da94b68
//! `bin/node/src/voice.rs`, current client contract a133087e
//! `bin/media-service`): one voice flow and one video flow per channel,
//! opened by a channel's first session and closed by its last; the roster
//! minus self as the admission set on both; a client's audio stamped with
//! the media header and fanned out to its roster, an inbound media frame
//! delivered as a peer-tagged frame to every session whose roster holds the
//! sender; pictures fragmented out and reassembled per sender; the beacon
//! repeated at 1 Hz and expired into `peer_left`; a malformed client frame
//! closing that session.
//!
//! What it does not do: listen to audio. The hub moves encoded frames; each
//! client owns its jitter buffer and mix (`voice::VoiceEngine` stays the
//! device-side engine, unused here).
//!
//! The two mesh framings on the voice lane share one datagram stream per
//! peer (the world carries no flow on a datagram): a beacon is exactly
//! [`BEACON_LEN`] bytes, a media frame at least its header — see
//! [`crate::control`].

use std::collections::BTreeMap;

use ducktape_lane_sdk::PEER_LEN;
use ducktape_lane_sdk::types::{
    ClientFrame, ClientSend, Close, CloseFlow, Config, Datagram, Effect, Event, Frame, Lane,
    LaneSend, OpenFlow, Peer, Roster, Session, SessionOpened, SetRoster,
};
use sha2::{Digest, Sha256};

use crate::PeerId;
use crate::call_wire::{self, CapturedFrame, PeerFrame};
use crate::control::{BEACON_LEN, Beacon};
use crate::video::{self, Assembly, Reassembler};
use crate::voice::{FRAME_SAMPLES, media};

/// Per-sender datagram queue on the voice flow (the hub's `FLOW_QUEUE`:
/// ~2.5 s of one speaker) and on the video flow (`VIDEO_FLOW_QUEUE`: ~2
/// picture bursts of fragments).
const VOICE_FLOW_QUEUE: u32 = 128;
const VIDEO_FLOW_QUEUE: u32 = 256;
/// The beacon cadence the hub kept: every session's last state, once a
/// second, to every peer of its roster — what keeps a late joiner current.
const BEACON_EVERY_MS: u64 = 1_000;
/// A remote peer whose beacon has not been heard for this long has left or
/// died: the client is told `peer_left`, since it never expires a tile.
const BEACON_TTL_MS: u64 = 5_000;

/// The client wire's peer-audio frame: `[4][account u64 BE][node 32][opus]`.
/// The account is not known to a lane guest (the world names peers by node
/// key only) and is written as zero; the client reads the node.
const WS_TAG_PEER_AUDIO: u8 = 0x04;

/// `data_plane::FlowId::derive`: sha256 of the domain, first 8 bytes BE —
/// the same derivation every node uses, so two guests meet on one flow.
fn flow(domain: String) -> u64 {
    let digest = Sha256::digest(domain.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"))
}

fn voice_flow(channel: &str) -> u64 {
    flow(format!("voice-channel:{channel}"))
}

fn video_flow(channel: &str) -> u64 {
    flow(format!("video-channel:{channel}"))
}

fn hex(peer: &PeerId) -> String {
    peer.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn text(session: Session, value: serde_json::Value) -> Effect {
    Effect::ClientSend(ClientSend {
        session,
        frame: Frame::Text(value.to_string()),
    })
}

fn binary(session: Session, bytes: Vec<u8>) -> Effect {
    Effect::ClientSend(ClientSend {
        session,
        frame: Frame::Binary(bytes),
    })
}

fn peer_beacon(session: Session, peer: &PeerId, beacon: &Beacon) -> Effect {
    let mut value = serde_json::to_value(beacon).expect("four bools");
    value["type"] = "peer_beacon".into();
    value["peer"] = hex(peer).into();
    text(session, value)
}

fn peer_left(session: Session, peer: &PeerId) -> Effect {
    text(
        session,
        serde_json::json!({"type": "peer_left", "peer": hex(peer)}),
    )
}

/// The client's one control message.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Beacon {
        muted: bool,
        camera_on: bool,
        sharing: bool,
        speaking: bool,
    },
}

/// One admitted client socket: its channel, the peers the host said are in
/// that channel (minus this node), what it last said about itself, and the
/// stamps on what it sends.
struct Seat {
    channel: String,
    roster: Vec<PeerId>,
    beacon: Beacon,
    beacon_sent_ms: u64,
    /// this seat's sending engine, as far as the mesh is concerned: a fresh
    /// epoch per session so a receiver tells a reconnect from stale traffic.
    /// The wall clock at open stands in for the entropy a guest lacks.
    epoch: u32,
    seq: u16,
    timestamp: u32,
    frame_no: u32,
}

/// One remote sender: its last beacon (and when), and its picture stream.
struct Remote {
    beacon: Option<(Beacon, u64)>,
    epoch: u32,
    video: Reassembler,
}

pub struct Call {
    self_peer: PeerId,
    voice: Lane,
    /// absent when the host bound no video lane: pictures are then dropped
    /// at this node, audio and beacons unaffected.
    video: Option<Lane>,
    seats: BTreeMap<Session, Seat>,
    remotes: BTreeMap<PeerId, Remote>,
}

impl Call {
    /// The guest's `init`: refuses a self peer that is not a node key and a
    /// binding without a `voice` lane (the reason is the `init` error).
    pub fn new(config: Config) -> Result<Self, String> {
        let self_peer = peer_id(&config.self_peer).ok_or("invalid_peer")?;
        let lane = |name: &str| {
            config
                .lanes
                .iter()
                .find(|lane| lane.name == name)
                .map(|lane| lane.id)
        };
        Ok(Call {
            self_peer,
            voice: lane("voice").ok_or("no lane named voice")?,
            video: lane("video"),
            seats: BTreeMap::new(),
            remotes: BTreeMap::new(),
        })
    }

    pub fn step(&mut self, event: Event, now_ms: u64) -> Vec<Effect> {
        match event {
            Event::Tick => self.tick(now_ms),
            Event::Datagram(datagram) => self.datagram(datagram, now_ms),
            Event::ClientFrame(frame) => self.client_frame(frame, now_ms),
            Event::Roster(roster) => self.roster(roster),
            Event::SessionOpened(opened) => self.open(opened, now_ms),
            Event::SessionClosed(session) => self.close(session),
        }
    }

    fn seats_in<'a>(&'a self, channel: &'a str) -> impl Iterator<Item = &'a Seat> + 'a {
        self.seats
            .values()
            .filter(move |seat| seat.channel == channel)
    }

    /// Every seat that admits `peer` — the sessions an inbound frame from
    /// it reaches.
    fn admitting(&self, peer: &PeerId) -> Vec<Session> {
        self.seats
            .iter()
            .filter(|(_, seat)| seat.roster.contains(peer))
            .map(|(session, _)| *session)
            .collect()
    }

    fn channel_roster(&self, channel: &str) -> Vec<Peer> {
        let mut peers: Vec<PeerId> = self
            .seats_in(channel)
            .flat_map(|seat| seat.roster.iter().copied())
            .collect();
        peers.sort();
        peers.dedup();
        peers.into_iter().map(|peer| peer.0.to_vec()).collect()
    }

    /// The admission set of a channel on every lane bound for it.
    fn set_roster(&self, channel: &str) -> Vec<Effect> {
        let peers = self.channel_roster(channel);
        self.lanes(channel)
            .into_iter()
            .map(|(lane, flow)| {
                Effect::SetRoster(SetRoster {
                    lane,
                    flow,
                    peers: peers.clone(),
                })
            })
            .collect()
    }

    fn lanes(&self, channel: &str) -> Vec<(Lane, u64)> {
        let mut lanes = vec![(self.voice, voice_flow(channel))];
        if let Some(video) = self.video {
            lanes.push((video, video_flow(channel)));
        }
        lanes
    }

    fn open(&mut self, opened: SessionOpened, now_ms: u64) -> Vec<Effect> {
        let SessionOpened { session, channel } = opened;
        let mut effects = Vec::new();
        if self.seats_in(&channel).next().is_none() {
            for (lane, flow) in self.lanes(&channel) {
                let max_queued = if lane == self.voice {
                    VOICE_FLOW_QUEUE
                } else {
                    VIDEO_FLOW_QUEUE
                };
                effects.push(Effect::OpenFlow(OpenFlow {
                    lane,
                    flow,
                    max_queued,
                }));
            }
        }
        self.seats.insert(
            session,
            Seat {
                channel,
                roster: Vec::new(),
                beacon: Beacon::default(),
                beacon_sent_ms: now_ms,
                epoch: now_ms as u32,
                seq: 0,
                timestamp: 0,
                frame_no: 0,
            },
        );
        // Admission sends the roster before any media. Peers arrive with
        // the host's roster event, as `peer_beacon`s, so this one is empty.
        effects.push(text(
            session,
            serde_json::json!({"type": "ready", "peers": []}),
        ));
        effects
    }

    fn close(&mut self, session: Session) -> Vec<Effect> {
        let Some(closed) = self.seats.remove(&session) else {
            return Vec::new();
        };
        self.prune_remotes();
        if self.seats_in(&closed.channel).next().is_none() {
            self.lanes(&closed.channel)
                .into_iter()
                .map(|(lane, flow)| Effect::CloseFlow(CloseFlow { lane, flow }))
                .collect()
        } else {
            self.set_roster(&closed.channel)
        }
    }

    /// End a session the guest will not serve: its bookkeeping goes the
    /// way of a host close, then the client gets its one reason.
    fn refuse(&mut self, session: Session, reason: &str) -> Vec<Effect> {
        let mut effects = self.close(session);
        effects.push(Effect::Close(Close {
            session,
            reason: reason.into(),
        }));
        effects
    }

    /// Remote state lives only while some seat admits the peer.
    fn prune_remotes(&mut self) {
        let seats = &self.seats;
        self.remotes
            .retain(|peer, _| seats.values().any(|seat| seat.roster.contains(peer)));
    }

    fn roster(&mut self, roster: Roster) -> Vec<Effect> {
        let Roster { session, peers } = roster;
        let self_peer = self.self_peer;
        let Some(seat) = self.seats.get_mut(&session) else {
            return Vec::new();
        };
        let next: Vec<PeerId> = peers
            .iter()
            .filter_map(|peer| peer_id(peer))
            .filter(|peer| *peer != self_peer)
            .collect();
        let previous = std::mem::replace(&mut seat.roster, next.clone());
        let channel = seat.channel.clone();
        self.prune_remotes();
        let mut effects = self.set_roster(&channel);
        // what the client already knows about a newly admitted peer, now;
        // a departed one is gone from its tiles now, not when its beacon
        // would have expired.
        for peer in next.iter().filter(|peer| !previous.contains(peer)) {
            if let Some((beacon, _)) = self.remotes.get(peer).and_then(|remote| remote.beacon) {
                effects.push(peer_beacon(session, peer, &beacon));
            }
        }
        for peer in previous.iter().filter(|peer| !next.contains(peer)) {
            effects.push(peer_left(session, peer));
        }
        effects
    }

    fn client_frame(&mut self, frame: ClientFrame, now_ms: u64) -> Vec<Effect> {
        let ClientFrame { session, frame } = frame;
        if !self.seats.contains_key(&session) {
            return Vec::new();
        }
        match frame {
            Frame::Text(text) => match serde_json::from_str(&text) {
                Ok(Control::Beacon {
                    muted,
                    camera_on,
                    sharing,
                    speaking,
                }) => {
                    if camera_on && sharing {
                        return self.refuse(session, "ambiguous_capture_source");
                    }
                    let seat = self.seats.get_mut(&session).expect("checked");
                    seat.beacon = Beacon {
                        muted,
                        camera_on,
                        sharing,
                        speaking: speaking && !muted,
                    };
                    // push now so a toggle feels live; the 1 Hz repeat
                    // keeps late joiners current.
                    self.send_beacon(session, now_ms)
                }
                Err(_) => self.refuse(session, "invalid_call_control"),
            },
            Frame::Binary(bytes) => match bytes.first().copied() {
                Some(call_wire::WS_TAG_AUDIO) => match call_wire::decode_audio(&bytes) {
                    Some(voice) => self.send_audio(session, voice),
                    None => self.refuse(session, "invalid_audio_frame"),
                },
                Some(call_wire::WS_TAG_VIDEO_CAPTURED) => {
                    match call_wire::decode_captured(&bytes) {
                        Some(picture) => self.send_picture(session, picture),
                        None => self.refuse(session, "invalid_picture_frame"),
                    }
                }
                _ => self.refuse(session, "invalid_media_tag"),
            },
        }
    }

    /// One datagram to every peer of the seat's roster on `lane`.
    fn fan_out(seat: &Seat, lane: Lane, bytes: &[u8]) -> Vec<Effect> {
        seat.roster
            .iter()
            .map(|peer| {
                Effect::LaneSend(LaneSend {
                    lane,
                    peer: peer.0.to_vec(),
                    bytes: bytes.to_vec(),
                })
            })
            .collect()
    }

    fn send_beacon(&mut self, session: Session, now_ms: u64) -> Vec<Effect> {
        let seat = self.seats.get_mut(&session).expect("checked");
        seat.beacon_sent_ms = now_ms;
        Self::fan_out(seat, self.voice, &seat.beacon.encode())
    }

    fn send_audio(&mut self, session: Session, voice: &[u8]) -> Vec<Effect> {
        let seat = self.seats.get_mut(&session).expect("checked");
        if seat.beacon.muted {
            return Vec::new();
        }
        let header = media::MediaHeader {
            epoch: seat.epoch,
            seq: seat.seq,
            timestamp: seat.timestamp,
        };
        // the client bound fits the datagram bound with the header in front
        let frame = media::encode_frame(header, voice).expect("one voice frame fits");
        seat.seq = seat.seq.wrapping_add(1);
        seat.timestamp = seat.timestamp.wrapping_add(FRAME_SAMPLES as u32);
        Self::fan_out(seat, self.voice, &frame)
    }

    fn send_picture(&mut self, session: Session, picture: CapturedFrame) -> Vec<Effect> {
        let Some(video) = self.video else {
            return Vec::new();
        };
        let seat = self.seats.get_mut(&session).expect("checked");
        let capturing = seat.beacon.camera_on || seat.beacon.sharing;
        if !capturing {
            return Vec::new();
        }
        // oversize or empty: drop, stay alive (the hub's posture)
        let Ok(fragments) = video::fragment_frame(
            seat.epoch,
            seat.frame_no,
            picture.keyframe,
            picture.ts_ms,
            &picture.data,
        ) else {
            return Vec::new();
        };
        seat.frame_no = seat.frame_no.wrapping_add(1);
        fragments
            .iter()
            .flat_map(|fragment| Self::fan_out(seat, video, fragment))
            .collect()
    }

    fn datagram(&mut self, datagram: Datagram, now_ms: u64) -> Vec<Effect> {
        let Datagram { lane, peer, bytes } = datagram;
        let Some(peer) = peer_id(&peer) else {
            return Vec::new();
        };
        let sessions = self.admitting(&peer);
        if sessions.is_empty() {
            return Vec::new(); // the host's admission failed open: drop
        }
        if lane == self.voice {
            if bytes.len() == BEACON_LEN {
                let Some(beacon) = Beacon::decode(&bytes) else {
                    return Vec::new();
                };
                self.remote(peer).beacon = Some((beacon, now_ms));
                return sessions
                    .iter()
                    .map(|session| peer_beacon(*session, &peer, &beacon))
                    .collect();
            }
            let Ok((_, voice)) = media::decode_frame(&bytes) else {
                return Vec::new();
            };
            if voice.is_empty() || voice.len() > call_wire::MAX_AUDIO_PAYLOAD {
                return Vec::new();
            }
            let mut frame = Vec::with_capacity(41 + voice.len());
            frame.push(WS_TAG_PEER_AUDIO);
            frame.extend_from_slice(&0u64.to_be_bytes());
            frame.extend_from_slice(&peer.0);
            frame.extend_from_slice(voice);
            return sessions
                .iter()
                .map(|session| binary(*session, frame.clone()))
                .collect();
        }
        if Some(lane) != self.video {
            return Vec::new();
        }
        let Ok((header, payload)) = video::decode_fragment(&bytes) else {
            return Vec::new();
        };
        let remote = self.remote(peer);
        if remote.epoch != header.epoch {
            // their media restarted under the same roster entry: a fresh
            // stream, so a retained reassembler would call it stale forever.
            remote.epoch = header.epoch;
            remote.video = Reassembler::default();
        }
        let Assembly::Complete(done) = remote.video.insert(header, payload) else {
            return Vec::new();
        };
        let frame = call_wire::encode_peer(&PeerFrame {
            peer: peer.0,
            keyframe: done.keyframe,
            ts_ms: done.ts_ms,
            data: done.data,
        });
        sessions
            .iter()
            .map(|session| binary(*session, frame.clone()))
            .collect()
    }

    fn remote(&mut self, peer: PeerId) -> &mut Remote {
        self.remotes.entry(peer).or_insert_with(|| Remote {
            beacon: None,
            epoch: 0,
            video: Reassembler::default(),
        })
    }

    fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        let due: Vec<Session> = self
            .seats
            .iter()
            .filter(|(_, seat)| now_ms.saturating_sub(seat.beacon_sent_ms) >= BEACON_EVERY_MS)
            .map(|(session, _)| *session)
            .collect();
        for session in due {
            effects.extend(self.send_beacon(session, now_ms));
        }
        let expired: Vec<PeerId> = self
            .remotes
            .iter()
            .filter(|(_, remote)| {
                remote
                    .beacon
                    .is_some_and(|(_, seen_ms)| now_ms.saturating_sub(seen_ms) > BEACON_TTL_MS)
            })
            .map(|(peer, _)| *peer)
            .collect();
        for peer in expired {
            self.remotes.remove(&peer);
            for session in self.admitting(&peer) {
                effects.push(peer_left(session, &peer));
            }
        }
        effects
    }
}

fn peer_id(peer: &[u8]) -> Option<PeerId> {
    (peer.len() == PEER_LEN).then(|| PeerId(peer.try_into().expect("32 bytes")))
}

#[cfg(any(test, feature = "selftest"))]
pub mod tests {
    use super::*;
    use ducktape_lane_sdk::types::LaneBinding;

    const VOICE: Lane = 2;
    const VIDEO: Lane = 3;

    fn peer(octet: u8) -> PeerId {
        PeerId([octet; 32])
    }

    fn config(lanes: &[(&str, Lane)]) -> Config {
        Config {
            self_peer: peer(1).0.to_vec(),
            lanes: lanes
                .iter()
                .map(|(name, id)| LaneBinding {
                    name: (*name).into(),
                    id: *id,
                })
                .collect(),
        }
    }

    /// self = peer 1, voice lane 2, video lane 3.
    fn call() -> Call {
        Call::new(config(&[("voice", VOICE), ("video", VIDEO)])).unwrap()
    }

    fn opened(session: Session, channel: &str) -> Event {
        Event::SessionOpened(SessionOpened {
            session,
            channel: channel.into(),
        })
    }

    fn roster(session: Session, peers: &[PeerId]) -> Event {
        Event::Roster(Roster {
            session,
            peers: peers.iter().map(|peer| peer.0.to_vec()).collect(),
        })
    }

    fn client_text(session: Session, text: &str) -> Event {
        Event::ClientFrame(ClientFrame {
            session,
            frame: Frame::Text(text.into()),
        })
    }

    fn client_binary(session: Session, bytes: Vec<u8>) -> Event {
        Event::ClientFrame(ClientFrame {
            session,
            frame: Frame::Binary(bytes),
        })
    }

    fn datagram(lane: Lane, peer: PeerId, bytes: Vec<u8>) -> Event {
        Event::Datagram(Datagram {
            lane,
            peer: peer.0.to_vec(),
            bytes,
        })
    }

    fn beacon_text(muted: bool, camera_on: bool, sharing: bool, speaking: bool) -> String {
        serde_json::json!({"type": "beacon", "muted": muted, "camera_on": camera_on,
        "sharing": sharing, "speaking": speaking})
        .to_string()
    }

    /// a seat in `channel` with `peers` admitted, its `ready` and admission
    /// effects consumed.
    fn seated(call: &mut Call, session: Session, channel: &str, peers: &[PeerId]) {
        call.step(opened(session, channel), 0);
        call.step(roster(session, peers), 0);
    }

    // ---- the effect shapes the assertions read

    fn lane_sends(effects: &[Effect]) -> Vec<(Lane, PeerId, Vec<u8>)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::LaneSend(send) => {
                    Some((send.lane, peer_id(&send.peer).unwrap(), send.bytes.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn client_sends(effects: &[Effect]) -> Vec<(Session, Frame)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::ClientSend(send) => Some((send.session, send.frame.clone())),
                _ => None,
            })
            .collect()
    }

    fn texts(effects: &[Effect]) -> Vec<(Session, serde_json::Value)> {
        client_sends(effects)
            .into_iter()
            .filter_map(|(session, frame)| match frame {
                Frame::Text(text) => Some((session, serde_json::from_str(&text).unwrap())),
                Frame::Binary(_) => None,
            })
            .collect()
    }

    fn binaries(effects: &[Effect]) -> Vec<(Session, Vec<u8>)> {
        client_sends(effects)
            .into_iter()
            .filter_map(|(session, frame)| match frame {
                Frame::Binary(bytes) => Some((session, bytes)),
                Frame::Text(_) => None,
            })
            .collect()
    }

    fn rosters(effects: &[Effect]) -> Vec<(Lane, u64, Vec<PeerId>)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::SetRoster(set) => Some((
                    set.lane,
                    set.flow,
                    set.peers
                        .iter()
                        .map(|peer| peer_id(peer).unwrap())
                        .collect(),
                )),
                _ => None,
            })
            .collect()
    }

    /// one tag per effect, in order — the shape of a step's answer.
    fn kinds(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .map(|effect| match effect {
                Effect::LaneSend(_) => "lane-send",
                Effect::ClientSend(_) => "client-send",
                Effect::SetRoster(_) => "set-roster",
                Effect::OpenFlow(_) => "open-flow",
                Effect::CloseFlow(_) => "close-flow",
                Effect::Log(_) => "log",
                Effect::Close(_) => "close",
            })
            .collect()
    }

    fn closes(effects: &[Effect]) -> Vec<(Session, String)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Close(close) => Some((close.session, close.reason.clone())),
                _ => None,
            })
            .collect()
    }

    #[cfg_attr(test, test)]
    pub fn init_refuses_a_bad_self_peer_and_a_binding_without_voice() {
        let mut bad_peer = config(&[("voice", VOICE)]);
        bad_peer.self_peer = vec![1; 31];
        assert_eq!(Call::new(bad_peer).err(), Some("invalid_peer".into()));
        assert_eq!(
            Call::new(config(&[("video", VIDEO)])).err(),
            Some("no lane named voice".into())
        );
        // video is optional: an audio-only binding is served
        assert!(Call::new(config(&[("voice", VOICE)])).is_ok());
    }

    /// The flow derivation is `data_plane::FlowId::derive`, so a guest on
    /// one node meets Core's native double on another: pinned bytes.
    #[cfg_attr(test, test)]
    pub fn flows_derive_like_the_data_plane() {
        let digest = Sha256::digest(b"voice-channel:room");
        assert_eq!(
            voice_flow("room"),
            u64::from_be_bytes(digest[..8].try_into().unwrap())
        );
        assert_ne!(voice_flow("room"), video_flow("room"));
        assert_ne!(voice_flow("room"), voice_flow("room2"));
    }

    #[cfg_attr(test, test)]
    pub fn a_channel_opens_its_flows_with_its_first_seat_and_closes_them_with_its_last() {
        let mut call = call();
        let first = call.step(opened(7, "room"), 10);
        assert_eq!(
            kinds(&first),
            ["open-flow", "open-flow", "client-send"],
            "flows first, then the client's ready"
        );
        let Effect::OpenFlow(voice) = &first[0] else {
            panic!()
        };
        let Effect::OpenFlow(video) = &first[1] else {
            panic!()
        };
        assert_eq!(
            (voice.lane, voice.flow, voice.max_queued),
            (VOICE, voice_flow("room"), VOICE_FLOW_QUEUE)
        );
        assert_eq!(
            (video.lane, video.flow, video.max_queued),
            (VIDEO, video_flow("room"), VIDEO_FLOW_QUEUE)
        );
        assert_eq!(
            texts(&first),
            [(7, serde_json::json!({"type": "ready", "peers": []}))]
        );
        // a second seat in the same channel reuses the flows
        let second = call.step(opened(8, "room"), 20);
        assert_eq!(kinds(&second), ["client-send"]);
        // another channel is another pair of flows
        assert_eq!(
            kinds(&call.step(opened(9, "other"), 30)),
            ["open-flow", "open-flow", "client-send"]
        );
        // one seat leaving re-states the channel's admission; the last
        // closes the flows
        call.step(roster(8, &[peer(2)]), 40);
        let left = call.step(Event::SessionClosed(8), 50);
        assert_eq!(kinds(&left), ["set-roster", "set-roster"]);
        assert_eq!(rosters(&left)[0], (VOICE, voice_flow("room"), vec![]));
        let last = call.step(Event::SessionClosed(7), 60);
        assert_eq!(kinds(&last), ["close-flow", "close-flow"]);
        let Effect::CloseFlow(closed) = &last[1] else {
            panic!()
        };
        assert_eq!((closed.lane, closed.flow), (VIDEO, video_flow("room")));
        // an unknown session closing is nothing
        assert!(call.step(Event::SessionClosed(7), 70).is_empty());
        // an audio-only binding opens and closes one flow
        let mut audio_only = Call::new(config(&[("voice", VOICE)])).unwrap();
        assert_eq!(
            kinds(&audio_only.step(opened(1, "room"), 0)),
            ["open-flow", "client-send"]
        );
        assert_eq!(
            kinds(&audio_only.step(Event::SessionClosed(1), 0)),
            ["close-flow"]
        );
    }

    #[cfg_attr(test, test)]
    pub fn the_roster_minus_self_is_the_admission_set_on_both_lanes() {
        let mut call = call();
        call.step(opened(7, "room"), 0);
        let admitted = call.step(roster(7, &[peer(1), peer(3), peer(2)]), 0);
        assert_eq!(
            rosters(&admitted),
            [
                (VOICE, voice_flow("room"), vec![peer(2), peer(3)]),
                (VIDEO, video_flow("room"), vec![peer(2), peer(3)]),
            ],
            "self is filtered, the set is sorted"
        );
        // a second seat in the channel widens the channel's set
        call.step(opened(8, "room"), 0);
        let widened = call.step(roster(8, &[peer(4)]), 0);
        assert_eq!(
            rosters(&widened)[0].2,
            vec![peer(2), peer(3), peer(4)],
            "the channel's admission is the union of its seats' rosters"
        );
        // a roster for a session that was never opened is nothing
        assert!(call.step(roster(99, &[peer(5)]), 0).is_empty());
        // a peer that is not a node key is not admitted (and not a trap)
        let mut odd = Roster {
            session: 7,
            peers: vec![vec![9; 31]],
        };
        odd.peers.push(peer(6).0.to_vec());
        let effects = call.step(Event::Roster(odd), 0);
        assert_eq!(rosters(&effects)[0].2, vec![peer(4), peer(6)]);
    }

    #[cfg_attr(test, test)]
    pub fn audio_is_stamped_out_to_the_roster_and_delivered_to_who_admits_the_sender() {
        let mut call = call();
        seated(&mut call, 7, "room", &[peer(2), peer(3)]);
        seated(&mut call, 8, "room", &[peer(2)]);
        seated(&mut call, 9, "other", &[peer(4)]);
        // out: one media frame per roster peer, on the voice lane
        let voice = vec![0x78, 0x01, 0x02];
        let out = call.step(client_binary(7, call_wire::encode_audio(&voice)), 100);
        let sends = lane_sends(&out);
        assert_eq!(sends.len(), 2);
        assert_eq!(sends[0].0, VOICE);
        assert_eq!(sends[0].1, peer(2));
        assert_eq!(sends[1].1, peer(3));
        assert_eq!(sends[0].2, sends[1].2, "the same frame to every peer");
        let (header, payload) = media::decode_frame(&sends[0].2).unwrap();
        assert_eq!(payload, &voice[..]);
        assert_eq!((header.seq, header.timestamp), (0, 0));
        let again = call.step(client_binary(7, call_wire::encode_audio(&voice)), 120);
        let (next, _) = media::decode_frame(&lane_sends(&again)[0].2).unwrap();
        assert_eq!(next.epoch, header.epoch, "one epoch per session");
        assert_eq!((next.seq, next.timestamp), (1, FRAME_SAMPLES as u32));
        assert_eq!(
            kinds(&again),
            ["lane-send", "lane-send"],
            "audio is mesh-only"
        );
        // a muted seat's audio goes nowhere
        call.step(client_text(7, &beacon_text(true, false, false, false)), 130);
        assert!(
            call.step(client_binary(7, call_wire::encode_audio(&voice)), 140)
                .is_empty()
        );
        // in: a media frame from peer 2 reaches the seats admitting 2, as
        // the client's peer-audio frame; peer 3 reaches only seat 7
        let frame = media::encode_frame(
            media::MediaHeader {
                epoch: 5,
                seq: 9,
                timestamp: 0,
            },
            &voice,
        )
        .unwrap();
        let delivered = call.step(datagram(VOICE, peer(2), frame.clone()), 150);
        let mut want = vec![WS_TAG_PEER_AUDIO];
        want.extend_from_slice(&[0; 8]);
        want.extend_from_slice(&peer(2).0);
        want.extend_from_slice(&voice);
        assert_eq!(binaries(&delivered), [(7, want.clone()), (8, want)]);
        assert_eq!(
            binaries(&call.step(datagram(VOICE, peer(3), frame.clone()), 160)).len(),
            1
        );
        // an unadmitted sender, a truncated frame, an empty or oversize
        // payload, and a datagram on an unbound lane are all dropped
        assert!(
            call.step(datagram(VOICE, peer(5), frame.clone()), 170)
                .is_empty()
        );
        assert!(
            call.step(datagram(VOICE, peer(2), vec![1, 2, 3]), 180)
                .is_empty()
        );
        let empty = media::encode_frame(
            media::MediaHeader {
                epoch: 5,
                seq: 10,
                timestamp: 0,
            },
            &[],
        )
        .unwrap();
        assert!(call.step(datagram(VOICE, peer(2), empty), 190).is_empty());
        let big = media::encode_frame(
            media::MediaHeader {
                epoch: 5,
                seq: 11,
                timestamp: 0,
            },
            &vec![0; call_wire::MAX_AUDIO_PAYLOAD + 1],
        )
        .unwrap();
        assert!(call.step(datagram(VOICE, peer(2), big), 200).is_empty());
        assert!(call.step(datagram(9, peer(2), frame), 210).is_empty());
    }

    #[cfg_attr(test, test)]
    pub fn pictures_fragment_out_and_reassemble_in_per_sender() {
        let mut call = call();
        seated(&mut call, 7, "room", &[peer(2)]);
        let picture = CapturedFrame {
            keyframe: true,
            ts_ms: 0x0102_0304,
            data: vec![0xAB; 3 * video::MAX_FRAGMENT_PAYLOAD + 1],
        };
        // not capturing: dropped, session kept
        let idle = call.step(client_binary(7, call_wire::encode_captured(&picture)), 0);
        assert!(idle.is_empty());
        call.step(client_text(7, &beacon_text(false, true, false, false)), 0);
        let out = call.step(client_binary(7, call_wire::encode_captured(&picture)), 0);
        let sends = lane_sends(&out);
        assert_eq!(sends.len(), 4, "four fragments to the one peer");
        assert!(
            sends
                .iter()
                .all(|(lane, peer, _)| *lane == VIDEO && *peer == peer_id(&[2; 32]).unwrap())
        );
        let (header, _) = video::decode_fragment(&sends[0].2).unwrap();
        assert_eq!(
            (header.frame_no, header.frag_count, header.keyframe),
            (0, 4, true)
        );
        // the same fragments arriving from peer 2 (out of order) complete
        // into one peer frame for the seat admitting 2
        let mut arrived = Vec::new();
        for (_, _, bytes) in [&sends[3], &sends[0], &sends[2], &sends[1]] {
            arrived.extend(call.step(datagram(VIDEO, peer(2), bytes.clone()), 0));
        }
        let frames = binaries(&arrived);
        assert_eq!(frames.len(), 1);
        let got = call_wire::decode_peer(&frames[0].1).unwrap();
        assert_eq!(got.peer, peer(2).0);
        assert_eq!((got.keyframe, got.ts_ms), (true, 0x0102_0304));
        assert_eq!(got.data, picture.data);
        // a restarted sender (new epoch, frame_no back at 0) is followed,
        // not called stale
        let restarted = video::fragment_frame(99, 0, true, 1, &[1, 2, 3]).unwrap();
        let effects = call.step(datagram(VIDEO, peer(2), restarted[0].clone()), 0);
        assert_eq!(binaries(&effects).len(), 1);
        // a truncated fragment is dropped; no video lane bound drops
        // captured pictures without closing the seat
        assert!(
            call.step(datagram(VIDEO, peer(2), vec![1, 2]), 0)
                .is_empty()
        );
        let mut audio_only = Call::new(config(&[("voice", VOICE)])).unwrap();
        seated(&mut audio_only, 1, "room", &[peer(2)]);
        audio_only.step(client_text(1, &beacon_text(false, true, false, false)), 0);
        assert!(
            audio_only
                .step(client_binary(1, call_wire::encode_captured(&picture)), 0)
                .is_empty()
        );
        assert_eq!(audio_only.seats.len(), 1);
    }

    #[cfg_attr(test, test)]
    pub fn beacons_go_out_now_and_every_second_and_come_in_as_peer_beacons() {
        let mut call = call();
        seated(&mut call, 7, "room", &[peer(2), peer(3)]);
        // the client's beacon: to the roster now, speaking gated by muted
        let out = call.step(client_text(7, &beacon_text(true, false, true, true)), 1_000);
        let sends = lane_sends(&out);
        assert_eq!(sends.len(), 2);
        assert_eq!(
            Beacon::decode(&sends[0].2),
            Some(Beacon {
                muted: true,
                camera_on: false,
                sharing: true,
                speaking: false
            })
        );
        // repeated at 1 Hz on the host's ticks, not before
        assert!(call.step(Event::Tick, 1_500).is_empty());
        let repeated = call.step(Event::Tick, 2_000);
        assert_eq!(lane_sends(&repeated).len(), 2);
        assert!(call.step(Event::Tick, 2_900).is_empty());
        assert_eq!(lane_sends(&call.step(Event::Tick, 3_000)).len(), 2);
        // a peer's beacon reaches the seats admitting it as `peer_beacon`
        let beacon = Beacon {
            muted: false,
            camera_on: true,
            sharing: false,
            speaking: true,
        };
        let delivered = call.step(datagram(VOICE, peer(2), beacon.encode()), 3_100);
        assert_eq!(
            texts(&delivered),
            [(
                7,
                serde_json::json!({"type": "peer_beacon", "peer": hex(&peer(2)),
            "muted": false, "camera_on": true, "sharing": false, "speaking": true})
            )]
        );
        // a seat admitted later learns the known state at once
        seated(&mut call, 8, "room", &[]);
        let admitted = call.step(roster(8, &[peer(2), peer(3)]), 3_200);
        assert_eq!(
            kinds(&admitted),
            ["set-roster", "set-roster", "client-send"]
        );
        assert_eq!(texts(&admitted)[0].1["type"], "peer_beacon");
        assert_eq!(texts(&admitted)[0].1["peer"], hex(&peer(2)));
        // a roster shrink is `peer_left` now
        let shrunk = call.step(roster(8, &[peer(3)]), 3_300);
        assert_eq!(
            texts(&shrunk),
            [(
                8,
                serde_json::json!({"type": "peer_left", "peer": hex(&peer(2))})
            )]
        );
        // a beacon that falls silent expires into `peer_left` for the seats
        // still admitting the peer (seat 7 only: seat 8 dropped 2)
        assert!(texts(&call.step(Event::Tick, 3_100 + BEACON_TTL_MS)).is_empty());
        let expired = call.step(Event::Tick, 3_101 + BEACON_TTL_MS);
        assert_eq!(
            texts(&expired),
            [(
                7,
                serde_json::json!({"type": "peer_left", "peer": hex(&peer(2))})
            )]
        );
        assert!(!call.remotes.contains_key(&peer(2)));
        // a malformed control datagram is dropped
        assert!(
            call.step(datagram(VOICE, peer(3), vec![9, 0, 0, 0, 0]), 4_000)
                .is_empty()
        );
    }

    #[cfg_attr(test, test)]
    pub fn a_malformed_client_frame_closes_that_session_only() {
        type Offence = fn(Session) -> Event;
        let refusals: [(Offence, &str); 5] = [
            (
                |s| client_text(s, "{\"type\":\"beacon\",\"muted\":true}"),
                "invalid_call_control",
            ),
            (
                |s| client_text(s, &beacon_text(false, true, true, false)),
                "ambiguous_capture_source",
            ),
            (
                |s| client_binary(s, vec![call_wire::WS_TAG_AUDIO]),
                "invalid_audio_frame",
            ),
            (
                |s| client_binary(s, vec![call_wire::WS_TAG_VIDEO_CAPTURED, 0, 0]),
                "invalid_picture_frame",
            ),
            (|s| client_binary(s, vec![0x09, 1, 2]), "invalid_media_tag"),
        ];
        for (frame, reason) in refusals {
            let mut call = call();
            seated(&mut call, 7, "room", &[peer(2)]);
            seated(&mut call, 8, "room", &[peer(3)]);
            let effects = call.step(frame(7), 0);
            assert_eq!(
                kinds(&effects),
                ["set-roster", "set-roster", "close"],
                "{reason}"
            );
            assert_eq!(closes(&effects), [(7, reason.to_string())]);
            assert_eq!(
                rosters(&effects)[0].2,
                vec![peer(3)],
                "seat 7's peers left admission"
            );
            // the refused session is gone: its later frames are nothing,
            // and the host's own close for it is nothing
            assert!(
                call.step(client_text(7, &beacon_text(false, false, false, false)), 1)
                    .is_empty()
            );
            assert!(call.step(Event::SessionClosed(7), 2).is_empty());
            assert_eq!(call.seats.len(), 1);
        }
        // a frame for a session never opened is nothing, not a close
        let mut call = call();
        assert!(call.step(client_binary(5, vec![0x09]), 0).is_empty());
    }
}
