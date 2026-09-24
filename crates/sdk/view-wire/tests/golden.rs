//! The wire's bytes, committed. One `Frame` holding every `Node` variant,
//! one of every `Event`, and one request and reply through every door in
//! `doors::ALL`, encoded into `tests/golden/{frame,doors}.bin` with a JSON
//! twin beside each for readable diffs. Any byte that moves — a field, a
//! variant, a `gpui::StyleRefinement` change from a fork bump — fails here,
//! and the fix is to bump `WIRE_EPOCH` in the same commit and regenerate
//! with `WIRE_GOLDEN_WRITE=1`.
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::PathBuf;

use gpui::{Bounds, Pixels, StyleRefinement, point, px, size};
use view_wire::doors::{self, Door, Program};
use view_wire::editor_document::{
    EditorDocumentMessage, EditorDocumentRef, EditorTransfer, EditorTransferError, EditorTransferId,
};
use view_wire::editor_presentation::EditorInteraction;
use view_wire::list::{
    ListCommand, UniformListHorizontalSizing, UniformListScrollRequest, UniformListScrollStrategy,
    UniformListSizing,
};
use view_wire::{
    Anchor, AnchoredFitMode, AnchoredPositionMode, Axis, ButtonContent, CanvasCommand, CanvasShape,
    ContainerNode, ContentFit, DispatchPhase, EditorCursor, EditorDecision, EditorEditKind,
    EditorHistoryEffect, EditorPatch, EditorRequest, EditorRequestInput, EditorResponse,
    EditorTransactionEvent, EditorTransactionId, ElementIdWire, Event, Frame, ImageData,
    ImageObjectFit, ImageStyle, Interactivity, ListAlignment, ListOffset, ListRequest, ListScroll,
    ListSizingBehavior, Live, Node, Patch, Qr, Refusal, Request, RichTextHighlightStyle,
    RichTextHover, RichTextRuns, Role, ScrollAnchor, ScrollDirection, SurfaceValue, SvgSource,
    SvgTransformation, TextNode, ToggleKind, TooltipResponse, WidgetCommand, click, events,
    interactivity, keyboard, mouse,
};

const MESSAGE: &str = "the wire changed: bump WIRE_EPOCH and regenerate with WIRE_GOLDEN_WRITE=1";

/// Bumped by hand with the enum: `variant` below fails to compile until
/// the fixture names the new one, and this count keeps the fixture honest.
const NODE_VARIANTS: usize = 34;
const EVENT_VARIANTS: usize = 42;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Compares `actual` to the committed fixture, or rewrites it under
/// `WIRE_GOLDEN_WRITE=1`. `json` is the readable twin, never compared.
fn check(name: &str, actual: &[u8], json: &str) {
    let bin = golden(&format!("{name}.bin"));
    if std::env::var_os("WIRE_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, actual).unwrap();
        std::fs::write(golden(&format!("{name}.json")), json).unwrap();
        return;
    }
    let expected =
        std::fs::read(&bin).unwrap_or_else(|error| panic!("{}: {error}; {MESSAGE}", bin.display()));
    if expected != actual {
        let first = expected
            .iter()
            .zip(actual)
            .position(|(a, b)| a != b)
            .unwrap_or(expected.len().min(actual.len()));
        panic!(
            "{MESSAGE} ({name}.bin: {} bytes expected, {} built, first difference at byte {first})",
            expected.len(),
            actual.len()
        );
    }
}

fn id(name: &str) -> ElementIdWire {
    ElementIdWire::Name(name.into())
}

fn style() -> StyleRefinement {
    StyleRefinement::default()
}

fn text(content: &str) -> Node {
    Node::Text(TextNode {
        id: Some(id(content)),
        style: style(),
        content: content.into(),
        heading: Some(2),
        live: Some(Live::Polite),
    })
}

fn boxed(content: &str) -> Box<Node> {
    Box::new(text(content))
}

fn document(byte_len: u32) -> EditorDocumentRef {
    EditorDocumentRef {
        document: "app:draft".into(),
        reset: 3,
        text_revision: 5,
        revision: 7,
        cursor: EditorCursor::default(),
        byte_len,
    }
}

fn transaction() -> EditorTransactionId {
    EditorTransactionId {
        instance: 1,
        document: "app:draft".into(),
        reset: 0,
        sequence: 2,
        attempt: 0,
        text_revision: 1,
        revision: 3,
    }
}

fn transfer() -> EditorTransferId {
    EditorTransferId {
        instance: 1,
        document: "app:draft".into(),
        reset: 0,
        serial: 4,
        attempt: 1,
    }
}

fn key_state() -> keyboard::KeyState {
    keyboard::KeyState {
        key: keyboard::Key::Named(keyboard::Named::Enter),
        modified_key: keyboard::Key::Character("\n".into()),
        physical_key: keyboard::Physical::Unidentified(keyboard::NativeCode::MacOS(36)),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers {
            shift: true,
            ..Default::default()
        },
    }
}

fn at(x: f32, y: f32) -> gpui::Point<Pixels> {
    point(px(x), px(y))
}

#[path = "golden/events.rs"]
mod events_fixture;
/// Every `Node` variant once, in one tree. Exhaustive by construction: a
/// variant added to `Node` must be added here, or `variant` will not build.
#[path = "golden/nodes.rs"]
mod nodes;
use events_fixture::{event_variant, every_event, every_frame};
use nodes::{every_node, node_variant};

#[test]
fn frame_and_events_are_the_committed_bytes() {
    let frame = every_frame();
    let events = every_event();
    let root = frame.root.as_ref().unwrap();
    let Node::Container(ContainerNode { children, .. }) = root else {
        unreachable!()
    };
    let nodes: BTreeSet<_> = children.iter().chain([root]).map(node_variant).collect();
    assert_eq!(
        nodes.len(),
        NODE_VARIANTS,
        "every Node variant once: {nodes:?}"
    );
    let kinds: BTreeSet<_> = events.iter().map(event_variant).collect();
    assert_eq!(
        kinds.len(),
        EVENT_VARIANTS,
        "every Event variant once: {kinds:?}"
    );

    let value = (frame, events);
    let bytes = view_wire::encode(&value);
    let json = serde_json::to_string_pretty(&value).unwrap();
    check("frame", &bytes, &json);
    assert_eq!(
        view_wire::decode::<(Frame, Vec<Event>)>(&bytes).unwrap(),
        value
    );
}

/// The program a golden `rpc.query`/`op.submit` addresses.
struct Golden;
impl Program for Golden {
    const NAME: &'static str = "golden";
    type Op = String;
    type Query = (u64, String);
    type Reply = Vec<u32>;
}

/// The kind, the request bytes and the reply bytes of one exchange.
type Exchange = (String, Vec<u8>, Vec<u8>);

/// One exchange, plus the values as JSON for the readable twin.
fn exchange<D: Door>(request: D::Request, reply: D::Reply) -> (Exchange, serde_json::Value)
where
    D::Request: serde::Serialize,
    D::Reply: serde::Serialize,
{
    let json = serde_json::json!({ "kind": D::KIND, "request": request, "reply": reply });
    (
        (
            D::KIND.into(),
            D::encode_request(&request),
            D::encode_reply(&reply),
        ),
        json,
    )
}

fn every_door() -> Vec<(Exchange, serde_json::Value)> {
    use doors::*;
    let file = SelectedFile {
        token: "t1".into(),
        name: "a.txt".into(),
        bytes: 5,
    };
    vec![
        exchange::<Query<Golden>>((7, "q".into()), vec![1, 2, 3]),
        exchange::<Submit<Golden>>("op".into(), b"receipt".to_vec()),
        exchange::<Status>(
            (),
            NodeStatus {
                network: "local#1".into(),
                time: 1,
                block_time_ms: 500,
                epoch_length: 100,
                height: 9,
                tip: [1; 32],
                root: [2; 32],
                epoch: 0,
                identity: vec![3; 32],
                contract: 1,
            },
        ),
        exchange::<Invite>(
            Mint { ttl_days: 7 },
            Minted {
                invite: "duck://invite/x".into(),
                notes: vec![Note {
                    reason: "not_yet".into(),
                    sentence: "the invite is not indexed yet".into(),
                }],
            },
        ),
        exchange::<Live>("chat".into(), Some(9)),
        exchange::<Blocks>(
            BlockPage {
                before: Some(10),
                limit: 2,
            },
            vec![Block {
                height: 9,
                id: [4; 32],
                parent: [5; 32],
                time: 1,
                epoch: 0,
                proposer: Some(vec![6; 32]),
                txs: vec![Tx {
                    hash: [7; 32],
                    signer: vec![8; 32],
                    seq: 1,
                    target: "chat".into(),
                    payload: vec![9],
                }],
            }],
        ),
        exchange::<BlockGet>(BlockRef::Id([4; 32]), None),
        exchange::<BlobGet>("sha256:00".into(), b"blob".to_vec()),
        exchange::<Props>(
            (),
            Session {
                connected: true,
                dark: false,
                chain: "local#1".into(),
                account: "alice".into(),
                endpoint: "http://127.0.0.1:1".into(),
            },
        ),
        exchange::<Visible>((), true),
        exchange::<Badge>(3, ()),
        exchange::<OpenLink>("duck://chat/room".into(), ()),
        exchange::<Route>((), "tx/00ff".into()),
        exchange::<Chord>("cmd-k".into(), ()),
        exchange::<Id>("msg".into(), "msg-1".into()),
        exchange::<Ticks>(1000, ()),
        exchange::<Log>("hello".into(), ()),
        exchange::<Widget>(
            WidgetCommand::Focus {
                target: vec![id("input")],
            },
            (),
        ),
        exchange::<Pick>((), vec![file.clone()]),
        exchange::<Drops>((), vec![file.clone()]),
        exchange::<FsRead>(
            ReadRequest {
                token: "t1".into(),
                offset: 0,
                len: 5,
            },
            b"hello".to_vec(),
        ),
        exchange::<Release>("t1".into(), ()),
        exchange::<ClipboardRead>(
            (),
            Clipboard {
                text: "copied".into(),
                files: vec![file],
            },
        ),
        exchange::<ClipboardWrite>("copied".into(), ()),
        exchange::<Devices>(
            (),
            vec![Device {
                id: "mic0".into(),
                kind: "microphone".into(),
                name: "Built-in".into(),
            }],
        ),
        exchange::<AudioCapture>(
            Listen {
                device: Some("mic0".into()),
                rate: Some(48_000),
                channels: Some(1),
            },
            AudioItem::Samples(vec![0, 1]),
        ),
        exchange::<VideoCapture>(
            Watch {
                device: None,
                width: Some(640),
                height: Some(480),
                fps: Some(30),
            },
            VideoItem::Opened(Framing {
                width: 640,
                height: 480,
                fps: 30,
                format: "rgba8".into(),
            }),
        ),
        exchange::<AudioPlay>(
            AudioMode {
                rate: 48_000,
                channels: 2,
            },
            AudioMode {
                rate: 48_000,
                channels: 2,
            },
        ),
        exchange::<AudioWrite>(vec![0, 1, 2, 3], ()),
        exchange::<AudioStop>((), ()),
        exchange::<NotifyShow>(
            Notice {
                title: "Chat".into(),
                body: "alice: hi".into(),
                tag: "room".into(),
            },
            true,
        ),
        exchange::<NotifyPost>(
            Post {
                title: "alice mentioned you".into(),
                body: "@bob hi".into(),
                tag: "room".into(),
                link: "duck://chat/room".into(),
            },
            Posted::Banner,
        ),
    ]
}

#[test]
fn every_door_carries_the_committed_bytes() {
    let (exchanges, json): (Vec<_>, Vec<_>) = every_door().into_iter().unzip();
    let kinds: Vec<&str> = exchanges.iter().map(|(kind, ..)| kind.as_str()).collect();
    assert_eq!(kinds, doors::ALL, "one exchange per door in ALL, in order");
    let bytes = doors::encode(&exchanges);
    check(
        "doors",
        &bytes,
        &serde_json::to_string_pretty(&json).unwrap(),
    );
    assert_eq!(
        doors::decode::<Vec<(String, Vec<u8>, Vec<u8>)>>(&bytes).unwrap(),
        exchanges
    );
}

/// The kinds the app host routes, read from its source (`("<cap>", "<op>")`
/// match arms under `src/runtime`), must be exactly `ALL`. Runs when
/// `DUCKTAPE_APP` names the app checkout; the app decodes `doors::*`
/// generically once its follow-up lands, and this reads its arms until then.
#[test]
fn the_app_host_serves_every_door_and_nothing_else() {
    let Some(app) = std::env::var_os("DUCKTAPE_APP") else {
        eprintln!("DUCKTAPE_APP unset: skipping the app-host coverage check");
        return;
    };
    let mut served = BTreeSet::new();
    let mut stack = vec![PathBuf::from(app).join("src/runtime")];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    served.extend(served_kind(line));
                }
            }
        }
    }
    let all: BTreeSet<String> = doors::ALL.iter().map(|kind| (*kind).to_owned()).collect();
    let unserved: Vec<_> = all.difference(&served).collect();
    let unknown: Vec<_> = served.difference(&all).collect();
    assert!(
        unserved.is_empty() && unknown.is_empty(),
        "doors the app does not serve: {unserved:?}; kinds the app serves that are not doors: {unknown:?}"
    );
}

/// `("host", "badge")` on a line → `host.badge`.
fn served_kind(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("(\"")?;
    let (capability, rest) = rest.split_once("\", \"")?;
    let (operation, _) = rest.split_once("\")")?;
    let word = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    (word(capability) && word(operation)).then(|| format!("{capability}.{operation}"))
}
