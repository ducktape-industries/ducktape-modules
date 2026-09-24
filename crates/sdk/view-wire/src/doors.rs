//! Every kind a view may ask its host for, with the request and reply each
//! carries. This is the ONE list: a view names a door by its type, the host
//! answers by the same type, and a kind that is not here is a compile error
//! on one side and `unknown_request` on the other. [`ALL`] is what a host
//! test checks its handlers against.
//!
//! THE CODEC RULE. Two layers cross the guest boundary and they want
//! opposite things. The tree a view draws (`Frame`, [`WidgetCommand`]) must
//! tolerate a hundred optional fields and is decoded under a budget — that
//! is named MessagePack, in `codec`. Everything a door carries is DATA: the
//! bytes a program signs, stores or answers with, where the same value must
//! be the same bytes and an unknown field is a fault. That is borsh, the
//! codec the program abi is written in, so a program's own request rides a
//! door with no second encoding around it. The rule is held by types, not
//! by review: [`Door`] is sealed, so a view cannot declare a kind or pick a
//! codec, and [`Program`]'s bounds are borsh, so a program that speaks
//! anything else does not have a door.
//!
//! ABSENT is `None`, never a refusal: a door whose thing may not exist replies `Option`, and a refusal means the ask itself failed.
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::WidgetCommand;

mod sealed {
    pub trait Sealed {}
}

/// One kind a view may ask for. Sealed: the doors are the ones in this
/// module.
pub trait Door: sealed::Sealed {
    const KIND: &'static str;
    /// The program a node door is addressed to, so a test host can key on
    /// it; `None` for the host's own doors.
    const TARGET: Option<&'static str> = None;
    type Request: std::fmt::Debug;
    type Reply;
    fn encode_request(request: &Self::Request) -> Vec<u8>;
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, String>;
    fn encode_reply(reply: &Self::Reply) -> Vec<u8>;
    fn decode_reply(bytes: &[u8]) -> Result<Self::Reply, String>;
}

pub fn encode<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).expect("borsh encodes an in-memory value")
}

pub fn decode<T: BorshDeserialize>(bytes: &[u8]) -> Result<T, String> {
    borsh::from_slice(bytes).map_err(|error| error.to_string())
}

macro_rules! door {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty) => {
        $(#[$doc])*
        pub struct $name;
        impl sealed::Sealed for $name {}
        impl Door for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
            fn encode_request(request: &$request) -> Vec<u8> {
                encode(request)
            }
            fn decode_request(bytes: &[u8]) -> Result<$request, String> {
                decode(bytes)
            }
            fn encode_reply(reply: &$reply) -> Vec<u8> {
                encode(reply)
            }
            fn decode_reply(bytes: &[u8]) -> Result<$reply, String> {
                decode(bytes)
            }
        }
    };
}

/// Every [`door!`] below, and [`ALL`] from the same list, so a door is
/// never declared without being listed. `also` names the kinds written by
/// hand: the two node doors generic over a [`Program`], and [`HostWidget`].
macro_rules! doors {
    (
        also: [$($also:expr),* $(,)?];
        $($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty;)*
    ) => {
        $(door!($(#[$doc])* $name, $kind, $request, $reply);)*

        /// Every kind, so a host can assert it answers each one.
        pub const ALL: &[&str] = &[$($also,)* $($kind),*];

        /// Which doors a view was built against, in its manifest, so a host
        /// with fewer refuses it at load rather than at the call. Within a
        /// wire epoch the doors only grow (a moved or dropped one is an epoch
        /// bump: `tests/golden.rs`), so their count names the set.
        pub const DOORS_REVISION: u32 = ALL.len() as u32;
    };
}

// ---------- the node ----------

/// A program a view talks to: its name on the node and the types it speaks.
/// Implemented next to the view (a marker type), never by the program
/// crate, which must not link a view runtime. A read-only program names
/// `()` as its `Op`.
pub trait Program {
    const NAME: &'static str;
    type Op: BorshSerialize + BorshDeserialize + std::fmt::Debug;
    type Query: BorshSerialize + BorshDeserialize + std::fmt::Debug;
    type Reply: BorshSerialize + BorshDeserialize;
}

/// The envelope of a node door: the program addressed and the bytes it
/// gets, which the host signs into a frame without reading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Call {
    pub target: String,
    pub body: Vec<u8>,
}

fn decode_call<T: BorshDeserialize>(bytes: &[u8], target: &str) -> Result<T, String> {
    let call: Call = decode(bytes)?;
    if call.target != target {
        return Err(format!("expected target {target}, got {}", call.target));
    }
    decode(&call.body)
}

/// `rpc.query`: one query to `P`, answered with the bytes it `Respond`ed.
pub struct Query<P>(std::marker::PhantomData<P>);
impl<P: Program> sealed::Sealed for Query<P> {}
impl<P: Program> Door for Query<P> {
    const KIND: &'static str = "rpc.query";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = P::Query;
    type Reply = P::Reply;
    fn encode_request(request: &P::Query) -> Vec<u8> {
        encode(&Call {
            target: P::NAME.into(),
            body: encode(request),
        })
    }
    fn decode_request(bytes: &[u8]) -> Result<P::Query, String> {
        decode_call(bytes, P::NAME)
    }
    fn encode_reply(reply: &P::Reply) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<P::Reply, String> {
        decode(bytes)
    }
}

/// `op.submit`: one operation to `P`, signed with the seated key; the
/// reply is the receipt's output, the program's own bytes.
pub struct Submit<P>(std::marker::PhantomData<P>);
impl<P: Program> sealed::Sealed for Submit<P> {}
impl<P: Program> Door for Submit<P> {
    const KIND: &'static str = "op.submit";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = P::Op;
    type Reply = Vec<u8>;
    fn encode_request(request: &P::Op) -> Vec<u8> {
        encode(&Call {
            target: P::NAME.into(),
            body: encode(request),
        })
    }
    fn decode_request(bytes: &[u8]) -> Result<P::Op, String> {
        decode_call(bytes, P::NAME)
    }
    fn encode_reply(reply: &Vec<u8>) -> Vec<u8> {
        reply.clone()
    }
    fn decode_reply(bytes: &[u8]) -> Result<Vec<u8>, String> {
        Ok(bytes.to_vec())
    }
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct NodeStatus {
    pub network: String,
    pub time: u64,
    pub block_time_ms: u64,
    pub epoch_length: u64,
    pub height: u64,
    pub tip: [u8; 32],
    pub root: [u8; 32],
    pub epoch: u64,
    pub identity: Vec<u8>,
    pub contract: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Mint {
    pub ttl_days: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Note {
    pub reason: String,
    pub sentence: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Minted {
    pub invite: String,
    pub notes: Vec<Note>,
}
/// A page of finalized blocks, newest first: those below `before` (from the
/// tip when `None`), at most `limit` (the node caps a page at 100).
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct BlockPage {
    pub before: Option<u64>,
    pub limit: u32,
}
/// One finalized block, by height or by its id (the block digest).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum BlockRef {
    Height(u64),
    Id([u8; 32]),
}
/// One applied frame of a block. `hash` is sha256 over the frame's exact
/// bytes; `payload` is the op the target program was handed.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Tx {
    pub hash: [u8; 32],
    pub signer: Vec<u8>,
    pub seq: u64,
    pub target: String,
    pub payload: Vec<u8>,
}
/// A finalized block as the node's archive keeps it. `proposer` is the
/// validator key that led its round, where the node holds its certificate.
/// No state root or writes: the node keeps neither per height.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Block {
    pub height: u64,
    pub id: [u8; 32],
    pub parent: [u8; 32],
    pub time: u64,
    pub epoch: u64,
    pub proposer: Option<Vec<u8>>,
    pub txs: Vec<Tx>,
}
// ---------- the host ----------

/// The session facts every view is handed: the theme, the connection, the
/// network (`<label>#<salt>`), the seated account handle and the read-only
/// endpoint.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Session {
    pub connected: bool,
    pub dark: bool,
    pub chain: String,
    pub account: String,
    pub endpoint: String,
}
/// `host.widget`: a command on the mounted tree. The one door on the TREE
/// side of the codec rule — a [`WidgetCommand`] names typed element ids the
/// tree is drawn with — so it is the one door in named MessagePack.
pub struct HostWidget;
impl sealed::Sealed for HostWidget {}
impl Door for HostWidget {
    const KIND: &'static str = "host.widget";
    type Request = WidgetCommand;
    type Reply = ();
    fn encode_request(request: &WidgetCommand) -> Vec<u8> {
        crate::encode(request)
    }
    fn decode_request(bytes: &[u8]) -> Result<WidgetCommand, String> {
        crate::decode(bytes)
    }
    fn encode_reply(_: &()) -> Vec<u8> {
        Vec::new()
    }
    fn decode_reply(_: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

// ---------- the device ----------

/// A file the person granted, readable through [`FsRead`] by token until
/// released ([`FsRelease`]).
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct SelectedFile {
    pub token: String,
    pub name: String,
    pub bytes: u64,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct ReadRequest {
    pub token: String,
    pub offset: u64,
    pub len: u64,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Clipboard {
    pub text: String,
    pub files: Vec<SelectedFile>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Device {
    pub id: String,
    /// `"microphone"`, `"speaker"` or `"camera"`.
    pub kind: String,
    pub name: String,
}
/// What `audio.capture` may ask for; the device's own mode is used unless
/// it offers exactly this, and the stream's first item says what opened.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Listen {
    pub device: Option<String>,
    pub rate: Option<u32>,
    pub channels: Option<u8>,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Watch {
    pub device: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u8>,
}
/// An audio mode: what `audio.play` asks for, what a capture or a playout
/// opened in.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct AudioMode {
    pub rate: u32,
    pub channels: u8,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Framing {
    pub width: u32,
    pub height: u32,
    pub fps: u8,
    /// The pixel format of every frame, e.g. `"rgba8"`.
    pub format: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum AudioItem {
    Opened(AudioMode),
    /// Interleaved i16 little-endian samples in the opened mode.
    Samples(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum VideoItem {
    Opened(Framing),
    Frame(Vec<u8>),
}
/// One notice for the host to decide on: `notify.post`. The view asks; the
/// host logs it in its notification centre and decides whether a banner
/// reaches the screen (the person's per-view choice, focus, a burst limit).
/// `link` is a `duck://` link the centre opens when the notice is picked,
/// or empty.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Post {
    pub title: String,
    pub body: String,
    pub tag: String,
    pub link: String,
}
/// What the host did with a [`Post`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum Posted {
    /// Logged, and a banner was raised.
    Banner,
    /// Logged in the centre only: no banner (not yet allowed, silenced,
    /// in front, over the burst limit, or banners are off).
    Logged,
    /// The person blocked this view's notices: dropped, not logged.
    Blocked,
}
doors! {
    also: ["rpc.query", "op.submit", HostWidget::KIND];
    /// `rpc.status`: the connected node's status.
    RpcStatus, "rpc.status", (), NodeStatus;
    /// `rpc.invite`: mint one invite, once (never retried).
    RpcInvite, "rpc.invite", Mint, Minted;
    /// `rpc.live <program>`: one item per block that wrote to the program,
    /// carrying its height; `None` when the node link was reopened and the
    /// view should re-read.
    RpcLive, "rpc.live", String, Option<u64>;
    /// `rpc.blocks`: a page of finalized blocks, newest first.
    RpcBlocks, "rpc.blocks", BlockPage, Vec<Block>;
    /// `rpc.block`: one finalized block; `None` where the node has none by
    /// that name.
    RpcBlock, "rpc.block", BlockRef, Option<Block>;
    /// `blob.get`: a blob by `sha256:<hex>` or `sha1:<hex>` id, unframed.
    BlobGet, "blob.get", String, Vec<u8>;
    /// `host.props`: a subscription to [`Session`], an item per change.
    HostProps, "host.props", (), Session;
    /// `host.visible`: whether the view is on screen, an item per change.
    HostVisible, "host.visible", (), bool;
    /// `host.badge`: the count on the view's tab.
    HostBadge, "host.badge", i64, ();
    /// `host.open_link`: the one way out, a `duck://` link.
    HostOpenLink, "host.open_link", String, ();
    /// `host.route`: a subscription, one item per `duck://` link opened into
    /// this view: the path after the view's own segment (`tx/<hash>` of
    /// `duck://<chain>/explorer/tx/<hash>`), segments of `[A-Za-z0-9._-]`.
    /// A link that mounted the view is its first item.
    HostRoute, "host.route", (), String;
    /// `host.chord`: claim a command chord (`cmd[-shift][-alt]-<key>`); an
    /// item per press while the subscription stands.
    HostChord, "host.chord", String, ();
    /// `host.id`: a fresh id under the named prefix.
    HostId, "host.id", String, String;
    /// `clock.ticks`: an item per period, in milliseconds.
    ClockTicks, "clock.ticks", i64, ();
    /// `host.log`: one line to the host's log.
    HostLog, "host.log", String, ();
    /// `fs.pick`: the file chooser, answered with what the person chose.
    FsPick, "fs.pick", (), Vec<SelectedFile>;
    /// `fs.drops`: an item per drop onto the view.
    FsDrops, "fs.drops", (), Vec<SelectedFile>;
    /// `fs.read`: one chunk of a granted file.
    FsRead, "fs.read", ReadRequest, Vec<u8>;
    /// `fs.release`: give a grant back.
    FsRelease, "fs.release", String, ();
    /// `clipboard.read`: the clipboard's text and any files on it.
    ClipboardRead, "clipboard.read", (), Clipboard;
    /// `clipboard.write`: text onto the clipboard.
    ClipboardWrite, "clipboard.write", String, ();
    /// `media.devices`: the capture and playout devices this machine has.
    MediaDevices, "media.devices", (), Vec<Device>;
    /// `audio.capture`: the microphone, first the mode then the samples.
    AudioCapture, "audio.capture", Listen, AudioItem;
    /// `video.capture`: the camera, first the framing then the frames.
    VideoCapture, "video.capture", Watch, VideoItem;
    /// `audio.play`: open this view's one output in a mode.
    AudioPlay, "audio.play", AudioMode, AudioMode;
    /// `audio.write`: interleaved i16 little-endian samples onto the output;
    /// refused past the playout ceiling rather than queued.
    AudioWrite, "audio.write", Vec<u8>, ();
    /// `audio.stop`: close the output.
    AudioStop, "audio.stop", (), ();
    /// `notify.post`: hand the host a notice; it says what it did.
    NotifyPost, "notify.post", Post, Posted;
    /// `store.get`: the value this view keeps under a key on this device,
    /// for the network in hand; `None` where it keeps none. A view sees
    /// only its own keys, and only on the network it runs on.
    StoreGet, "store.get", String, Option<Vec<u8>>;
    /// `store.set`: keep a value under a key, or drop it with `None`.
    StoreSet, "store.set", (String, Option<Vec<u8>>), ();
}

/// The `<capability>` half of every kind in [`ALL`]: the names a view's
/// manifest may declare. `export_view!` refuses any other at compile time.
pub const CAPABILITIES: &[&str] = &[
    "rpc",
    "op",
    "blob",
    "host",
    "clock",
    "fs",
    "clipboard",
    "media",
    "audio",
    "video",
    "notify",
    "store",
];

/// Whether `name` is in [`CAPABILITIES`]; `const` so a manifest literal is
/// checked where it is written.
pub const fn is_capability(name: &str) -> bool {
    let name = name.as_bytes();
    let mut index = 0;
    while index < CAPABILITIES.len() {
        let known = CAPABILITIES[index].as_bytes();
        if known.len() == name.len() {
            let mut byte = 0;
            while byte < known.len() && known[byte] == name[byte] {
                byte += 1;
            }
            if byte == known.len() {
                return true;
            }
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_exactly_the_prefixes_of_every_kind() {
        let mut prefixes: Vec<&str> = ALL
            .iter()
            .map(|kind| kind.split_once('.').unwrap().0)
            .collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        let mut known = CAPABILITIES.to_vec();
        known.sort_unstable();
        assert_eq!(prefixes, known);
        assert!(is_capability("rpc") && is_capability("notify"));
        assert!(!is_capability("chat") && !is_capability("rpc.query") && !is_capability(""));
    }

    struct Binary;
    impl Program for Binary {
        const NAME: &'static str = "binary";
        type Op = ();
        type Query = (u64, String);
        type Reply = Vec<u32>;
    }

    #[test]
    fn program_doors_address_their_program_and_carry_its_bytes() {
        let request = (42, "query".to_owned());
        let bytes = Query::<Binary>::encode_request(&request);
        let call: Call = decode(&bytes).unwrap();
        assert_eq!(call.target, "binary");
        assert_eq!(call.body, encode(&request));
        assert_eq!(Query::<Binary>::decode_request(&bytes).unwrap(), request);
        let other = encode(&Call {
            target: "other".into(),
            body: encode(&request),
        });
        assert!(Query::<Binary>::decode_request(&other).is_err());
        let reply = vec![1, 2, 3];
        assert_eq!(
            Query::<Binary>::decode_reply(&Query::<Binary>::encode_reply(&reply)).unwrap(),
            reply
        );
    }

    #[test]
    fn every_kind_is_listed_once() {
        let mut kinds = ALL.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), ALL.len());
        assert!(ALL.iter().all(|kind| kind.split_once('.').is_some()));
    }
}
