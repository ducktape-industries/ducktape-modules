// The git wire protocol pieces the server speaks: pkt-line framing, sideband, receive-pack v1, upload-pack v2.

pub mod pktline;
pub mod receive;
pub mod sideband;
pub mod upload;

use alloc::vec::Vec;

pub fn smart_http_service_header(service: &[u8]) -> Vec<u8> {
    let mut line = b"# service=".to_vec();
    line.extend_from_slice(service);
    line.push(b'\n');
    let mut out = pktline::encode(&line);
    out.extend_from_slice(pktline::flush());
    out
}
