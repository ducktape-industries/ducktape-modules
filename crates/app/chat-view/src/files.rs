//! Attachments: the file addresses messages carry, the upload that puts a
//! picked file into the files module, the picture decode and the text
//! preview the card over the screen reads. Everything here is a future the
//! view spawns; nothing touches state.
use std::collections::BTreeMap;

use base64::Engine as _;
use ducklink::{ChainId, Link, Refused};
use ducktape_view_guest::host::{Refusal, malformed};
use ducktape_view_guest::view::{Query, ask};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::api::{Files, FsRead, Id, PictureLoad, Release, SelectedFile, SubmitBytes};

/// The host picture slot this view draws into.
pub const PICTURE_SURFACE: &str = "chat";
/// The box a picture attachment is shown in: it fits inside, keeping its
/// shape, and never grows past its own size.
pub const PICTURE_BOX: (f32, f32) = (360., 280.);
/// The margin the preview card keeps from the screen's edges.
const PREVIEW_INSET: (f32, f32) = (160., 200.);
const PREVIEW_BYTES: usize = 65_536;
const PREVIEW_DISPLAY_BYTES: usize = 16 << 10;
pub const BINARY_PLATE: &str = "This file is not text, so there is nothing to show here.";

const CHUNK_SIZE: u64 = 1024 * 1024;
const MAX_INLINE_COMMIT_BYTES: u64 = 256 * 1024;
const MAX_UPLOAD: u64 = 64 << 20;
const MAX_NAME_BYTES: usize = 255;
const MAX_PATH_BYTES: usize = 4096;
const MAX_DEPTH: usize = 128;

/// The absolute duckfs path a `duck://<chain>/files/…` link names.
pub fn address_path(link: &str) -> Result<String, Refused> {
    let address = Link::parse(link)?;
    if address.program != "files" || address.tail.is_empty() {
        return Err(Refused::new(
            "invalid_input",
            "A file address must name at least one path segment.",
        ));
    }
    let path = format!("/{}", address.tail.join("/"));
    canonical_path(&path).map_err(|why| {
        Refused::new(
            "invalid_input",
            format!("A file address names a duckfs path, and `{path}` is not one: {why}."),
        )
    })?;
    Ok(path)
}

/// The path a link names, or "" when it names no file.
pub fn attachment_file_path(link: &str) -> String {
    address_path(link).unwrap_or_default()
}

/// The duckfs file address for a canonical path on `chain`.
pub fn file_address(chain: &str, path: &str) -> Result<String, Refused> {
    let chain: ChainId = chain.parse()?;
    let path = format!("/{}", path.strip_prefix('/').unwrap_or(path));
    let segments = canonical_path(&path).map_err(|why| {
        Refused::new(
            "invalid_input",
            format!("A file address names a duckfs path, and `{path}` is not one: {why}."),
        )
    })?;
    Link::new(chain, "files", segments).map(|link| link.to_string())
}

fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A file the preview card renders as Markdown rather than code.
pub fn markdown_path(name: &str) -> bool {
    matches!(extension(name).as_str(), "md" | "markdown")
}

/// A file the host can decode for the timeline.
pub fn is_picture(name: &str) -> bool {
    matches!(
        extension(name).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
    )
}

/// What a file is, from its extension: the caption under its name.
pub fn attachment_kind(name: &str) -> String {
    let ext = extension(name).to_ascii_uppercase();
    match ext.len() {
        1..=5 => format!("{ext} file"),
        _ => "File".into(),
    }
}

/// A picture inside a box, keeping its shape and never larger than itself.
fn fit(width: i64, height: i64, room: (f32, f32)) -> (f32, f32) {
    let (width, height) = (width.max(1) as f32, height.max(1) as f32);
    let scale = (room.0 / width).min(room.1 / height).min(1.);
    ((width * scale).round(), (height * scale).round())
}

pub fn picture_box(width: i64, height: i64) -> (f32, f32) {
    fit(width, height, PICTURE_BOX)
}

/// The most a preview's body may take of the screen.
pub fn preview_room(screen: (f32, f32)) -> (f32, f32) {
    (
        (screen.0 - PREVIEW_INSET.0).max(PICTURE_BOX.0),
        (screen.1 - PREVIEW_INSET.1).max(PICTURE_BOX.1),
    )
}

pub fn preview_box(width: i64, height: i64, screen: (f32, f32)) -> (f32, f32) {
    fit(width, height, preview_room(screen))
}

/// Ask the host to decode a duckfs picture into this view's slot; the
/// answer is the size it will be drawn at, (0, 0) for one that did not.
pub async fn picture_load(path: String) -> (i64, i64) {
    let Ok(drawn) = ask::<PictureLoad>(json!({"surface": PICTURE_SURFACE, "path": path})).await
    else {
        return (0, 0);
    };
    (
        drawn["width"].as_i64().unwrap_or(0),
        drawn["height"].as_i64().unwrap_or(0),
    )
}

/// One reading of a non-picture attachment: the head of the file, or why not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub clipped: bool,
    pub binary: bool,
}

async fn files_get(lane: &str, params: Value) -> Result<Value, Refusal> {
    let reply = ask::<Query<Files>>(json!({lane: params})).await?;
    reply
        .get(lane)
        .cloned()
        .ok_or_else(|| malformed("unexpected Files reply".into()))
}

/// The head of the file at the head snapshot, branded binary when it does
/// not read as text.
pub async fn read_preview(path: String) -> Result<Preview, Refusal> {
    let refs = files_get("refs", json!({})).await?;
    let base = refs["head"]
        .as_str()
        .ok_or_else(|| Refusal::new("no_snapshot", "The file has no committed snapshot"))?
        .to_owned();
    let reply = files_get(
        "read",
        json!({ "path": path, "len": PREVIEW_BYTES, "snapshot": base }),
    )
    .await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(reply["b64"].as_str().unwrap_or_default())
        .map_err(|_| malformed("The node's read page is not valid base64".into()))?;
    let eof = reply["eof"].as_bool().unwrap_or(true);
    let (text, binary) = readable(bytes, eof);
    let (text, clipped) = head_within(&text, PREVIEW_DISPLAY_BYTES);
    Ok(Preview {
        text,
        clipped: clipped || !eof,
        binary,
    })
}

/// A file that is text, or the plate that says it is not. A page that ended
/// before the file did may have cut a multi-byte character in half.
pub fn readable(mut bytes: Vec<u8>, eof: bool) -> (String, bool) {
    if let Err(error) = std::str::from_utf8(&bytes)
        && !eof
        && error.error_len().is_none()
    {
        bytes.truncate(error.valid_up_to());
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return (BINARY_PLATE.into(), true);
    };
    let control = text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'));
    match control {
        true => (BINARY_PLATE.into(), true),
        false => (text, false),
    }
}

fn head_within(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_owned(), false);
    }
    (text[..text.floor_char_boundary(limit)].to_owned(), true)
}

/// A name as the attachments directory files it.
pub fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '/') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Uploads `file` and answers the address every member opens it by, on
/// `chain`. The address is built FIRST: a name or a chain that has no
/// address is refused before a byte is stored.
pub async fn upload(file: SelectedFile, chain: String) -> Result<String, Refusal> {
    let attachment_id = ask::<Id>("attachment").await?;
    let path = format!(
        "/shared/attachments/{attachment_id}/{}",
        safe_name(&file.name)
    );
    let address = file_address(&chain, &path)
        .map_err(|refused| Refusal::new(refused.reason, refused.sentence))?;
    upload_inner(&file, path).await?;
    let _ = ask::<Release>(file.token.clone()).await;
    Ok(address)
}

fn canonical_path(path: &str) -> Result<Vec<String>, String> {
    if !path.starts_with('/') {
        return Err("path must be absolute (start with '/')".to_owned());
    }
    if path.chars().nfc().collect::<String>() != path {
        return Err("path is not NFC-normalized".to_owned());
    }
    if path.contains('\0') {
        return Err("path must not contain a NUL byte".to_owned());
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(format!(
            "path exceeds the {MAX_PATH_BYTES}-byte length limit"
        ));
    }
    if path == "/" {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for segment in path[1..].split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err("path contains an empty or dot segment".to_owned());
        }
        if segment.len() > MAX_NAME_BYTES {
            return Err(format!(
                "segment name exceeds the {MAX_NAME_BYTES}-byte limit"
            ));
        }
        segments.push(segment.to_owned());
    }
    if segments.len() > MAX_DEPTH {
        return Err(format!("path exceeds the maximum depth of {MAX_DEPTH}"));
    }
    Ok(segments)
}

async fn upload_inner(file: &SelectedFile, path: String) -> Result<(), Refusal> {
    if file.bytes > MAX_UPLOAD {
        return Err(Refusal::new("too_large", "Files must be at most 64 MiB"));
    }
    canonical_path(&path).map_err(|said| Refusal::new("invalid_path", said))?;
    let refs = files_get("refs", json!({})).await?;
    let mut chunks = Vec::new();
    let mut chunk = Vec::new();
    let mut offset = 0u64;
    let inline = file.bytes <= MAX_INLINE_COMMIT_BYTES;
    while offset < file.bytes {
        let len = (file.bytes - offset).min(256 << 10) as usize;
        let bytes = ask::<FsRead>(FsRead::request(&file.token, offset, len)).await?;
        if bytes.is_empty() || bytes.len() > len {
            return Err(Refusal::new(
                "file_changed",
                "The selected file changed during its upload",
            ));
        }
        offset += bytes.len() as u64;
        chunk.extend_from_slice(&bytes);
        let chunk_ready = !inline && (chunk.len() as u64 == CHUNK_SIZE || offset == file.bytes);
        if chunk_ready {
            submit_bytes(encode_putblob(&chunk)).await?;
            chunks.push(crate::chat::hex(&object_id(0, &chunk)));
            chunk.clear();
        }
    }
    let content = if inline {
        Content::Inline {
            b64: base64::engine::general_purpose::STANDARD.encode(chunk),
        }
    } else {
        Content::Chunks {
            size: file.bytes,
            chunks,
        }
    };
    let commit = FilesMsg::Commit {
        base_snapshot: refs["head"].as_str().map(str::to_owned),
        message: format!("upload {}", file.name),
        changes: vec![Change::Put {
            path,
            exec: false,
            meta: BTreeMap::new(),
            content,
        }],
    };
    submit_bytes(serde_json::to_vec(&commit).expect("files message")).await
}

async fn submit_bytes(bytes: Vec<u8>) -> Result<(), Refusal> {
    ask::<SubmitBytes>(json!({
        "target": "files",
        "body_b64": base64::engine::general_purpose::STANDARD.encode(bytes)
    }))
    .await
    .map(|_| ())
}

fn encode_putblob(bytes: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(bytes.len() + 1);
    encoded.push(0);
    encoded.extend_from_slice(bytes);
    encoded
}

fn object_id(kind: u8, body: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update([kind]);
    digest.update(body);
    digest.finalize().into()
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum FilesMsg {
    Commit {
        base_snapshot: Option<String>,
        message: String,
        changes: Vec<Change>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Change {
    Put {
        path: String,
        exec: bool,
        meta: BTreeMap<String, String>,
        content: Content,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Content {
    Inline { b64: String },
    Chunks { size: u64, chunks: Vec<String> },
}

// ---------- duck links ----------

/// `duck://<chain>/<program>/<tail…>`, or "" without a chain.
fn minted(chain: &str, program: &str, tail: Vec<String>) -> String {
    chain
        .parse()
        .ok()
        .and_then(|chain| Link::new(chain, program, tail).ok())
        .map(|link| link.to_string())
        .unwrap_or_default()
}

/// `duck://<chain>/chat/<channel>[/<seq>]`: chat's own tail, as the module
/// reads it.
pub fn channel_link(chain: &str, channel: &str, seq: Option<u64>) -> String {
    let mut tail = vec![channel.to_owned()];
    tail.extend(seq.map(|seq| seq.to_string()));
    minted(chain, "chat", tail)
}

/// A pressed mention (an account number) becomes `duck://<chain>/identity/<n>`,
/// the link the app opens; any other link is already one and passes through.
pub fn pressed_link(link: String, chain: &str) -> String {
    match link.parse::<u64>() {
        Ok(account) => minted(chain, "identity", vec![account.to_string()]),
        Err(_) => link,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_addresses_keep_the_producer_spelling() {
        let address = file_address("testnet#0a1b2c3d", "/shared/보고서 Final.pdf").unwrap();
        assert_eq!(
            address,
            "duck://testnet-0a1b2c3d/files/shared/%EB%B3%B4%EA%B3%A0%EC%84%9C%20Final.pdf"
        );
        assert_eq!(address_path(&address).unwrap(), "/shared/보고서 Final.pdf");
        assert!(file_address("", "/shared/a.md").is_err());
        assert!(address_path("duck://testnet-0a1b2c3d/pages/a").is_err());
    }

    #[test]
    fn links_and_commits_keep_their_shapes() {
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "general", Some(42)),
            "duck://testnet-0a1b2c3d/chat/general/42"
        );
        assert_eq!(channel_link("", "general", None), "");
        assert_eq!(
            pressed_link("7".into(), "testnet#0a1b2c3d"),
            "duck://testnet-0a1b2c3d/identity/7"
        );
        assert_eq!(encode_putblob(b"abc"), [0, b'a', b'b', b'c']);
        let (text, binary) = readable(b"hi\n".to_vec(), true);
        assert_eq!((text.as_str(), binary), ("hi\n", false));
        assert!(readable(vec![0, 1, 2], true).1);
        assert_eq!(picture_box(720, 560), (360., 280.));
        assert_eq!(attachment_kind("deck.pdf"), "PDF file");
    }
}
