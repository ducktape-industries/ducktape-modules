//! Component probe for init, every event, and ordered returned effects.
use ducktape_lane_sdk::{host, types::*, Guest, PEER_LEN};
struct Echo;
impl Guest for Echo {
    fn init(config: Config) -> Result<(), String> {
        if config.self_peer.len() != PEER_LEN {
            return Err("invalid_peer".into());
        }
        host::log(host::Level::Info, "probe", "initialized");
        Ok(())
    }
    fn step(event: Event, now_ms: u64) -> Vec<Effect> {
        match event {
            Event::Tick if now_ms % 2 == 1 => vec![
                Effect::OpenFlow(OpenFlow {
                    lane: 1,
                    flow: now_ms,
                    max_queued: 8,
                }),
                Effect::Log(LogLine {
                    level: host::Level::Info,
                    message: now_ms.to_string(),
                }),
            ],
            Event::Tick => vec![],
            Event::Datagram(d) => vec![Effect::LaneSend(LaneSend {
                lane: d.lane,
                peer: d.peer,
                bytes: d.bytes,
            })],
            Event::ClientFrame(c) => vec![Effect::ClientSend(ClientSend {
                session: c.session,
                frame: c.frame,
            })],
            Event::Roster(r) => vec![Effect::SetRoster(SetRoster {
                lane: 1,
                flow: r.session,
                peers: r.peers,
            })],
            Event::SessionOpened(s) => vec![Effect::Log(LogLine {
                level: host::Level::Info,
                message: s.channel,
            })],
            Event::SessionClosed(session) => vec![
                Effect::CloseFlow(CloseFlow {
                    lane: 1,
                    flow: session,
                }),
                Effect::Close(Close {
                    session,
                    reason: "session_closed".into(),
                }),
            ],
        }
    }
}
ducktape_lane_sdk::export_media!(Echo);
