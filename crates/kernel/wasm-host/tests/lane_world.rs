use sha2::{Digest, Sha256};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store};

mod bindings {
    wasmtime::component::bindgen!({
        world: "media",
        path: "../../lane-sdk/wit",
    });
}

use bindings::Media;
use bindings::ducktape::lane::{host, types::*};

// Deliberately pinned test input. CI exercises these bytes without rebuilding them.
// Source: crates/guests/lane-echo-wasm, repository rust-toolchain.toml.
const ECHO: &[u8] = include_bytes!("fixtures/lane-echo.component.wasm");

/// the sha256 of `crates/module-sdk/wit/module.wit` at the base this world
/// branched from. The lane world lives in its own package directory precisely
/// so this never moves: a module guest needs no ABI rebuild.
const MODULE_WORLD_SHA256: &str =
    "0e0845bedda9e93c4d6ffd5e06a0d0abc9a7468ec2ac4a060cbb38bf4f182280";

/// Only logging is callable; sends are returned effects.
#[derive(Default)]
struct Imports {
    calls: Vec<&'static str>,
}

impl host::Host for Imports {
    fn log(&mut self, _: host::Level, _: String, _: String) {
        self.calls.push("log");
    }
}
impl bindings::ducktape::lane::types::Host for Imports {}

fn instantiate() -> (Store<Imports>, Media) {
    let engine = Engine::default();
    let component = Component::new(&engine, ECHO).expect("lane-echo is a component");
    let mut linker = Linker::new(&engine);
    Media::add_to_linker::<Imports, HasSelf<Imports>>(&mut linker, |d| d).expect("link");
    let mut store = Store::new(&engine, Imports::default());
    let media = Media::instantiate(&mut store, &component, &linker).expect("instantiate");
    (store, media)
}

fn step(store: &mut Store<Imports>, media: &Media, event: &Event, now_ms: u64) -> Vec<Effect> {
    media.call_step(store, event, now_ms).expect("step")
}

#[test]
fn module_world_is_untouched() {
    let module = include_str!("../../../module-sdk/wit/module.wit");
    let digest: String = Sha256::digest(module.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, MODULE_WORLD_SHA256);
    assert_eq!(wasm_host::module_world_digest(), MODULE_WORLD_SHA256);
    let lane = include_bytes!("../../../lane-sdk/wit/media.wit");
    assert_eq!(
        format!("{:x}", Sha256::digest(lane)),
        "f42dbb808e29993c3723e6cc0053149b24e5492f6e3c6be3f2f132072ff7d4d6"
    );
}

#[test]
fn component_surface_has_only_logging_import_and_init_step_exports() {
    assert_eq!(
        format!("{:x}", Sha256::digest(ECHO)),
        "c604ffb7e4822a485cfb6fa22d13885b729831d7cb52e6cfbc295e08bdd91401"
    );
    let engine = Engine::default();
    let component = Component::new(&engine, ECHO).expect("component");
    let ty = component.component_type();
    let imports: Vec<_> = ty
        .imports(&engine)
        .map(|(name, _)| name.to_string())
        .collect();
    let exports: Vec<_> = ty
        .exports(&engine)
        .map(|(name, _)| name.to_string())
        .collect();
    assert_eq!(
        imports,
        ["ducktape:lane/host@0.1.0", "ducktape:lane/types@0.1.0"]
    );
    assert_eq!(exports, ["init", "step"]);
}

#[test]
fn init_and_every_event_cross_the_component_boundary() {
    let (mut store, media) = instantiate();
    let peer = vec![7; 32];
    let config = Config {
        self_peer: peer.clone(),
        lanes: vec![LaneBinding {
            name: "voice".into(),
            id: 1,
        }],
    };
    assert_eq!(media.call_init(&mut store, &config).unwrap(), Ok(()));
    assert_eq!(store.data().calls, ["log"]);
    let invalid = Config {
        self_peer: vec![],
        lanes: vec![],
    };
    assert_eq!(
        media.call_init(&mut store, &invalid).unwrap(),
        Err("invalid_peer".into())
    );
    assert!(step(&mut store, &media, &Event::Tick, 2).is_empty());
    let effects = step(&mut store, &media, &Event::Tick, 3);
    assert!(
        matches!(&effects[..], [Effect::OpenFlow(f), Effect::Log(l)] if f.flow == 3 && f.max_queued == 8 && l.message == "3")
    );
    let effects = step(
        &mut store,
        &media,
        &Event::Datagram(Datagram {
            lane: 1,
            peer: peer.clone(),
            bytes: b"opus".to_vec(),
        }),
        4,
    );
    assert!(
        matches!(&effects[..], [Effect::LaneSend(s)] if s.lane == 1 && s.peer == peer && s.bytes == b"opus")
    );
    let session = u64::from(u32::MAX) + 1;
    for frame in [Frame::Text("hello".into()), Frame::Binary(b"pcm".to_vec())] {
        let effects = step(
            &mut store,
            &media,
            &Event::ClientFrame(ClientFrame {
                session,
                frame: frame.clone(),
            }),
            5,
        );
        let [Effect::ClientSend(s)] = &effects[..] else {
            panic!("client send");
        };
        assert_eq!(s.session, session);
        match (&s.frame, &frame) {
            (Frame::Text(a), Frame::Text(b)) => assert_eq!(a, b),
            (Frame::Binary(a), Frame::Binary(b)) => assert_eq!(a, b),
            _ => panic!("frame kind changed"),
        }
    }
    let effects = step(
        &mut store,
        &media,
        &Event::Roster(Roster {
            session,
            peers: vec![peer.clone()],
        }),
        6,
    );
    assert!(
        matches!(&effects[..], [Effect::SetRoster(r)] if r.flow == session && r.peers == [peer])
    );
    let effects = step(
        &mut store,
        &media,
        &Event::SessionOpened(SessionOpened {
            session,
            channel: "audio".into(),
        }),
        7,
    );
    assert!(matches!(&effects[..], [Effect::Log(l)] if l.message == "audio"));
    let effects = step(&mut store, &media, &Event::SessionClosed(session), 8);
    assert!(
        matches!(&effects[..], [Effect::CloseFlow(f), Effect::Close(c)] if f.flow == session && c.session == session && c.reason == "session_closed")
    );
    // Sends never call imports; only init logged, across all steps on this instance.
    assert_eq!(store.data().calls, ["log"]);
}
