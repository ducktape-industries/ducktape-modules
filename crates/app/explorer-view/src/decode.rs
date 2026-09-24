//! A transaction's payload as the operation it names, and the few formats
//! a chain reads in: short hashes, grouped numbers, dates and ages.
//!
//! A payload is decoded with the op type of the program it targets, for the
//! programs a view may link; the decoded value's `Debug` form is then read
//! back into a variant and a field table, so every variant of every linked
//! program has a readable table without a hand-written one each. A program
//! the view does not link reads as its size and a short hex preview.
use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};

/// The longest a field's value runs before it is clipped.
const MAX_VALUE: usize = 160;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    /// what the operation does, in words: `Post in #design`
    pub title: String,
    /// the type path it decoded as: `chat::PostMessage`
    pub kind: String,
    pub fields: Vec<(String, String)>,
}

/// The longest `Debug` text an op is read from. A payload can carry a whole
/// packfile as bytes, and formatting all of it would run a view past its
/// per-tick fuel; the text stops here and the field it cut reads as cut.
const MAX_DEBUG: usize = 4096;

/// A `fmt::Write` that takes [`MAX_DEBUG`] bytes and then fails, which
/// stops the `Debug` impl writing into it.
struct Bounded(String);

impl std::fmt::Write for Bounded {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let room = MAX_DEBUG.saturating_sub(self.0.len());
        if text.len() <= room {
            self.0.push_str(text);
            return Ok(());
        }
        let cut = (0..=room).rev().find(|at| text.is_char_boundary(*at));
        self.0.push_str(&text[..cut.unwrap_or(0)]);
        Err(std::fmt::Error)
    }
}

fn debug_of<T: BorshDeserialize + std::fmt::Debug>(payload: &[u8]) -> Option<String> {
    use std::fmt::Write as _;
    let op = borsh::from_slice::<T>(payload).ok()?;
    let mut text = Bounded(String::new());
    let _ = write!(text, "{op:?}");
    Some(text.0)
}

pub fn decode(program: &str, payload: &[u8]) -> Op {
    let debug = match program {
        "chat" => debug_of::<chat::ChatMsg>(payload),
        "forge" => debug_of::<forge::Op>(payload),
        identity::PROGRAM => debug_of::<identity::Op>(payload),
        valset::PROGRAM => debug_of::<valset::Op>(payload),
        module_registry::PROGRAM => debug_of::<module_registry::Op>(payload),
        _ => None,
    };
    let Some(debug) = debug else {
        return opaque(program, payload);
    };
    let (variant, mut fields) = split(&debug);
    // a message's blocks read as the text they flatten to
    if let Ok(chat::ChatMsg::PostMessage { blocks, .. } | chat::ChatMsg::EditMessage { blocks, .. }) =
        borsh::from_slice::<chat::ChatMsg>(payload)
        && program == "chat"
        && let Some(field) = fields.iter_mut().find(|(name, _)| name == "blocks")
    {
        *field = ("text".into(), clip(&chat::plain_text(&blocks)));
    }
    Op {
        title: title(program, &variant, &fields),
        kind: format!("{program}::{variant}"),
        fields,
    }
}

fn opaque(program: &str, payload: &[u8]) -> Op {
    Op {
        title: format!(
            "{program} · {}",
            plural(payload.len() as u64, "byte", "bytes")
        ),
        kind: program.into(),
        fields: vec![("bytes".into(), bytes(payload))],
    }
}

fn title(program: &str, variant: &str, fields: &[(String, String)]) -> String {
    let field = |name: &str| {
        fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if program == "chat"
        && variant == "PostMessage"
        && let Some(channel) = field("channel_id")
    {
        return format!("Post in #{channel}");
    }
    let words = humanize(variant);
    let context = ["channel_id", "repo", "name", "program"]
        .iter()
        .find_map(|name| field(name));
    match context {
        Some(context) => format!("{words} · {context}"),
        None => words,
    }
}

/// `PostMessage` → `Post message`.
fn humanize(variant: &str) -> String {
    let mut words = String::new();
    for (index, c) in variant.chars().enumerate() {
        if c.is_uppercase() && index > 0 {
            words.push(' ');
            words.extend(c.to_lowercase());
        } else {
            words.push(c);
        }
    }
    words
}

/// A `Debug` value read back as its variant and its fields. A tuple variant
/// around one struct reads as that struct's fields.
fn split(debug: &str) -> (String, Vec<(String, String)>) {
    let end = debug
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(debug.len());
    let variant = debug[..end].to_string();
    let rest = debug[end..].trim();
    if let Some(inner) = rest
        .strip_prefix('{')
        .map(|r| r.strip_suffix('}').unwrap_or(r))
    {
        let fields = parts(inner)
            .into_iter()
            .filter_map(|part| {
                let (name, value) = part.split_once(": ")?;
                Some((name.trim().to_string(), shown(value.trim())))
            })
            .collect();
        return (variant, fields);
    }
    if let Some(inner) = rest
        .strip_prefix('(')
        .map(|r| r.strip_suffix(')').unwrap_or(r))
    {
        let items = parts(inner);
        if let [only] = items.as_slice() {
            let (_, fields) = split(only);
            if !fields.is_empty() {
                return (variant, fields);
            }
            return (variant, vec![("value".into(), shown(only))]);
        }
        let fields = items
            .iter()
            .enumerate()
            .map(|(index, item)| (index.to_string(), shown(item)))
            .collect();
        return (variant, fields);
    }
    (variant, Vec::new())
}

/// `inner` split at its top-level commas: none inside brackets or strings.
fn parts(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut escaped, mut start) = (0i32, false, false, 0);
    for (index, c) in inner.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                parts.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// One `Debug` value as a person reads it.
fn shown(value: &str) -> String {
    if value == "None" {
        return "—".into();
    }
    if let Some(inner) = value
        .strip_prefix("Some(")
        .and_then(|v| v.strip_suffix(')'))
    {
        return shown(inner);
    }
    if let Some(inner) = value
        .strip_prefix('"')
        .map(|v| v.strip_suffix('"').unwrap_or(v))
    {
        let text = inner
            .replace("\\\"", "\"")
            .replace("\\n", "\n")
            .replace("\\\\", "\\");
        return clip(&text);
    }
    if let Some(open) = value.strip_prefix('[') {
        let (inner, whole) = match open.strip_suffix(']') {
            Some(inner) => (inner, true),
            None => (open, false),
        };
        let raw: Option<Vec<u8>> = parts(inner).iter().map(|item| item.parse().ok()).collect();
        match raw {
            Some(raw) if whole => return bytes(&raw),
            Some(raw) => {
                let preview = abi::hex(&raw[..raw.len().min(8)]);
                return format!("over {} bytes · {preview}…", grouped(raw.len() as u64));
            }
            None => {}
        }
    }
    clip(value)
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// Bytes in hex: all of them up to 32, else their count and a preview.
pub fn bytes(raw: &[u8]) -> String {
    match raw.len() {
        0 => "0 bytes".into(),
        1..=32 => abi::hex(raw),
        len => format!("{len} bytes · {}…", abi::hex(&raw[..8])),
    }
}

/// `9f3a…c21e`: the head and tail of a hash.
pub fn short(raw: &[u8]) -> String {
    let hex = abi::hex(raw);
    if hex.len() <= 12 {
        return hex;
    }
    format!("{}…{}", &hex[..4], &hex[hex.len() - 4..])
}

/// `6230` → `6,230`.
pub fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, c) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

/// How long before `now` a time in milliseconds was: `2s`, `3m`, `4h`, `5d`.
pub fn ago(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// A time in milliseconds as a UTC date: `24 Sep 2026, 05:12:07`.
pub fn date(millis: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let seconds = millis / 1000;
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{day} {} {year}, {:02}:{:02}:{:02}",
        MONTHS[(month - 1) as usize],
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

/// A signing scheme by its short name: `ed25519`, `p256`.
pub fn scheme(scheme: abi::Scheme) -> String {
    match scheme {
        abi::Scheme::Secp256r1 => "p256".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}
