//! Program-owned codecs over the generic host doors.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ducktape_view_guest::{capability, caps::Program, host::{Refusal, malformed}, view::Capability};
use serde::{Deserialize, Serialize};
use crate::{Op, OpReply, Query, Reply};

pub struct ForgeProgram;
impl Program for ForgeProgram {
    const PROGRAM: &'static str = "forge";
    type Request = Query;
    type Reply = Reply;
}

pub struct Identity;
impl Program for Identity {
    const PROGRAM: &'static str = "identity";
    type Request = modules::identity::Query;
    type Reply = modules::identity::Reply;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub account: String,
    pub connected: bool,
    pub chain: String,
    pub endpoint: String,
    pub dark: bool,
}
capability!(Props, "host.props", (), Session);
capability!(Badge, "host.badge", serde_json::Value, ());

fn envelope(target: &str, bytes: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"target": target, "body_b64": STANDARD.encode(bytes)})).expect("envelope")
}
fn body(target: &str, bytes: &[u8]) -> Result<Vec<u8>, Refusal> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| malformed(e.to_string()))?;
    if value["target"].as_str() != Some(target) {
        return Err(malformed("unexpected program target".into()));
    }
    STANDARD.decode(value["body_b64"].as_str().unwrap_or_default()).map_err(|e| malformed(e.to_string()))
}

pub struct SubmitForge;
impl Capability for SubmitForge {
    const KIND: &'static str = "op.submit_bytes";
    const TARGET: Option<&'static str> = Some("forge");
    type Request = Op;
    type Reply = Option<OpReply>;
    fn encode(op: &Op) -> Vec<u8> { envelope("forge", &abi::encode(op)) }
    fn decode_request(bytes: &[u8]) -> Result<Op, Refusal> {
        abi::decode(&body("forge", bytes)?).map_err(|e| malformed(e.to_string()))
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        if bytes.is_empty() { Ok(None) } else { abi::decode(bytes).map(Some).map_err(|e| malformed(e.to_string())) }
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> { reply.as_ref().map(abi::encode).unwrap_or_default() }
}

/// Chat's program accepts JSON bytes, even through the raw query door.
pub struct ChatRead;
impl Capability for ChatRead {
    const KIND: &'static str = "rpc.query_bytes";
    const TARGET: Option<&'static str> = Some("chat");
    type Request = chat::ChatViewQuery;
    type Reply = chat::ChatViewReply;
    fn encode(query: &Self::Request) -> Vec<u8> { envelope("chat", &serde_json::to_vec(query).expect("chat query")) }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        serde_json::from_slice(&body("chat", bytes)?).map_err(|e| malformed(e.to_string()))
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> { serde_json::from_slice(bytes).map_err(|e| malformed(e.to_string())) }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> { serde_json::to_vec(reply).expect("chat reply") }
}

pub struct SubmitChat;
impl Capability for SubmitChat {
    const KIND: &'static str = "op.submit_bytes";
    const TARGET: Option<&'static str> = Some("chat");
    type Request = chat::ChatMsg;
    type Reply = ();
    fn encode(op: &Self::Request) -> Vec<u8> { envelope("chat", &serde_json::to_vec(op).expect("chat op")) }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        serde_json::from_slice(&body("chat", bytes)?).map_err(|e| malformed(e.to_string()))
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> { Ok(()) }
    fn encode_reply(_: &()) -> Vec<u8> { Vec::new() }
}
