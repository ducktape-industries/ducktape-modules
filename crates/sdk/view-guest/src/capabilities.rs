use crate::host::{self, Refusal};
use serde::{Serialize, de::DeserializeOwned};

/// One `kind` a view may ask the host for, with the request and reply it
/// carries. `KIND` is `<capability>.<operation>`; the host refuses one the
/// manifest did not declare.
///
/// A capability names its own codec, because not every door is JSON: a
/// binary one ([`caps::QueryBytes`](crate::caps::QueryBytes)) carries borsh
/// the serde traits never see. [`capability!`] writes the JSON pair for the
/// ones that are.
pub trait Capability {
    const KIND: &'static str;
    const TARGET: Option<&'static str> = None;
    type Request: std::fmt::Debug;
    type Reply;
    fn encode(request: &Self::Request) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal>;
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal>;
    fn encode_reply(reply: &Self::Reply) -> Vec<u8>;
}

/// The JSON half of a capability, named once so every JSON door spells it
/// the same way.
pub fn json_encode<T: Serialize>(request: &T) -> Vec<u8> {
    serde_json::to_vec(request).expect("request encodes")
}

pub fn json_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Refusal> {
    serde_json::from_slice(bytes).map_err(|error| host::malformed(error.to_string()))
}

/// A module a view talks to: its name on the node and the types it speaks.
/// Implemented next to the view (a marker type), never by the module crate,
/// which must not link this runtime.
pub trait Module {
    const NAME: &'static str;
    /// `op.submit` payload.
    type Op: Serialize + DeserializeOwned + std::fmt::Debug;
    /// `rpc.query` request / reply — the module's own query surface.
    type Query: Serialize + DeserializeOwned + std::fmt::Debug;
    type Reply: Serialize + DeserializeOwned;
    /// `rpc.view` request / reply — the module's index-tier view.
    type ViewQuery: Serialize + DeserializeOwned + std::fmt::Debug;
    type ViewReply: Serialize + DeserializeOwned;
}

/// A [`Capability`] whose request and reply are both JSON.
///
/// `capability!(Pick, "fs.pick", Value, Vec<SelectedFile>);`
#[macro_export]
macro_rules! capability {
    ($name:ident, $kind:literal, $request:ty, $reply:ty) => {
        pub struct $name;
        impl $crate::view::Capability for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
            fn encode(request: &$request) -> Vec<u8> {
                $crate::view::json_encode(request)
            }
            fn decode_request(bytes: &[u8]) -> Result<$request, $crate::host::Refusal> {
                $crate::view::json_decode(bytes)
            }
            fn encode_reply(reply: &$reply) -> Vec<u8> {
                $crate::view::json_encode(reply)
            }
            fn decode(bytes: &[u8]) -> Result<$reply, $crate::host::Refusal> {
                $crate::view::json_decode(bytes)
            }
        }
    };
}

/// `rpc.view` against `M`.
pub struct ViewOf<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for ViewOf<M> {
    const KIND: &'static str = "rpc.view";
    const TARGET: Option<&'static str> = Some(M::NAME);
    type Request = M::ViewQuery;
    type Reply = M::ViewReply;
    fn encode(request: &Self::Request) -> Vec<u8> {
        json_encode(&serde_json::json!({ "target": M::NAME, "query": request }))
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        json_decode(bytes)
    }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        decode_envelope(bytes, M::NAME, "query")
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        json_encode(reply)
    }
}

/// `rpc.query` against `M`.
pub struct Query<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for Query<M> {
    const KIND: &'static str = "rpc.query";
    const TARGET: Option<&'static str> = Some(M::NAME);
    type Request = M::Query;
    type Reply = M::Reply;
    fn encode(request: &Self::Request) -> Vec<u8> {
        json_encode(&serde_json::json!({ "target": M::NAME, "query": request }))
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        json_decode(bytes)
    }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        decode_envelope(bytes, M::NAME, "query")
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        json_encode(reply)
    }
}

/// `op.submit` to `M`; the reply is whatever the node echoes.
pub struct Submit<M>(std::marker::PhantomData<M>);
impl<M: Module> Capability for Submit<M> {
    const KIND: &'static str = "op.submit";
    const TARGET: Option<&'static str> = Some(M::NAME);
    type Request = M::Op;
    type Reply = serde_json::Value;
    fn encode(request: &Self::Request) -> Vec<u8> {
        json_encode(&serde_json::json!({ "target": M::NAME, "payload": request }))
    }
    fn decode(bytes: &[u8]) -> Result<Self::Reply, Refusal> {
        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        json_decode(bytes)
    }
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, Refusal> {
        decode_envelope(bytes, M::NAME, "payload")
    }
    fn encode_reply(reply: &Self::Reply) -> Vec<u8> {
        json_encode(reply)
    }
}

/// `rpc.live`: one item per state change of the named module.
pub struct Live;
impl Capability for Live {
    fn decode_request(bytes: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(bytes.to_vec()).map_err(|error| host::malformed(error.to_string()))
    }
    fn encode_reply(_: &()) -> Vec<u8> {
        Vec::new()
    }
    const KIND: &'static str = "rpc.live";
    type Request = String;
    type Reply = ();
    fn encode(module: &String) -> Vec<u8> {
        module.as_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
}

/// `host.visible`: whether the view is on screen.
pub struct Visible;
impl Capability for Visible {
    fn decode_request(_: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }
    fn encode_reply(reply: &bool) -> Vec<u8> {
        json_encode(reply)
    }
    const KIND: &'static str = "host.visible";
    type Request = ();
    type Reply = bool;
    fn encode(_: &()) -> Vec<u8> {
        Vec::new()
    }
    fn decode(bytes: &[u8]) -> Result<bool, Refusal> {
        Ok(bytes == b"true" || bytes == b"1" || bytes == b"visible")
    }
}

pub(crate) fn decode_envelope<T: DeserializeOwned>(
    bytes: &[u8],
    target: &str,
    field: &str,
) -> Result<T, Refusal> {
    let mut value: serde_json::Value = json_decode(bytes)?;
    if value["target"].as_str() != Some(target) {
        return Err(host::malformed(format!("expected target {target}")));
    }
    serde_json::from_value(value[field].take()).map_err(|error| host::malformed(error.to_string()))
}
