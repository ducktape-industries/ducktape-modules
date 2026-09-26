//! A transaction's payload as the operation it names, and the few formats
//! a chain reads in: short hashes, grouped numbers, dates and ages.
//!
//! A payload is described by the program it targets, through the host
//! (`module.describe`: the describe module the program's own code
//! carries). A program with none, or bytes it cannot read, reads as its
//! size and a short hex preview: [`bytes`].
use ducktape_view_guest::methods::{Description, Field, Value};

/// The longest a field's value runs before it is clipped.
const MAX_VALUE: usize = 160;

/// An op no describe module read: its program, its size, its first bytes.
pub fn bytes(program: &str, payload: &[u8]) -> Description {
    Description {
        title: format!(
            "{program} · {}",
            plural(payload.len() as u64, "byte", "bytes")
        ),
        fields: vec![Field {
            label: "bytes".into(),
            value: Value::bytes(payload),
        }],
    }
}

/// [`Value::Bytes`] as `abi::preview` reads bytes: all of a short run, the
/// length and the ends of a long one.
pub fn preview(len: u64, preview: &[u8]) -> String {
    match len {
        0 => "0 bytes".into(),
        1..=32 => short_hex(&abi::hex(preview)),
        _ => format!("{len} bytes · {}", short_hex(&abi::hex(preview))),
    }
}

/// `value / 10^decimals`, the whole part grouped.
pub fn amount(value: u128, decimals: u8) -> String {
    let Some(scale) = 10u128.checked_pow(u32::from(decimals)) else {
        return value.to_string();
    };
    let whole = value / scale;
    let whole = match u64::try_from(whole) {
        Ok(whole) => grouped(whole),
        Err(_) => whole.to_string(),
    };
    match decimals {
        0 => whole,
        _ => format!(
            "{whole}.{:0width$}",
            value % scale,
            width = decimals as usize
        ),
    }
}

pub fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// A hash or key as [`short_hex`] shows it.
pub fn short(raw: &[u8]) -> String {
    short_hex(&abi::hex(raw))
}

pub use ducktape_view_guest::design::{date, grouped, plural, short_hex};

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

/// A signing scheme by its short name: `ed25519`, `p256`.
pub fn scheme(scheme: abi::Scheme) -> String {
    match scheme {
        abi::Scheme::Secp256r1 => "p256".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}
