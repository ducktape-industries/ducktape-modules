//! Every kind a view may ask its host for, with the request and reply each
//! carries. This is the ONE list: a view names a method by its type, the host
//! answers by the same type, and a kind that is not here is a compile error
//! on one side and `unknown_request` on the other. [`ALL`] is what a host
//! test checks its handlers against.
//!
//! THE CODEC RULE. Two layers cross the guest boundary and they want
//! opposite things. The tree a view draws (`Frame`, [`WidgetCommand`]) must
//! tolerate a hundred optional fields and is decoded under a budget — that
//! is named MessagePack, in `codec`. Everything a method carries is DATA: the
//! bytes a program signs, stores or answers with, where the same value must
//! be the same bytes and an unknown field is a fault. That is borsh, the
//! codec the program abi is written in, so a program's own request rides a
//! method with no second encoding around it. The rule is held by types, not
//! by review: [`Method`] is sealed, so a view cannot declare a kind or pick a
//! codec, and [`Module`]'s bounds are borsh, so a program that speaks
//! anything else does not have a method.
//!
//! ABSENT is `None`, never a refusal: a method whose thing may not exist replies `Option`, and a refusal means the ask itself failed.
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::WidgetCommand;
pub use describe::{Description, Field, Value};

mod sealed {
    pub trait Sealed {}
}

/// One kind a view may ask for. Sealed: the methods are the ones in this
/// module.
pub trait Method: sealed::Sealed {
    const KIND: &'static str;
    /// The program a node method is addressed to, so a test host can key on
    /// it; `None` for the host's own methods.
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

macro_rules! method {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty) => {
        $(#[$doc])*
        pub struct $name;
        impl sealed::Sealed for $name {}
        impl Method for $name {
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

/// Every [`method!`] below, and [`ALL`] from the same list, so a method is
/// never declared without being listed. `also` names the kinds written by
/// hand: the three node methods generic over a [`Module`], and [`HostWidget`].
macro_rules! methods {
    (
        also: [$($also:expr),* $(,)?];
        $($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty;)*
    ) => {
        $(method!($(#[$doc])* $name, $kind, $request, $reply);)*

        /// Every kind, so a host can assert it answers each one.
        pub const ALL: &[&str] = &[$($also,)* $($kind),*];

        /// Which methods a view was built against, in its manifest, so a host
        /// with fewer refuses it at load rather than at the call. Within a
        /// wire epoch the methods only grow (a moved or dropped one is a new
        /// epoch: `tests/golden.rs`), so their count names the set.
        pub const METHODS_REVISION: u32 = ALL.len() as u32;
    };
}

// ---------- the node ----------

/// A program a view talks to: its name on the node and the types it speaks.
/// Implemented next to the view (a marker type), never by the program
/// crate, which must not link a view runtime. A read-only program names
/// `()` as its `Op`.
pub trait Module {
    const NAME: &'static str;
    type Op: BorshSerialize + BorshDeserialize + std::fmt::Debug;
    type Query: BorshSerialize + BorshDeserialize + std::fmt::Debug;
    type Reply: BorshSerialize + BorshDeserialize;
}

/// The envelope of a node method: the program addressed and the bytes it
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

/// `module.query`: one query to `P`, answered with the bytes it `Respond`ed.
pub struct Query<P>(std::marker::PhantomData<P>);
impl<P: Module> sealed::Sealed for Query<P> {}
impl<P: Module> Method for Query<P> {
    const KIND: &'static str = "module.query";
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
impl<P: Module> sealed::Sealed for Submit<P> {}
impl<P: Module> Method for Submit<P> {
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

/// `module.changes`: a subscription to `P`, one item per block that wrote to it,
/// carrying its height; `None` when the node link was reopened and the view
/// should re-read. The request is `P`'s name, as the host reads it.
pub struct Changes<P>(std::marker::PhantomData<P>);
impl<P: Module> sealed::Sealed for Changes<P> {}
impl<P: Module> Method for Changes<P> {
    const KIND: &'static str = "module.changes";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = ();
    type Reply = Option<u64>;
    fn encode_request(_: &()) -> Vec<u8> {
        encode(&P::NAME.to_owned())
    }
    fn decode_request(bytes: &[u8]) -> Result<(), String> {
        let name: String = decode(bytes)?;
        match name == P::NAME {
            true => Ok(()),
            false => Err(format!("expected program {}, got {name}", P::NAME)),
        }
    }
    fn encode_reply(reply: &Option<u64>) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<Option<u64>, String> {
        decode(bytes)
    }
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct NodeStatus {
    pub chain_id: String,
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
pub struct CreateInvite {
    pub ttl_days: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Invite {
    pub invite: String,
    pub notes: Vec<crate::Error>,
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
/// bytes; `payload` is the op the target program was handed. `receipt` is
/// its run as the node kept it when it applied the block; `None` where the
/// node keeps none (it never ran the block: one below its state-sync anchor).
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Tx {
    pub hash: [u8; 32],
    pub signer: Vec<u8>,
    pub seq: u64,
    pub target: String,
    pub payload: Vec<u8>,
    pub receipt: Option<Receipt>,
}
/// One run: the program's, how it ended, what it announced, and the runs
/// its messages caused, in order. A nested run's `Applied` stands only
/// where every run above it applied too: an ancestor's rejection undid it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Receipt {
    pub program: String,
    pub outcome: Outcome,
    pub events: Vec<Vec<u8>>,
    pub nested: Vec<Receipt>,
}
/// How a run ended: applied with the program's output, or rejected with
/// its refusal (`code` the refusal's reason token, `message` its sentence).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum Outcome {
    Applied { output: Vec<u8> },
    Rejected(crate::Error),
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
/// One finalized head, as `chain.heads` pushes it.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Head {
    pub height: u64,
    pub time: u64,
    pub id: [u8; 32],
}
// ---------- the host ----------

/// The session facts every view is handed: the theme, the connection, the
/// network (`<label>#<salt>`), the seated key (hex), the account it belongs
/// to (`None` while the host has not resolved one, or the key has none yet;
/// an item follows when that changes) and the read-only endpoint.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Session {
    pub connected: bool,
    pub dark: bool,
    pub chain_id: String,
    pub signer: String,
    pub account: Option<u64>,
    pub endpoint: String,
}
/// `host.widget`: a command on the mounted tree. The one method on the TREE
/// side of the codec rule — a [`WidgetCommand`] names typed element ids the
/// tree is drawn with — so it is the one method in named MessagePack.
pub struct HostWidget;
impl sealed::Sealed for HostWidget {}
impl Method for HostWidget {
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

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Clipboard {
    pub text: String,
}
/// One notice for the host to decide on: `notify.post`. The view asks; the
/// host logs it in its notification centre and decides whether a banner
/// reaches the screen (the person's per-view choice, focus, a burst limit).
/// `link` is a `duck://` link the centre opens when the notice is picked,
/// or empty.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub tag: String,
    pub link: String,
}
/// What the host did with a [`Notification`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum Delivery {
    /// Logged, and a banner was raised.
    Banner,
    /// Logged in the centre only: no banner (not yet allowed, silenced,
    /// in front, over the burst limit, or banners are off).
    Logged,
    /// The person blocked this view's notices: dropped, not logged.
    Blocked,
}
methods! {
    also: ["module.query", "op.submit", "module.changes", HostWidget::KIND];
    /// `chain.status`: the connected node's status.
    ChainStatus, "chain.status", (), NodeStatus;
    /// `invite.create`: mint one invite, once (never retried).
    InviteCreate, "invite.create", CreateInvite, Invite;
    /// `chain.blocks`: a page of finalized blocks, newest first.
    ChainBlocks, "chain.blocks", BlockPage, Vec<Block>;
    /// `chain.block`: one finalized block; `None` where the node has none by
    /// that name.
    ChainBlock, "chain.block", BlockRef, Option<Block>;
    /// `blob.get`: a blob by `sha256:<hex>` or `sha1:<hex>` id, unframed;
    /// `None` where the node holds no blob by that id.
    BlobGet, "blob.get", String, Option<Vec<u8>>;
    /// `host.session`: a subscription to [`Session`], an item per change.
    HostSession, "host.session", (), Session;
    /// `host.visible`: whether the view is on screen, an item per change.
    HostVisible, "host.visible", (), bool;
    /// `host.badge`: the count on the view's tab.
    HostBadge, "host.badge", i64, ();
    /// `link.open`: the one way out: a `duck://` link, opened in the
    /// app, or an `https://` one, handed to the system browser. Any other
    /// scheme is refused (`malformed_request`).
    LinkOpen, "link.open", String, ();
    /// `host.route`: a subscription, one item per `duck://` link opened into
    /// this view: the path after the view's own segment (`tx/<hash>` of
    /// `duck://<chain>/explorer/tx/<hash>`). The route is delivered DECODED:
    /// the link spells each segment percent-encoded (`ducklink`), the host
    /// decodes it and joins the segments with `/`, so `forge%3Aweb%3A3/42`
    /// arrives as `forge:web:3/42`. A segment is nonempty, not `.` or `..`,
    /// and holds no `/` and no control character; the whole route is at most
    /// 256 bytes. A link that mounted the view is its first item.
    HostRoute, "host.route", (), String;
    /// `host.id`: a fresh id under the named prefix.
    HostId, "host.id", String, String;
    /// `clock.ticks`: an item per period, in milliseconds.
    ClockTicks, "clock.ticks", i64, ();
    /// `host.log`: one line to the host's log.
    HostLog, "host.log", String, ();
    /// `clipboard.read`: the clipboard's text and any files on it.
    ClipboardRead, "clipboard.read", (), Clipboard;
    /// `clipboard.write`: text onto the clipboard.
    ClipboardWrite, "clipboard.write", String, ();
    /// `notify.post`: hand the host a notice; it says what it did.
    NotifyPost, "notify.post", Notification, Delivery;
    /// `notify.seen`: the reader has seen what this view posted under a
    /// tag; the host marks this view's rows under it read and takes down
    /// its standing banner. Another view's rows are never touched.
    NotifySeen, "notify.seen", String, ();
    /// `store.get`: the value this view keeps under a key on this device,
    /// for the network in hand; `None` where it keeps none. A view sees
    /// only its own keys, and only on the network it runs on.
    StoreGet, "store.get", String, Option<Vec<u8>>;
    /// `store.set`: keep a value under a key, or drop it with `None`.
    StoreSet, "store.set", (String, Option<Vec<u8>>), ();
    /// `chain.heads`: a subscription, one item per finalized block, oldest
    /// first, from the tip at subscribe on. Heights may skip where the host
    /// could not fill a gap (a reconnect, a node with no archive); a view
    /// that must see every block reads the gap with `chain.blocks`.
    ChainHeads, "chain.heads", (), Head;
    /// `module.describe`: an op as its program says a person reads it,
    /// from the `ducktape.describe` module in the program's current code;
    /// `None` where the code carries none or it cannot read these bytes.
    ModuleDescribe, "module.describe", (String, Vec<u8>), Option<Description>;
}

/// The `<capability>` half of every kind in [`ALL`]: the names a view's
/// manifest may declare. `export_view!` refuses any other at compile time.
pub const CAPABILITIES: &[&str] = &[
    "chain",
    "module",
    "op",
    "invite",
    "link",
    "blob",
    "host",
    "clock",
    "clipboard",
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
        assert!(is_capability("chain") && is_capability("link") && is_capability("notify"));
        assert!(!is_capability("rpc"));
        assert!(!is_capability("chat") && !is_capability("module.query") && !is_capability(""));
    }

    struct Binary;
    impl Module for Binary {
        const NAME: &'static str = "binary";
        type Op = ();
        type Query = (u64, String);
        type Reply = Vec<u32>;
    }

    #[test]
    fn program_methods_address_their_program_and_carry_its_bytes() {
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
