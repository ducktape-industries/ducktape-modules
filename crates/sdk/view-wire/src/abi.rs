//! The calling convention between the host and a view: a plain core wasm
//! module, bytes through the guest's own memory, the shape `crates/sdk/guest`
//! gives a program.
//!
//! ```text
//! import ducktape_view.panicked(ptr: u32, len: u32)
//! export alloc(len: u32) -> u32
//! export init(macos: u32)
//! export tick(ptr: u32, len: u32) -> u64
//! export snapshot() -> u64
//! export restore(ptr: u32, len: u32, macos: u32) -> u64
//! ```
//!
//! The host writes an argument into a buffer `alloc` handed it, and the guest
//! owns that buffer from the call on. A `u64` answer is [`pack`]ed: it names
//! bytes the guest keeps until the next export is entered, so the host copies
//! them out before it calls again. `tick` answers an encoded `Frame`;
//! `snapshot` and `restore` answer a [`encode_result`].

/// The module the guest's one import lives in.
pub const IMPORT_MODULE: &str = "ducktape_view";

pub fn pack(ptr: u32, len: u32) -> u64 {
    (u64::from(ptr) << 32) | u64::from(len)
}

pub fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// `0` and the bytes, or `1` and the refusal sentence.
pub fn encode_result(result: Result<Vec<u8>, String>) -> Vec<u8> {
    let (tag, body) = match result {
        Ok(bytes) => (0, bytes),
        Err(sentence) => (1, sentence.into_bytes()),
    };
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(tag);
    out.extend_from_slice(&body);
    out
}

/// `None` is an answer no guest built on this crate writes.
pub fn decode_result(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let (tag, body) = bytes.split_first()?;
    match tag {
        0 => Some(Ok(body.to_vec())),
        1 => Some(Err(String::from_utf8_lossy(body).into_owned())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_round_trip() {
        assert_eq!(unpack(pack(0xdead_beef, 7)), (0xdead_beef, 7));
        for result in [
            Ok(vec![1, 0, 2]),
            Ok(Vec::new()),
            Err("not yet".to_string()),
        ] {
            assert_eq!(decode_result(&encode_result(result.clone())), Some(result));
        }
        assert_eq!(decode_result(&[]), None);
        assert_eq!(decode_result(&[2, 0]), None);
    }
}
