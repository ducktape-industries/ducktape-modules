//! Headless participation: the host pushes a `background` request on the
//! props stream and this view answers it without a screen — desktop notice
//! policy, huddle join/leave/move, a search, the shell's room facts and the
//! live-run reads. The answer goes back through `finish_response`.
use ducktape_view_guest::host::{self, Refusal};
use ducktape_view_guest::view::{Submit, ViewOf, ask};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{Admin, ChatApi};
use crate::chat::{Block, ChatViewQuery, ChatViewReply, Mark, Party, party_handle, unhex};
use crate::client::{NameDirectory, author_display, dm_channel_id, message_body};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    ShellDelta {
        payload: Value,
        assigned: Option<Value>,
        key: String,
        names: Value,
    },
    RunProgress {
        runs: Vec<String>,
    },
    LiveRuns {
        labels: std::collections::BTreeMap<String, String>,
    },
    Workspace {
        requested: Option<String>,
        key: String,
    },
    Window {
        channel: String,
        key: String,
    },
    Channel {
        channel: String,
        key: String,
        names: Value,
    },
    Notice {
        request: NoticeRequest,
    },
    Search {
        channel: String,
        text: String,
    },
    Join {
        channel: String,
    },
    Move {
        from: String,
        channel: String,
    },
    Leave {
        channel: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoticeRequest {
    pub payload: Value,
    pub assigned: Option<Value>,
    pub context: Context,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub key: Vec<u8>,
    pub screen: OnScreen,
}

/// What the reader can already see for themselves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OnScreen {
    pub app_focused: bool,
    pub active_channel: String,
}

/// One notification, already worded — the shape the platform call takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopNotice {
    pub title: String,
    pub subtitle: String,
    pub body: String,
    /// the room, for grouping every notice from one conversation together
    pub thread: String,
}

#[derive(Serialize)]
struct Failure {
    message: String,
    committed: bool,
}

pub async fn run(request: Request, names: NameDirectory) {
    let output = match participate(request, names).await {
        Ok(output) => output,
        Err((message, committed)) => json!({"error": Failure { message, committed }}),
    };
    host::finish_response(&serde_json::to_vec(&output).expect("background result encodes"));
}

type Outcome = Result<Value, (String, bool)>;

fn failed(refusal: Refusal) -> (String, bool) {
    (refusal.sentence, false)
}

async fn participate(request: Request, names: NameDirectory) -> Outcome {
    match request {
        Request::Notice { request } => Ok(json!({"notice": notice(request, &names).await})),
        Request::Search { channel, text } => search(channel, text, &names).await,
        Request::Join { channel } => join(channel).await.map(|c| json!({"channel": c})),
        Request::Leave { channel } => leave(channel).await.map(|c| json!({"channel": c})),
        Request::Move { from, channel } => move_seat(from, channel)
            .await
            .map(|c| json!({"channel": c})),
        Request::LiveRuns { mut labels } => {
            let seeds = crate::live::discover(&mut labels).await.map_err(failed)?;
            Ok(json!({"records": seeds, "labels": labels}))
        }
        Request::RunProgress { runs } => Ok(json!(crate::live::progress(runs).await)),
        Request::Workspace { requested, key } => workspace(requested, &key, &names).await,
        Request::Window { channel, key } => window(channel, &key, &names).await,
        Request::Channel { channel, key, .. } => {
            Ok(json!({"channel": facts(&channel, &key, &names).await?}))
        }
        Request::ShellDelta {
            payload,
            assigned,
            key,
            ..
        } => delta(payload, assigned, &key, &names).await,
    }
}

// ---------- huddle participation ----------

fn participation_channel(channel: &str) -> Result<&str, (String, bool)> {
    let channel = channel.trim();
    if channel.is_empty() || channel.len() > 256 {
        return Err(("a channel is required".into(), false));
    }
    Ok(channel)
}

async fn join(channel: String) -> Result<String, (String, bool)> {
    let channel = participation_channel(&channel)?;
    let proof =
        ask::<Admin>(json!({"route": "/v1/huddle/node-proof", "payload": {"channel_id": channel}}))
            .await
            .map_err(failed)?;
    let node = proof["node"].as_str().unwrap_or_default();
    let signature = proof["node_proof"].as_str().unwrap_or_default();
    let (Some(node), Some(signature)) = (unhex(node), unhex(signature)) else {
        return Err(("invalid node participation proof".into(), false));
    };
    if node.len() != 32 || signature.len() != 64 {
        return Err(("invalid node participation proof".into(), false));
    }
    submit(json!({"join_huddle": {"channel_id": channel, "node": node, "node_proof": signature}}))
        .await?;
    Ok(channel.to_owned())
}

async fn leave(channel: String) -> Result<String, (String, bool)> {
    let channel = participation_channel(&channel)?;
    submit(json!({"leave_huddle": {"channel_id": channel}})).await?;
    Ok(channel.to_owned())
}

async fn move_seat(from: String, channel: String) -> Result<String, (String, bool)> {
    let channel = participation_channel(&channel)?.to_owned();
    if from == channel {
        return Ok(channel);
    }
    if from.is_empty() {
        return join(channel).await;
    }
    leave(from).await?;
    join(channel).await.map_err(|(message, _)| (message, true))
}

/// A raw chat op: the huddle ops carry key bytes this view's `ChatMsg`
/// slice does not spell.
async fn submit(payload: Value) -> Result<(), (String, bool)> {
    ask::<Submit<RawChat>>(payload)
        .await
        .map(|_| ())
        .map_err(failed)
}

struct RawChat;
impl ducktape_view_guest::view::Module for RawChat {
    const NAME: &'static str = "chat";
    type Op = Value;
    type Query = Value;
    type Reply = Value;
    type ViewQuery = Value;
    type ViewReply = Value;
}

// ---------- search ----------

async fn search(channel: String, text: String, names: &NameDirectory) -> Outcome {
    let text = text.trim();
    if text.is_empty() || text.contains('\0') {
        return Err(("search must be nonempty and contain no NUL".into(), false));
    }
    let channel_id = (!channel.is_empty()).then_some(channel);
    let (hits, capped, has_more, next_after) =
        crate::search_hits(text.to_owned(), channel_id, Vec::new(), None)
            .await
            .map_err(failed)?;
    let hits: Vec<Value> = hits
        .iter()
        .map(|hit| {
            json!({
                "channel_id": hit.channel_id,
                "seq": hit.seq,
                "root_seq": hit.thread.unwrap_or(hit.seq),
                "author": author_display(&hit.author, names),
                "text": message_body(&hit.blocks, names),
                "meta": format!("{} · #{}", hit.channel_id, hit.seq),
            })
        })
        .collect();
    Ok(json!({"hits": hits, "capped": capped, "has_more": has_more, "next_after": next_after}))
}

// ---------- desktop notices ----------

async fn notice(request: NoticeRequest, names: &NameDirectory) -> Option<DesktopNotice> {
    let operation = request.payload.as_object()?;
    if operation.len() != 1 {
        return None;
    }
    match operation.keys().next()?.as_str() {
        "post_message" => message_notice(request, names).await,
        "join_huddle" => joined_notice(request, names).await,
        _ => None,
    }
}

async fn channel_row(channel_id: &str) -> Option<crate::chat::ChannelInfo> {
    match ask::<ViewOf<ChatApi>>(ChatViewQuery::Channel {
        channel_id: channel_id.to_owned(),
    })
    .await
    {
        Ok(ChatViewReply::Channel(Some(info))) => Some(info),
        _ => None,
    }
}

async fn message_notice(request: NoticeRequest, names: &NameDirectory) -> Option<DesktopNotice> {
    let stamp = request.assigned.as_ref()?.get("posted")?;
    let post = request.payload.get("post_message")?;
    let channel_id = post["channel_id"].as_str()?.to_owned();
    let actor: Party = serde_json::from_value(stamp["actor"].clone()).ok()?;
    let blocks: Vec<Block> = serde_json::from_value(post["blocks"].clone()).ok()?;
    let key = &request.context.key;
    let has_reader = !key.is_empty();
    let me = names.party_of(key);
    let authored_by_me = has_reader && (actor == me || actor == Party::Key(key.clone()));
    if authored_by_me {
        return None;
    }
    let mentions_me = has_reader
        && blocks.iter().any(|block| match block {
            Block::Paragraph(spans) | Block::Quote(spans) => spans.iter().any(|span| {
                span.marks
                    .iter()
                    .any(|mark| matches!(mark, Mark::Mention(party) if party == &me))
            }),
            Block::Code { .. } | Block::Divider => false,
        });
    let mine = match me {
        Party::Account(number) => Some(number),
        _ => None,
    };
    let dm = mine.and_then(|mine| {
        names.accounts().find_map(|(peer, name)| {
            (*peer != mine && dm_channel_id(&mine.to_string(), &peer.to_string()) == channel_id)
                .then(|| name.clone())
        })
    });
    let author = author_display(&party_handle(&actor), names);
    let subtitle = match (mentions_me, dm.is_some()) {
        (true, _) => format!("{author} mentioned you"),
        (false, true) => author,
        (false, false) => return None,
    };
    let screen = &request.context.screen;
    if screen.app_focused && screen.active_channel == channel_id {
        return None;
    }
    let room = match dm {
        Some(name) => name,
        None => channel_row(&channel_id).await.map_or_else(
            || channel_id.clone(),
            |info| format!("#{}", info.channel.name),
        ),
    };
    Some(DesktopNotice {
        title: room,
        subtitle,
        body: excerpt(&message_body(&blocks, names)),
        thread: channel_id,
    })
}

async fn joined_notice(request: NoticeRequest, names: &NameDirectory) -> Option<DesktopNotice> {
    let channel_id = request.payload["join_huddle"]["channel_id"].as_str()?;
    let info = channel_row(channel_id).await?;
    let [first] = info.channel.huddle.as_slice() else {
        return None;
    };
    let key = &request.context.key;
    let own_seat = !key.is_empty() && names.owns_handle(&first.party, key);
    if own_seat {
        return None;
    }
    Some(DesktopNotice {
        title: format!("#{}", info.channel.name),
        subtitle: format!("{} started a call", author_display(&first.party, names)),
        body: "Join from the room list.".into(),
        thread: channel_id.into(),
    })
}

/// How much of a message body a notification carries.
const EXCERPT_CHARS: usize = 140;

pub fn excerpt(body: &str) -> String {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= EXCERPT_CHARS {
        return flat;
    }
    let kept: String = flat.chars().take(EXCERPT_CHARS).collect();
    format!("{}…", kept.trim_end())
}

// ---------- the shell's room facts ----------

fn shell_channel(info: &crate::chat::ChannelInfo, names: &NameDirectory, key: &str) -> Value {
    let huddle: Vec<Value> = info
        .channel
        .huddle
        .iter()
        .map(|seat| {
            let label = names.member_label(&seat.party);
            json!({
                "label": label,
                "initials": ducktape_view_guest::wire::kit::initials(&label),
                "is_you": unhex(key).is_some_and(|key| names.owns_handle(&seat.party, &key)),
                "node": seat.node,
            })
        })
        .collect();
    json!({
        "id": info.channel.id,
        "name": info.channel.name,
        "archived": info.channel.archived,
        "members_only": crate::chat::members_only(info),
        "huddle_count": info.channel.huddle.len(),
        "head_seq": info.head_seq,
        "huddle": huddle,
        "voice": info.channel.voice,
    })
}

async fn facts(id: &str, key: &str, names: &NameDirectory) -> Result<Value, (String, bool)> {
    let Some(info) = channel_row(id).await else {
        return Ok(Value::Null);
    };
    let roster: Vec<Value> = info
        .channel
        .huddle
        .iter()
        .map(|seat| {
            let label = names.member_label(&seat.party);
            json!({
                "key": seat.party,
                "label": label,
                "initials": ducktape_view_guest::wire::kit::initials(&label),
                "is_agent": false,
                "is_you": unhex(key).is_some_and(|key| names.owns_handle(&seat.party, &key)),
                "node": seat.node,
            })
        })
        .collect();
    Ok(json!([shell_channel(&info, names, key), roster]))
}

fn workspace_data(channels: Vec<Value>, active: Option<(Value, Value)>) -> Value {
    let (active, roster) = active.unwrap_or((Value::Null, json!([])));
    json!({
        "generation": 0, "channels": channels,
        "active_channel": active["id"].as_str().unwrap_or_default(),
        "active_channel_name": active["name"].as_str().unwrap_or_default(),
        "active_channel_archived": active["archived"].as_bool().unwrap_or_default(),
        "huddle_roster": roster,
    })
}

async fn workspace(requested: Option<String>, key: &str, names: &NameDirectory) -> Outcome {
    let channels = crate::channels().await.map_err(failed)?;
    let selected = requested
        .as_deref()
        .and_then(|id| channels.iter().find(|info| info.channel.id == id))
        .or_else(|| {
            channels
                .iter()
                .find(|info| !info.channel.archived && info.head_seq > 0)
        })
        .or_else(|| channels.iter().find(|info| !info.channel.archived))
        .or_else(|| channels.first());
    let rows: Vec<Value> = channels
        .iter()
        .map(|info| shell_channel(info, names, key))
        .collect();
    let Some(selected) = selected else {
        return Ok(workspace_data(rows, None));
    };
    let facts = facts(&selected.channel.id, key, names).await?;
    let Value::Array(pair) = facts else {
        return Err(("selected channel disappeared during loading".into(), false));
    };
    let [active, roster] = pair.as_slice() else {
        return Err(("selected channel disappeared during loading".into(), false));
    };
    Ok(workspace_data(rows, Some((active.clone(), roster.clone()))))
}

async fn window(id: String, key: &str, names: &NameDirectory) -> Outcome {
    let facts = facts(&id, key, names).await?;
    let Value::Array(pair) = facts else {
        return Box::pin(workspace(None, key, names)).await;
    };
    let [active, roster] = pair.as_slice() else {
        return Box::pin(workspace(None, key, names)).await;
    };
    Ok(workspace_data(
        vec![active.clone()],
        Some((active.clone(), roster.clone())),
    ))
}

/// The shell retains channel rows and unread heads, never message bodies.
async fn delta(
    payload: Value,
    assigned: Option<Value>,
    key: &str,
    names: &NameDirectory,
) -> Outcome {
    let operation = payload
        .as_object()
        .ok_or(("Chat operation is not an object".to_owned(), false))?;
    if operation.len() != 1 {
        return Err(("Chat operation must name one action".into(), false));
    }
    let (kind, body) = operation.iter().next().expect("one action");
    let stamp = assigned.unwrap_or(Value::Null);
    if kind == "post_message" {
        let channel_id = body["channel_id"]
            .as_str()
            .ok_or(("post has no channel".to_owned(), false))?;
        let seq = stamp["posted"]["seq"]
            .as_u64()
            .ok_or(("post has no assigned sequence".to_owned(), false))?;
        return Ok(json!({"delta": {"head": {"channel_id": channel_id, "seq": seq}}}));
    }
    let id = match kind.as_str() {
        "create_dm_channel" => stamp["dm_channel"]["channel_id"].as_str(),
        "create_channel"
        | "create_voice_channel"
        | "rename_channel"
        | "set_channel_archived"
        | "join_huddle"
        | "leave_huddle" => body["channel_id"].as_str(),
        _ => return Ok(json!({"delta": null})),
    }
    .ok_or(("channel operation has no channel".to_owned(), false))?;
    let facts = facts(id, key, names).await?;
    let row = facts[0].clone();
    if row.is_null() {
        return Err(("changed channel disappeared".into(), false));
    }
    Ok(json!({"delta": {"channel": {"channel": row}}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excerpt_is_one_line_cut_on_a_character() {
        assert_eq!(excerpt("a\n\n b   c"), "a b c");
        let long = "가".repeat(200);
        let cut = excerpt(&long);
        assert_eq!(cut.chars().count(), EXCERPT_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
