//! The host capabilities every view speaks the same way: ids, the clock,
//! the device (files, clipboard, pictures) and the node's raw surfaces.
//! What a view says to its own host lives beside the view.
use crate::capability;
use crate::host::{Refusal, malformed};
use crate::view::Capability;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// `fs.release`: the token goes out as text, nothing comes back.
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

/// `clipboard.write`: the text goes out raw.
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
