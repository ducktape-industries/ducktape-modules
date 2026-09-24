//! Naming and folding: the network's name directory, the reader's own keys,
//! index rows folded into the rows the frame draws, mention candidates and
//! the derived DM channel id. Everything is display logic over `chat`.
use std::collections::{BTreeMap, BTreeSet};

use crate::chat::{AccountRow, Block, Mark, MsgRow, Party, Span, dm_peers, hex, unhex};

/// The account bound to a user key: its number (the identity) and its name.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundAccount {
    pub number: u64,
    pub name: String,
}

/// The network's name directory: the account behind each user key (by key
/// hex) and every account's name. Names are display text, not identity:
/// "the same person" is the account number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameDirectory {
    accounts: BTreeMap<String, BoundAccount>,
    by_account: BTreeMap<u64, String>,
    programs: BTreeSet<u64>,
}

impl NameDirectory {
    pub const fn empty() -> Self {
        Self {
            accounts: BTreeMap::new(),
            by_account: BTreeMap::new(),
            programs: BTreeSet::new(),
        }
    }

    /// From the roster chat relays from identity.
    pub fn from_roster(accounts: impl IntoIterator<Item = AccountRow>) -> Self {
        let mut names = Self::empty();
        for account in accounts {
            names
                .by_account
                .insert(account.number, account.name.clone());
            if account.program {
                names.programs.insert(account.number);
            }
            for key in account.keys {
                let bound = BoundAccount {
                    number: account.number,
                    name: account.name.clone(),
                };
                names.accounts.insert(key, bound);
            }
        }
        names
    }

    pub fn account_of(&self, key_hex: &str) -> Option<u64> {
        self.accounts.get(key_hex).map(|account| account.number)
    }

    pub fn is_program(&self, account: u64) -> bool {
        self.programs.contains(&account)
    }

    /// A member's label: the bound name, else the shortened key. Takes a
    /// handle (`acct:n`, `user:hex`) or a bare key hex.
    pub fn member_label(&self, key_hex: &str) -> String {
        let handle = if key_hex.contains(':') {
            key_hex.to_string()
        } else {
            format!("user:{key_hex}")
        };
        self.of_handle(&handle).map_or_else(
            || ducktape_view_guest::design::short_hex(key_hex),
            str::to_string,
        )
    }

    fn of_handle(&self, handle: &str) -> Option<&str> {
        match handle.split_once(':') {
            Some(("acct", number)) => self
                .by_account
                .get(&number.parse::<u64>().ok()?)
                .map(String::as_str),
            Some(("user", key)) => self.accounts.get(key).map(|account| account.name.as_str()),
            _ => None,
        }
    }
}

/// One message as the frame draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    /// 0 for a pending row
    pub seq: u64,
    pub author: String,
    pub meta: String,
    /// the message as one run of plain text: the copy range's line
    pub body: String,
    /// the editable markdown of the same body, mentions as stable tokens
    pub edit_body: String,
    /// the message as chat keeps it; the frame styles it as it draws
    pub blocks: Vec<Block>,
    pub pending: bool,
    pub rev: u32,
    pub edited: bool,
    pub deleted: bool,
    pub reply_count: u64,
    pub thread: Option<u64>,
    /// Opens a run: the first message, one whose author differs from the
    /// one above, the first unread, or one after a long quiet (see
    /// [`mark_message_groups`]).
    pub show_author: bool,
    pub initial: String,
    pub agent: bool,
    pub height: u64,
    /// block time in milliseconds; 0 for a pending row
    pub time: u64,
    pub reactions: Vec<ChatReaction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSpan {
    pub text: String,
    pub style: SpanStyle,
}

/// The one style arm a run renders through: a link outranks every other
/// mark, a mention outranks emphasis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanStyle {
    Plain,
    Bold,
    Italic,
    BoldItalic,
    Link(String),
    /// the account the mention names, in decimal ("" for a bare key)
    Mention(String),
}

pub type ChatReaction = crate::chat::ReactionSummary;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMember {
    pub key: String,
    pub label: String,
}

pub fn chat_message(row: MsgRow, names: &NameDirectory) -> ChatMessage {
    let edited = row.rev > 0;
    let meta = match (row.seq, edited) {
        (0, _) => "sending…".to_string(),
        (seq, false) => format!("#{seq}"),
        (seq, true) => format!("#{seq} · edited"),
    };
    let (body, edit_body, blocks) = if row.deleted {
        (
            "Message deleted".to_string(),
            String::new(),
            vec![Block::paragraph("Message deleted")],
        )
    } else {
        (
            message_body(&row.blocks, names),
            draft_body(&row.blocks),
            row.blocks,
        )
    };
    ChatMessage {
        id: row.message_id,
        seq: row.seq,
        author: author_display(&row.author, names),
        meta,
        body,
        edit_body,
        blocks,
        pending: row.seq == 0,
        rev: row.rev,
        edited,
        deleted: row.deleted,
        reply_count: row.reply_count,
        thread: row.thread,
        show_author: true,
        initial: avatar_initial(&row.author, names),
        agent: is_agent(&row.author, names),
        height: row.height,
        time: row.time,
        reactions: row.reactions,
    }
}

/// A quiet longer than this opens a new run, as Slack's does.
pub const GROUP_GAP_MS: u64 = 5 * 60 * 1000;

/// The first message past the read `boundary` — the row the "New messages"
/// divider sits above. None when there is no boundary.
pub fn unread_seq(messages: &[ChatMessage], boundary: Option<u64>) -> Option<u64> {
    let boundary = boundary.filter(|b| *b > 0)?;
    messages
        .iter()
        .find(|message| !message.pending && message.seq > boundary)
        .map(|message| message.seq)
}

/// Slack-style grouping: a message shows its author header only when it
/// opens a run. Deleted messages, the unread divider, and a quiet longer
/// than [`GROUP_GAP_MS`] always break a run.
pub fn mark_message_groups(messages: &mut [ChatMessage], boundary: Option<u64>) {
    let unread = unread_seq(messages, boundary);
    for index in 0..messages.len() {
        let this = &messages[index];
        let opens = index.checked_sub(1).is_none_or(|i| {
            let above = &messages[i];
            this.deleted
                || above.deleted
                || above.author != this.author
                || unread == Some(this.seq)
                || this.time.saturating_sub(above.time) > GROUP_GAP_MS
        });
        messages[index].show_author = opens;
    }
}

/// The message as one run of plain text — the copy range's lines and the
/// search hit's preview. A mention reads as the NAME it addresses.
pub fn message_body(blocks: &[Block], names: &NameDirectory) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(spans) => span_text(spans, names),
            Block::Quote(spans) => format!("“{}”", span_text(spans, names)),
            Block::Code { lang, text } => match lang {
                Some(lang) => format!("{lang}\n{text}"),
                None => text.clone(),
            },
            Block::Divider => "────────".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Editable markdown with stable mention identities: a mention keeps its
/// `<@7>` token rather than the name it renders as today.
fn draft_body(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(spans) => draft_spans(spans),
            Block::Quote(spans) => format!("> {}", draft_spans(spans)),
            Block::Code { lang, text } => {
                format!("```{}\n{text}\n```", lang.clone().unwrap_or_default())
            }
            Block::Divider => "---".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draft_spans(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| {
            let mention = span.marks.iter().find_map(|mark| match mark {
                Mark::Mention(party) => Some(mention_token(party)),
                _ => None,
            });
            let mut text = mention.unwrap_or_else(|| span.text.clone());
            for mark in &span.marks {
                text = match mark {
                    Mark::Bold => format!("**{text}**"),
                    Mark::Italic => format!("_{text}_"),
                    Mark::Link(url) => format!("[{text}]({url})"),
                    Mark::Mention(_) => text,
                };
            }
            text
        })
        .collect()
}

/// A paragraph's or quote's spans as the runs the frame styles; empty when
/// no span carries a mark, so the block draws as one plain text.
pub fn styled_spans(spans: &[Span], names: &NameDirectory) -> Vec<ChatSpan> {
    if spans.iter().all(|span| span.marks.is_empty()) {
        return Vec::new();
    }
    spans
        .iter()
        .filter_map(|span| {
            let text = span_display(span, names);
            if text.is_empty() {
                return None;
            }
            let link = span.marks.iter().find_map(|mark| match mark {
                Mark::Link(url) => Some(url.clone()),
                _ => None,
            });
            let mention = span.marks.iter().find_map(|mark| match mark {
                Mark::Mention(Party::Account(account)) => Some(account.to_string()),
                Mark::Mention(_) => Some(String::new()),
                _ => None,
            });
            let bold = span.marks.contains(&Mark::Bold);
            let italic = span.marks.contains(&Mark::Italic);
            let style = match (link, mention, bold, italic) {
                (Some(url), _, _, _) => SpanStyle::Link(url),
                (None, Some(account), _, _) => SpanStyle::Mention(account),
                (None, None, true, true) => SpanStyle::BoldItalic,
                (None, None, true, false) => SpanStyle::Bold,
                (None, None, false, true) => SpanStyle::Italic,
                (None, None, false, false) => SpanStyle::Plain,
            };
            Some(ChatSpan { text, style })
        })
        .collect()
}

/// Spans to text; a mention plate shows the account's current name.
pub fn span_text(spans: &[Span], names: &NameDirectory) -> String {
    spans.iter().map(|span| span_display(span, names)).collect()
}

fn span_display(span: &Span, names: &NameDirectory) -> String {
    span.marks
        .iter()
        .find_map(|mark| match mark {
            Mark::Mention(party) => Some(mention_label(party, names)),
            _ => None,
        })
        .unwrap_or_else(|| span.text.clone())
}

/// An author handle (`user:{hex}`, `acct:{n}`, `module:{id}`, `system`) as
/// its label: the bound name when the directory knows it, else a plain
/// rendering of the handle.
pub fn author_display(author: &str, names: &NameDirectory) -> String {
    names.of_handle(author).map_or_else(
        || match author.split_once(':') {
            Some(("user", id)) => format!("user {}", ducktape_view_guest::design::short_hex(id)),
            Some(("acct", account)) => format!("account {account}"),
            Some(("module", id)) => id.to_string(),
            _ => "system".into(),
        },
        str::to_string,
    )
}

pub fn avatar_initial(author: &str, names: &NameDirectory) -> String {
    let source = match author.split_once(':') {
        Some(("user", id)) => names.member_label(id),
        Some(("acct", _)) => author_display(author, names),
        Some(("module", id)) => id.to_owned(),
        _ => "system".into(),
    };
    ducktape_view_guest::design::initial(&source)
}

/// A person's key or account is human; a program account (an agent's) and
/// every module or system author is software.
pub fn is_agent(author: &str, names: &NameDirectory) -> bool {
    match author.split_once(':') {
        Some(("user", _)) => false,
        Some(("acct", number)) => number.parse().is_ok_and(|n| names.is_program(n)),
        _ => true,
    }
}

/// Autocomplete candidates: every named account plus the room's unregistered
/// key members, labelled without the `@`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionChoice {
    pub label: String,
    pub party: Party,
}

pub fn mention_choices(names: &NameDirectory, members: &[ChatMember]) -> Vec<MentionChoice> {
    let mut targets: Vec<MentionChoice> = names
        .by_account
        .keys()
        .map(|account| choice(Party::Account(*account), names))
        .collect();
    for member in members {
        let party = match member.key.strip_prefix("acct:") {
            Some(number) => match number.parse::<u64>() {
                Ok(number) => Party::Account(number),
                Err(_) => continue,
            },
            None => {
                let key = member.key.strip_prefix("user:").unwrap_or(&member.key);
                let key = unhex(key).unwrap_or_else(|| key.as_bytes().to_vec());
                if names.account_of(&hex(&key)).is_some() {
                    continue;
                }
                Party::Key(key)
            }
        };
        if !targets.iter().any(|target| target.party == party) {
            targets.push(choice(party, names));
        }
    }
    targets.sort_by_key(|choice| choice.label.to_lowercase());
    targets
}

fn choice(party: Party, names: &NameDirectory) -> MentionChoice {
    MentionChoice {
        label: mention_label(&party, names)[1..].to_string(),
        party,
    }
}

/// The canonical token the composer inserts: `<@account>` or `<@key:hex>`.
pub fn mention_token(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("<@{account}>"),
        Party::Key(key) => format!("<@key:{}>", hex(key)),
        Party::Module(_) | Party::System => String::new(),
    }
}

pub fn mention_label(party: &Party, names: &NameDirectory) -> String {
    match party {
        Party::Account(account) => names
            .by_account
            .get(account)
            .filter(|name| !name.is_empty())
            .map_or_else(|| format!("@account-{account}"), |name| format!("@{name}")),
        Party::Key(key) => format!("@{}", names.member_label(&hex(key))),
        Party::Module(module) => format!("@{module}"),
        Party::System => "@system".into(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}

pub fn height_label(height: u64) -> String {
    format!("block {}", ducktape_view_guest::design::grouped(height))
}
