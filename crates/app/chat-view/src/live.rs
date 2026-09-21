//! Agent runs answering into chat, drawn as cards while they work: which
//! runs are anchored here (the runs module's pending list), what each one's
//! output says (its `run-output` stream) and what a reader who may not see
//! that output is told instead (committed progress).
use std::collections::BTreeMap;

use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::{Query, ask};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::Runs;
use crate::client::{ChatBlock, ChatMessage};

/// One anchored run as discovery names it: where its card goes and whose it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seed {
    pub channel_id: String,
    pub anchor_seq: u64,
    pub thread_root: u64,
    pub run_id: String,
    /// the run's address, and the topic its output arrives on
    pub dispatch_id: String,
    pub agent: String,
}

/// What one run's output stream has said so far.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub lines: Vec<String>,
    /// a failure the reader can act on — empty while the stream is healthy
    pub error: String,
    /// the node would not admit this device as the run's reader: not a
    /// failure, an entitlement. The card falls back to public progress.
    pub unavailable: bool,
}

/// A run in flight, and all a card needs to draw it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub seed: Seed,
    pub status: String,
    pub activity: Vec<(String, bool)>,
    pub answer_preview: String,
}

impl Run {
    /// Whether this run answers into the thread rooted at `root`: that root
    /// is its anchor, or its anchor is a reply inside the thread.
    pub fn in_thread(&self, root: u64) -> bool {
        self.seed.anchor_seq == root || self.seed.thread_root == root
    }
}

/// The runs anchored in chat and still pending, named by their agents.
/// `labels` caches the agent roster between polls.
pub async fn discover(labels: &mut BTreeMap<String, String>) -> Result<Vec<Seed>, Refusal> {
    let pending = ask::<Query<Runs>>(json!("pending_runs")).await?;
    let records: Vec<Value> = pending["pending_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|record| !record["channel_id"].as_str().unwrap_or_default().is_empty())
        .cloned()
        .collect();
    let unnamed = records
        .iter()
        .any(|record| !labels.contains_key(record["agent_id"].as_str().unwrap_or_default()));
    if unnamed {
        if let Ok(roster) = ask::<Query<Runs>>(json!({"model": {"query": "agents"}})).await {
            for agent in roster["model"]["agents"].as_array().into_iter().flatten() {
                let Some(id) = agent["agent_id"].as_str() else {
                    continue;
                };
                let label = agent["display_name"].as_str().unwrap_or(id);
                labels.insert(id.into(), label.into());
            }
        }
        for record in &records {
            let id = record["agent_id"].as_str().unwrap_or_default();
            labels.entry(id.into()).or_insert_with(|| id.into());
        }
    }
    Ok(records
        .iter()
        .map(|record| {
            let agent_id = record["agent_id"].as_str().unwrap_or_default();
            Seed {
                channel_id: record["channel_id"].as_str().unwrap_or_default().into(),
                anchor_seq: record["anchor_seq"].as_u64().unwrap_or_default(),
                thread_root: record["thread_root"].as_u64().unwrap_or_default(),
                run_id: record["run_id"].as_str().unwrap_or_default().into(),
                dispatch_id: record["dispatch_id"].as_str().unwrap_or_default().into(),
                agent: labels
                    .get(agent_id)
                    .cloned()
                    .unwrap_or_else(|| agent_id.into()),
            }
        })
        .collect())
}

/// The committed public status of the runs this device may not read the
/// output of: only facts, never provider text.
pub async fn progress(runs: Vec<String>) -> BTreeMap<String, String> {
    let sessions = ask::<Query<Runs>>(json!("agent_sessions")).await.ok();
    let mut out = BTreeMap::new();
    for run_id in runs {
        let delegations = ask::<Query<Runs>>(json!({"delegations": {"caller_run_id": run_id}}))
            .await
            .ok();
        out.insert(
            run_id.clone(),
            public_status(&run_id, sessions.as_ref(), delegations.as_ref()),
        );
    }
    out
}

fn public_status(run_id: &str, sessions: Option<&Value>, delegations: Option<&Value>) -> String {
    let calls = delegations.and_then(|reply| reply["delegations"].as_array());
    if calls.is_some_and(|calls| calls.iter().any(|call| call["status"] == "pending")) {
        return "Working · peer call pending".into();
    }
    if calls.is_some_and(|calls| calls.iter().any(|call| call["status"] == "delivered")) {
        return "Working · peer reply received".into();
    }
    let Some(sessions) = sessions.and_then(|reply| reply["agent_sessions"].as_array()) else {
        return "Working".into();
    };
    let Some(session) = sessions.iter().find(|session| session["run_id"] == run_id) else {
        return "Starting".into();
    };
    match session["actions"].as_u64().unwrap_or_default() {
        0 => "Working".into(),
        1 => "Working · 1 action recorded".into(),
        count => format!("Working · {count} actions recorded"),
    }
}

/// The tokens that say WHO may read rather than that something broke.
const UNREADABLE: [&str; 3] = ["session_locked", "unauthorized", "forbidden"];

/// One frame of a run's output stream, folded into the reading.
pub fn fold_output(item: &mut Output, topic: &str, frame: Result<Value, Refusal>) {
    let value = match frame {
        Ok(value) => value,
        Err(refusal) => {
            item.unavailable = UNREADABLE.contains(&refusal.reason.as_str());
            item.error = if item.unavailable {
                String::new()
            } else {
                refusal.sentence
            };
            return;
        }
    };
    if value["type"] == "error" {
        let detail = value["detail"]
            .as_str()
            .unwrap_or("The node refused this run output subscription.");
        item.unavailable = UNREADABLE.contains(&value["code"].as_str().unwrap_or_default());
        item.error = if item.unavailable {
            String::new()
        } else {
            detail.to_owned()
        };
        return;
    }
    if value["topic"].as_str() != Some(topic) {
        return;
    }
    let Some(line) = value["item"]["line"].as_str() else {
        return;
    };
    item.error.clear();
    item.unavailable = false;
    item.lines.push(line.to_owned());
}

/// A card, from the seed and the output this view streamed for it.
pub fn project(seed: &Seed, output: Option<&Output>, public: Option<&String>) -> Run {
    let mut run = Run {
        seed: seed.clone(),
        status: "Starting".into(),
        ..Run::default()
    };
    let Some(output) = output else {
        return run;
    };
    for (id, line) in output.lines.iter().enumerate() {
        if let Some(event) = output_event(line, id) {
            apply(&mut run, &event);
        }
    }
    if !output.error.is_empty() {
        run.status = output.error.clone();
    }
    if output.unavailable {
        run.status = public.cloned().unwrap_or_else(|| "Working".into());
    }
    run
}

/// A run in flight drawn as a message from its agent: the byline is the
/// agent's, the body is what it has done and is writing, the caption its
/// status. Nothing here is on the chain, so the row is pending.
pub fn run_message(run: &Run) -> ChatMessage {
    let paragraph = |text: String| ChatBlock {
        kind: "paragraph".into(),
        text,
        ..ChatBlock::default()
    };
    let mut blocks: Vec<ChatBlock> = run
        .activity
        .iter()
        .map(|(label, done)| paragraph(format!("{} {label}", if *done { "✓" } else { "·" })))
        .collect();
    if !run.answer_preview.is_empty() {
        blocks.push(paragraph(run.answer_preview.clone()));
    }
    ChatMessage {
        id: format!("live/{}", run.seed.run_id),
        author: run.seed.agent.clone(),
        meta: run.status.clone(),
        blocks,
        pending: true,
        show_author: true,
        initial: run
            .seed
            .agent
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default(),
        agent: true,
        ..ChatMessage::default()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Event {
    kind: &'static str,
    title: String,
    detail: String,
    done: bool,
    answer: String,
}

fn status(title: impl Into<String>) -> Option<Event> {
    Some(Event {
        kind: "status",
        title: title.into(),
        detail: String::new(),
        done: false,
        answer: String::new(),
    })
}

fn preview(answer: String) -> Option<Event> {
    Some(Event {
        kind: "preview",
        title: "Writing the answer".into(),
        detail: String::new(),
        done: false,
        answer,
    })
}

/// One provider line as the card reads it. Live output does not identify
/// its provider: Pi's event types, Claude's message shapes and Codex's items
/// are told apart by shape. Tool names describe activity without copying
/// arguments, tool output or thinking into the chat status.
fn output_event(line: &str, _id: usize) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str() {
        Some("message_end") => {
            let message = &value["message"];
            let completed = message["role"] == "assistant"
                && matches!(
                    message["stopReason"].as_str(),
                    Some("stop" | "length" | "toolUse")
                );
            if !completed {
                return None;
            }
            let answer = message["content"]
                .as_array()?
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if answer.trim().is_empty() {
                return None;
            }
            return preview(answer);
        }
        Some("tool_execution_start") => {
            return status(format!("Using {}", value["toolName"].as_str()?));
        }
        Some("tool_execution_end") => {
            return status(if value["isError"].as_bool()? {
                "Tool failed · waiting for agent"
            } else {
                "Tool finished · waiting for agent"
            });
        }
        Some("result") => return preview(value["result"].as_str()?.to_string()),
        Some("assistant") => {
            let blocks = value["message"]["content"].as_array()?;
            let tool = blocks
                .iter()
                .rev()
                .find(|block| block["type"] == "tool_use")?;
            return status(format!("Using {}", tool["name"].as_str()?));
        }
        Some("user") => {
            let blocks = value["message"]["content"].as_array()?;
            let result = blocks
                .iter()
                .rev()
                .find(|block| block["type"] == "tool_result")?;
            return status(if result["is_error"] == true {
                "Tool failed · waiting for agent"
            } else {
                "Tool finished · waiting for agent"
            });
        }
        _ => {}
    }
    let event_type = value["type"].as_str().unwrap_or_default();
    let item = &value["item"];
    let item_type = item["type"]
        .as_str()
        .or_else(|| item["item_type"].as_str())
        .unwrap_or_default();
    if item_type == "agent_message" {
        let answer = item["text"]
            .as_str()
            .or_else(|| item["message"].as_str())?
            .to_string();
        return preview(answer);
    }
    let (title, detail) = match item_type {
        "reasoning" => (
            "Reasoning".to_string(),
            json_text(item.get("text").or_else(|| item.get("summary"))),
        ),
        "command_execution" => (
            "Command".to_string(),
            json_text(
                item.get("command")
                    .or_else(|| item.get("aggregated_output")),
            ),
        ),
        "mcp_tool_call" => (
            format!(
                "{} · {}",
                item["server"].as_str().unwrap_or("tool"),
                item["tool"].as_str().unwrap_or("call")
            ),
            json_text(item.get("arguments")),
        ),
        "web_search" => ("Web search".to_string(), json_text(item.get("query"))),
        _ => return None,
    };
    Some(Event {
        kind: "activity",
        title,
        detail,
        done: event_type.ends_with("completed"),
        answer: String::new(),
    })
}

fn json_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Some(other) => other.to_string(),
    }
}

fn apply(run: &mut Run, event: &Event) {
    match event.kind {
        "status" => run.status = event.title.clone(),
        "activity" => {
            let label = if event.detail.is_empty() {
                event.title.clone()
            } else {
                format!("{}: {}", event.title, event.detail)
            };
            match run.activity.iter_mut().find(|(known, _)| *known == label) {
                Some((_, done)) => *done |= event.done,
                None => run.activity.push((label, event.done)),
            }
            run.status = event.title.clone();
        }
        _ => {
            // a provider's structured payload before the words is not a
            // preview anyone reads
            let raw = matches!(event.answer.trim_start().chars().next(), Some('{' | '['));
            if !raw {
                run.answer_preview = event.answer.clone();
            }
            run.status = "Answering".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_lines_fold_into_a_card_and_entitlement_is_by_token() {
        let seed = Seed {
            agent: "Reviewer".into(),
            ..Seed::default()
        };
        let mut output = Output::default();
        for line in [
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","is_error":false}]}}"#,
            r#"{"type":"result","result":"All good."}"#,
        ] {
            fold_output(
                &mut output,
                "run-output:d",
                Ok(json!({"topic": "run-output:d", "item": {"line": line}})),
            );
        }
        let run = project(&seed, Some(&output), None);
        assert_eq!(run.status, "Answering");
        assert_eq!(run.answer_preview, "All good.");
        assert_eq!(run_message(&run).author, "Reviewer");

        let mut refused = Output::default();
        fold_output(
            &mut refused,
            "run-output:d",
            Err(Refusal::new("forbidden", "words a view must not read")),
        );
        assert!(refused.unavailable && refused.error.is_empty());
        let public = "Working · 2 actions recorded".to_string();
        assert_eq!(project(&seed, Some(&refused), Some(&public)).status, public);
        let mut broke = Output::default();
        fold_output(
            &mut broke,
            "run-output:d",
            Err(Refusal::new("stream_open_failed", "401 in the sentence")),
        );
        assert!(!broke.unavailable && broke.error == "401 in the sentence");
    }
}
