//! A transaction's payload as the operation it names, and the few formats
//! a chain reads in: short hashes, grouped numbers, dates and ages.
//!
//! A payload is decoded with the op type of the program it targets, for the
//! programs a view may link, and read as the title and fields that program's
//! `describe` gives it. A program the view does not link reads as its size
//! and a short hex preview.

/// The longest a field's value runs before it is clipped.
const MAX_VALUE: usize = 160;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Op {
    /// what the operation does, in words: `Post in #design`
    pub title: String,
    pub fields: Vec<(&'static str, String)>,
}

/// What a program's `describe` gives: a title and its fields.
type Described = (String, Vec<(&'static str, String)>);

fn described<T: borsh::BorshDeserialize>(
    payload: &[u8],
    describe: fn(&T) -> Described,
) -> Option<Described> {
    borsh::from_slice::<T>(payload).ok().map(|op| describe(&op))
}

pub fn decode(program: &str, payload: &[u8]) -> Op {
    let described = match program {
        "chat" => described(payload, chat::describe),
        "forge" => described(payload, forge::describe),
        identity::PROGRAM => described(payload, identity::describe),
        valset::PROGRAM => described(payload, valset::describe),
        module_registry::PROGRAM => described(payload, module_registry::describe),
        _ => None,
    };
    match described {
        Some((title, fields)) => Op {
            title: clip(&title),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name, clip(&value)))
                .collect(),
        },
        None => Op {
            title: format!(
                "{program} · {}",
                plural(payload.len() as u64, "byte", "bytes")
            ),
            fields: vec![("bytes", abi::preview(payload))],
        },
    }
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_VALUE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
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
