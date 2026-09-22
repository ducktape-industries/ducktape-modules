//! The host capabilities every view speaks the same way: ids, the clock,
//! the device (files, clipboard, pictures) and the node's raw surfaces.
//! What a view says to its own host lives beside the view.
use base64::Engine as _;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability;
use crate::host::{malformed, Refusal};
use crate::view::{json_encode, Capability};

/// `host.id`: a fresh id of the named kind.
pub struct Id;
impl Capability for Id {
    fn decode_request(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| malformed(error.to_string()))
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        reply.as_bytes().to_vec()
    }
    const KIND: &'static str = "host.id";
    type Request = String;
    type Reply = String;
    fn encode(kind: &String) -> Vec<u8> {
        kind.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| malformed(error.to_string()))
    }
}

/// `clock.ticks`: one item per period, in milliseconds.
pub struct Ticks;
impl Capability for Ticks {
    fn decode_request(bytes: &[u8]) -> Result<i64, Refusal> {
        bytes
            .try_into()
            .map(i64::from_le_bytes)
            .map_err(|_| malformed("expected eight-byte tick interval".into()))
    }
    fn encode_reply(_: &()) -> Vec<u8> {
        Vec::new()
    }
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

capability!(Pick, "fs.pick", Value, Vec<SelectedFile>);
capability!(Drops, "fs.drops", Value, Vec<SelectedFile>);
capability!(FsRead, "fs.read", Value, Vec<u8>);
capability!(ClipboardRead, "clipboard.read", Value, Clipboard);
capability!(SubmitBytes, "op.submit_bytes", Value, Value);
capability!(PictureLoad, "picture.load", Value, Value);
capability!(Admin, "rpc.admin", Value, Value);
// `rpc.stream`: frames of a node topic, each a JSON value.
capability!(Stream, "rpc.stream", Value, Value);

/// A borsh program a view reads: its name on the node and the two types its
/// query surface speaks. Implemented next to the view (a marker type), the
/// way [`Module`](crate::view::Module) is — the contract crate holds the
/// types and never links this runtime.
pub trait Program {
    const PROGRAM: &'static str;
    type Request: BorshSerialize + BorshDeserialize + std::fmt::Debug;
    type Reply: BorshSerialize + BorshDeserialize;
}

/// `rpc.query_bytes`: one borsh query to `P`. The request rides base64 in the
/// JSON envelope the host door takes; the answer is the program's own reply,
/// raw borsh, decoded here. A program that refuses answers with a refusal,
/// so the four states of a [`Loaded`](crate::view::Loaded) slot are honest.
pub struct QueryBytes<P>(core::marker::PhantomData<P>);

impl<P: Program> Capability for QueryBytes<P> {
    const TARGET: Option<&'static str> = Some(P::PROGRAM);
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        let body: String = crate::capabilities::decode_envelope(bytes, P::PROGRAM, "body_b64")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|error| malformed(error.to_string()))?;
        borsh::from_slice(&bytes).map_err(|error| malformed(error.to_string()))
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        borsh::to_vec(reply).expect("borsh encodes an in-memory value")
    }
    const KIND: &'static str = "rpc.query_bytes";
    type Request = P::Request;
    type Reply = P::Reply;

    fn encode(request: &P::Request) -> Vec<u8> {
        let body = borsh::to_vec(request).expect("borsh encodes an in-memory value");
        json_encode(&serde_json::json!({
            "target": P::PROGRAM,
            "body_b64": base64::engine::general_purpose::STANDARD.encode(body),
        }))
    }

    fn decode(bytes: &[u8]) -> Result<P::Reply, Refusal> {
        borsh::from_slice(bytes).map_err(|error| malformed(error.to_string()))
    }
}

impl FsRead {
    pub fn request(token: &str, offset: u64, len: usize) -> Value {
        serde_json::json!({"token": token, "offset": offset, "len": len})
    }
}

/// `fs.release`: the token goes out as text, nothing comes back.
pub struct Release;
impl Capability for Release {
    fn decode_request(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| malformed(error.to_string()))
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        let _ = reply;
        Vec::new()
    }
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

/// `clipboard.write`: the text goes out raw.
pub struct ClipboardWrite;
impl Capability for ClipboardWrite {
    fn decode_request(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| malformed(error.to_string()))
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        let _ = reply;
        Vec::new()
    }
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedFile {
    pub token: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Clipboard {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub files: Vec<SelectedFile>,
}

/// A mutation of the mounted widget tree, encoded by the existing wire codec.
pub struct Widget;
impl Capability for Widget {
    const KIND: &'static str = "host.widget";
    type Request = crate::wire::WidgetCommand;
    type Reply = ();
    fn encode(request: &Self::Request) -> Vec<u8> {
        crate::wire::encode(request)
    }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        crate::wire::decode(bytes).map_err(malformed)
    }
    fn decode(bytes: &[u8]) -> Result<(), Refusal> {
        crate::wire::decode(bytes).map_err(malformed)
    }
    fn encode_reply(reply: &()) -> Vec<u8> {
        crate::wire::encode(reply)
    }
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    struct Binary;
    impl Program for Binary {
        const PROGRAM: &'static str = "binary";
        type Request = (u64, String);
        type Reply = Vec<u32>;
    }

    #[test]
    fn fake_host_decodes_addressed_borsh_requests_and_encodes_replies() {
        let request = (42, "query".into());
        assert_eq!(
            QueryBytes::<Binary>::decode_request(&QueryBytes::<Binary>::encode(&request)).unwrap(),
            request
        );
        let reply = vec![1, 2, 3];
        assert_eq!(
            QueryBytes::<Binary>::decode(&QueryBytes::<Binary>::encode_reply(&reply)).unwrap(),
            reply
        );
        assert!(
            QueryBytes::<Binary>::decode_request(br#"{"target":"other","body_b64":""}"#).is_err()
        );
        assert!(
            QueryBytes::<Binary>::decode_request(br#"{"target":"binary","body_b64":"!"}"#).is_err()
        );
    }

    #[test]
    fn raw_host_codecs_round_trip() {
        let kind = "member".to_owned();
        assert_eq!(Id::decode_request(&Id::encode(&kind)).unwrap(), kind);
        assert_eq!(Id::decode(&Id::encode_reply(&kind)).unwrap(), kind);
        assert_eq!(Ticks::decode_request(&Ticks::encode(&250)).unwrap(), 250);
        assert!(Ticks::decode_request(&[1, 2]).is_err());
    }
}
