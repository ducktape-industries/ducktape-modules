//! Every door this view speaks through: the forge program (borsh, raw), the
//! chat module (JSON, the same door chat-view uses) and the host's session.
use base64::Engine as _;
use ducktape_view_guest::caps::Program;
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Capability, Module, json_encode};
use ducktape_view_guest::{capability, caps::QueryBytes};
use serde::{Deserialize, Serialize};

use forge::{Op, Query, Reply};

/// `rpc.query_bytes` to forge: [`QueryBytes<ForgeProgram>`].
pub struct ForgeProgram;
impl Program for ForgeProgram {
    const PROGRAM: &'static str = "forge";
    type Request = Query;
    type Reply = Reply;
}
pub type Ask = QueryBytes<ForgeProgram>;

/// Chat, read and written exactly as chat-view does it.
pub struct ChatApi;
impl Module for ChatApi {
    const NAME: &'static str = "chat";
    type Op = chat::ChatMsg;
    type Query = serde_json::Value;
    type Reply = serde_json::Value;
    type ViewQuery = chat::ChatViewQuery;
    type ViewReply = chat::ChatViewReply;
}

/// `op.submit_bytes` to forge. The generic door takes the JSON envelope and
/// carries the borsh operation inside it; the receipt is the program's own
/// output, which this view never reads — an accepted operation lands in the
/// next block and is reconciled by the next query.
pub struct SubmitForge;
impl Capability for SubmitForge {
    const KIND: &'static str = "op.submit_bytes";
    const TARGET: Option<&'static str> = Some(ForgeProgram::PROGRAM);
    type Request = Op;
    type Reply = ();

    fn encode(op: &Op) -> Vec<u8> {
        let body = borsh::to_vec(op).expect("borsh encodes an in-memory value");
        json_encode(&serde_json::json!({
            "target": ForgeProgram::PROGRAM,
            "body_b64": base64::engine::general_purpose::STANDARD.encode(body),
        }))
    }
    fn decode_request(bytes: &[u8]) -> Result<Op, Refusal> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|error| malformed(error.to_string()))?;
        if value["target"].as_str() != Some(ForgeProgram::PROGRAM) {
            return Err(malformed("expected target forge".into()));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value["body_b64"].as_str().unwrap_or_default())
            .map_err(|error| malformed(error.to_string()))?;
        borsh::from_slice(&bytes).map_err(|error| malformed(error.to_string()))
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
    fn encode_reply(_: &()) -> Vec<u8> {
        Vec::new()
    }
}

/// The host's session facts. `host.props` names an account, never a key, so
/// the reader's signing key — what forge filters and judgment are keyed by —
/// is joined from the identity roster chat answers with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub account: String,
    pub connected: bool,
    pub busy: bool,
    pub dark: bool,
    pub chain: String,
    pub endpoint: String,
    pub network_name: String,
}

impl Session {
    /// The reader's account number, when the handle carries one.
    pub fn number(&self) -> Option<u64> {
        self.account.strip_prefix("acct:")?.parse().ok()
    }
}

capability!(Props, "host.props", (), Session);
