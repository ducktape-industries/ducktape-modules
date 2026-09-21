//! What this view says to the host: the chat module's types, the identity
//! roster, the files and runs modules, the host's props stream, the device
//! (files, clipboard, pictures) and the intents the host acts on.
use crate::chat::{ChatMsg, ChatViewQuery, ChatViewReply};
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Capability, Module};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct ChatApi;
impl Module for ChatApi {
    const NAME: &'static str = "chat";
    type Op = ChatMsg;
    type Query = Value;
    type Reply = Value;
    type ViewQuery = ChatViewQuery;
    type ViewReply = ChatViewReply;
}

/// A module spoken as plain JSON: the name directory (identity), the file
/// store behind attachments (files) and the agent runs answering here (runs).
macro_rules! json_module {
    ($name:ident, $target:literal) => {
        pub struct $name;
        impl Module for $name {
            const NAME: &'static str = $target;
            type Op = Value;
            type Query = Value;
            type Reply = Value;
            type ViewQuery = Value;
            type ViewReply = Value;
        }
    };
}
json_module!(Identity, "identity");
json_module!(Files, "files");
json_module!(Runs, "runs");

/// `host.id`: a fresh id of the named kind.
pub struct Id;
impl Capability for Id {
    const KIND: &'static str = "host.id";
    type Request = &'static str;
    type Reply = String;
    fn encode(kind: &&'static str) -> Vec<u8> {
        kind.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| malformed(error.to_string()))
    }
}

/// The host's session facts, pushed on `chat.props`; an item may instead
/// carry a `background` request the view answers headless.
pub struct Props;
impl Capability for Props {
    const KIND: &'static str = "chat.props";
    type Request = ();
    type Reply = PropsItem;
    fn encode(_: &()) -> Vec<u8> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PropsItem {
    Background {
        background: crate::background::Request,
    },
    Session(Box<Session>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub dark: bool,
    pub connected: bool,
    pub endpoint: String,
    pub network_name: String,
    /// this view's own network, `<label>#<salt>`, what its links carry
    pub chain: String,
    pub network_chain_id: String,
    pub status: String,
    pub block_height: i64,
    /// the reader's rendered handle (`acct:7` / `user:<hex>`) and key hex
    pub me: String,
    pub me_key: String,
    pub names_serial: i64,
    /// steered by `duck://` links, notifications and the tray
    pub active_channel: String,
    pub dm_peer: String,
    pub dm_serial: i64,
    /// the seq a landing asks the room to open around; 0 is the live tail
    pub land_seq: i64,
    pub loading: bool,
    pub busy: bool,
    pub huddle_joined: bool,
    pub huddle_channel: String,
    pub huddle_channel_name: String,
    pub huddle_joined_at: i64,
    pub huddle_now: i64,
    pub call_muted: bool,
    pub call_speaking: bool,
    pub call_peers: Vec<CallPeer>,
    pub shift_held: bool,
    pub copy_chord_serial: i64,
}

impl Session {
    /// This view's own network: the `chain` prop, else the legacy id.
    pub fn chain(&self) -> &str {
        if self.chain.is_empty() {
            &self.network_chain_id
        } else {
            &self.chain
        }
    }
    /// Every write in chat is authored by an account: a key that holds none
    /// reads and nothing more.
    pub fn holds_account(&self) -> bool {
        self.me.starts_with("acct:")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CallPeer {
    pub peer: String,
    pub muted: bool,
    pub speaking: bool,
}

/// A capability whose request encodes as JSON and whose reply is JSON.
macro_rules! capability {
    ($name:ident, $kind:literal, $request:ty, $reply:ty) => {
        pub struct $name;
        impl Capability for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
        }
    };
}
capability!(ShowHuddle, "chat.show_huddle", (), ());
capability!(LeaveHuddle, "chat.leave_huddle", (), ());
capability!(JoinHuddle, "chat.join_huddle", (), ());
capability!(JoinVoice, "chat.join_voice", Value, ());
capability!(Copy, "chat.copy", Value, ());
capability!(Pick, "fs.pick", Value, Vec<SelectedFile>);
capability!(Drops, "fs.drops", Value, Vec<SelectedFile>);
capability!(FsRead, "fs.read", Value, Vec<u8>);
capability!(ClipboardRead, "clipboard.read", Value, Clipboard);
capability!(SubmitBytes, "op.submit_bytes", Value, Value);
capability!(PictureLoad, "picture.load", Value, Value);
capability!(Admin, "rpc.admin", Value, Value);
// `rpc.stream`: frames of a node topic, each a JSON value.
capability!(Stream, "rpc.stream", Value, Value);

impl FsRead {
    pub fn request(token: &str, offset: u64, len: usize) -> Value {
        serde_json::json!({"token": token, "offset": offset, "len": len})
    }
}

/// The raw-bytes capabilities: what goes out is text, what comes back is
/// nothing or bytes.
pub struct Release;
impl Capability for Release {
    const KIND: &'static str = "fs.release";
    type Request = String;
    type Reply = ();
    fn encode(token: &String) -> Vec<u8> {
        token.as_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
}

pub struct ClipboardWrite;
impl Capability for ClipboardWrite {
    const KIND: &'static str = "clipboard.write";
    type Request = String;
    type Reply = ();
    fn encode(text: &String) -> Vec<u8> {
        text.as_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
}

/// `clock.ticks`: one item per period, in milliseconds.
pub struct Ticks;
impl Capability for Ticks {
    const KIND: &'static str = "clock.ticks";
    type Request = i64;
    type Reply = ();
    fn encode(millis: &i64) -> Vec<u8> {
        millis.to_le_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedFile {
    pub token: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Clipboard {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub files: Vec<SelectedFile>,
}

/// The `chat.copy` intent: text for the clipboard and the toast that says so.
pub fn copy(text: &str, label: &str) -> Value {
    serde_json::json!({"text": text, "label": label})
}
