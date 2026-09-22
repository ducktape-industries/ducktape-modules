//! What this view says to the host: the chat module's types, the identity
//! roster, the files and runs modules, the host's props stream, the device
//! (files, clipboard, pictures) and the intents the host acts on.
use crate::chat::{ChatMsg, ChatViewQuery, ChatViewReply};
use ducktape_view_guest::capability;
pub use ducktape_view_guest::caps::*;
use ducktape_view_guest::view::{Capability, Module};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The files module, as attachments still speak to it (JSON, untyped).
pub struct Files;
impl Module for Files {
    const NAME: &'static str = "files";
    type Op = Value;
    type Query = Value;
    type Reply = Value;
    type ViewQuery = Value;
    type ViewReply = Value;
}

pub struct ChatApi;
impl Module for ChatApi {
    const NAME: &'static str = "chat";
    type Op = ChatMsg;
    type Query = Value;
    type Reply = Value;
    type ViewQuery = ChatViewQuery;
    type ViewReply = ChatViewReply;
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
    fn decode_request(_: &[u8]) -> Result<(), ducktape_view_guest::host::Refusal> {
        Ok(())
    }
    fn encode_reply(reply: &PropsItem) -> Vec<u8> {
        serde_json::to_vec(reply).expect("props reply encodes")
    }
    fn decode(bytes: &[u8]) -> Result<PropsItem, ducktape_view_guest::host::Refusal> {
        ducktape_view_guest::view::json_decode(bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[expect(
    clippy::large_enum_variant,
    reason = "the external untagged props schema keeps its established shape"
)]
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

capability!(ShowHuddle, "chat.show_huddle", (), ());
capability!(LeaveHuddle, "chat.leave_huddle", (), ());
capability!(JoinHuddle, "chat.join_huddle", (), ());
capability!(JoinVoice, "chat.join_voice", Value, ());
capability!(Copy, "chat.copy", Value, ());

/// The `chat.copy` intent: text for the clipboard and the toast that says so.
pub fn copy(text: &str, label: &str) -> Value {
    serde_json::json!({"text": text, "label": label})
}
