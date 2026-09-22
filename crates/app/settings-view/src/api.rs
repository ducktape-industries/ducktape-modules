//! Node system requests and generic host session facts.
use borsh::{BorshDeserialize, BorshSerialize};
use ducktape_view_guest::{
    capability,
    caps::Program,
    host::{Refusal, malformed},
    view::{Capability, json_decode, json_encode},
};
use modules::{identity, valset};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub account: String,
    pub dark: bool,
    pub endpoint: String,
}
capability!(Props, "host.props", (), Session);

#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Status {
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
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Invite {
    pub invite: String,
    pub notes: Vec<Note>,
}
#[derive(Clone, Debug, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Note {
    pub reason: String,
    pub sentence: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mint {
    pub ttl_days: u64,
}
macro_rules! system_request {
    ($name:ident, $kind:literal, $request:ty, $reply:ty) => {
        pub struct $name;
        impl Capability for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
            fn encode(request: &Self::Request) -> Vec<u8> {
                json_encode(request)
            }
            fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
                json_decode(bytes)
            }
            fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
                borsh::to_vec(reply).expect("system reply encodes")
            }
            fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
                borsh::from_slice(bytes).map_err(|e| malformed(e.to_string()))
            }
        }
    };
}
system_request!(NodeStatus, "rpc.status", (), Status);
system_request!(MintInvite, "rpc.invite", Mint, Invite);
pub struct Identity;
impl Program for Identity {
    const PROGRAM: &'static str = identity::PROGRAM;
    type Request = identity::Query;
    type Reply = identity::Reply;
}
pub struct Valset;
impl Program for Valset {
    const PROGRAM: &'static str = valset::PROGRAM;
    type Request = valset::Query;
    type Reply = valset::Reply;
}
