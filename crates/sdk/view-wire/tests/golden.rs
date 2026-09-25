//! The wire's bytes, committed. One `Frame` holding every `Node` variant,
//! one of every `Event`, and one request and reply through every door in
//! `doors::ALL`, encoded into `tests/golden/{frame,doors}.bin` with a JSON
//! twin beside each for readable diffs. Any byte of the frame that moves — a
//! field, a variant, a `gpui::StyleRefinement` change from a fork bump —
//! fails here. The doors are a map keyed by kind: an existing kind whose
//! bytes moved, or a kind that went away, fails; a kind new since the
//! fixture passes, since no view built before it can call it. A failure is
//! fixed by bumping `WIRE_EPOCH` in the same commit and regenerating with
//! `WIRE_GOLDEN_WRITE=1`, which also records new kinds.
use std::collections::{BTreeMap, BTreeSet};
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

/// The program a golden `rpc.query`/`op.submit`/`rpc.live` addresses.
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
    vec![
        exchange::<Query<Golden>>((7, "q".into()), vec![1, 2, 3]),
        exchange::<Submit<Golden>>("op".into(), b"receipt".to_vec()),
        exchange::<RpcStatus>(
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
        exchange::<RpcInvite>(
            Mint { ttl_days: 7 },
            Minted {
                invite: "duck://invite/x".into(),
                notes: vec![Note {
                    reason: "not_yet".into(),
                    sentence: "the invite is not indexed yet".into(),
                }],
            },
        ),
        exchange::<doors::Live<Golden>>((), Some(9)),
        exchange::<RpcBlocks>(
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
        exchange::<RpcBlock>(BlockRef::Id([4; 32]), None),
        exchange::<BlobGet>("sha256:00".into(), Some(b"blob".to_vec())),
        exchange::<HostProps>(
            (),
            Session {
                connected: true,
                dark: false,
                chain: "local#1".into(),
                key: "ab01".into(),
                account: Some(3),
                endpoint: "http://127.0.0.1:1".into(),
            },
        ),
        exchange::<HostVisible>((), true),
        exchange::<HostBadge>(3, ()),
        exchange::<HostOpenLink>("duck://chat/room".into(), ()),
        exchange::<HostRoute>((), "tx/00ff".into()),
        exchange::<HostId>("msg".into(), "msg-1".into()),
        exchange::<ClockTicks>(1000, ()),
        exchange::<HostLog>("hello".into(), ()),
        exchange::<HostWidget>(
            WidgetCommand::Focus {
                target: vec![id("input")],
            },
            (),
        ),
        exchange::<ClipboardRead>(
            (),
            Clipboard {
                text: "copied".into(),
            },
        ),
        exchange::<ClipboardWrite>("copied".into(), ()),
        exchange::<NotifyPost>(
            Post {
                title: "alice mentioned you".into(),
                body: "@bob hi".into(),
                tag: "room".into(),
                link: "duck://chat/room".into(),
            },
            Posted::Banner,
        ),
        exchange::<StoreGet>("reads/alice".into(), Some(vec![1, 2])),
        exchange::<StoreSet>(("reads/alice".into(), None), ()),
        exchange::<NotifyRead>("#design".into(), ()),
        exchange::<RpcHeads>(
            (),
            Head {
                height: 9,
                time: 1,
                id: [4; 32],
            },
        ),
        exchange::<ProgramDescribe>(
            ("chat".into(), vec![1, 2]),
            Some(Description {
                title: "Post in #design".into(),
                fields: vec![Field {
                    label: "from".into(),
                    value: Value::List(vec![Value::Account(3), Value::bytes(&[7; 40])]),
                }],
            }),
        ),
    ]
}

/// The request and reply bytes of each door, by kind.
type Doors = BTreeMap<String, (Vec<u8>, Vec<u8>)>;

#[test]
fn every_door_carries_the_committed_bytes() {
    let (exchanges, json): (Vec<_>, Vec<_>) = every_door().into_iter().unzip();
    let built: Doors = exchanges
        .into_iter()
        .map(|(kind, request, reply)| (kind, (request, reply)))
        .collect();
    let kinds: BTreeSet<&str> = built.keys().map(String::as_str).collect();
    let all: BTreeSet<&str> = doors::ALL.iter().copied().collect();
    assert_eq!(kinds, all, "one exchange per door in ALL");
    let bin = golden("doors.bin");
    if std::env::var_os("WIRE_GOLDEN_WRITE").is_some() {
        std::fs::write(&bin, doors::encode(&built)).unwrap();
        std::fs::write(
            golden("doors.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
        return;
    }
    let committed: Doors = doors::decode(
        &std::fs::read(&bin)
            .unwrap_or_else(|error| panic!("{}: {error}; {MESSAGE}", bin.display())),
    )
    .unwrap();
    assert_eq!(moved(&committed, &built), Vec::<String>::new(), "{MESSAGE}");
}

/// The committed kinds whose bytes `built` changed or dropped. A kind only
/// `built` has is new, and passes.
fn moved(committed: &Doors, built: &Doors) -> Vec<String> {
    committed
        .iter()
        .filter(|(kind, bytes)| built.get(*kind) != Some(bytes))
        .map(|(kind, _)| kind.clone())
        .collect()
}

#[test]
fn a_new_door_passes_and_a_moved_or_dropped_one_fails() {
    let door = |kind: &str, byte: u8| (kind.to_string(), (vec![byte], vec![]));
    let committed: Doors = [door("a.one", 1), door("a.two", 2)].into();
    let grown: Doors = [door("a.one", 1), door("a.two", 2), door("a.new", 3)].into();
    assert!(moved(&committed, &grown).is_empty());
    let changed: Doors = [door("a.one", 9), door("a.two", 2)].into();
    assert_eq!(moved(&committed, &changed), ["a.one"]);
    let dropped: Doors = [door("a.one", 1)].into();
    assert_eq!(moved(&committed, &dropped), ["a.two"]);
}
