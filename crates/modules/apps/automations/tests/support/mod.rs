//! Minimal protocol fixtures for Automations' integration tests.
//!
//! These are not sibling implementations. They own only the state needed to
//! exercise Automations' documented cross-module calls, and speak the local
//! consumer contracts backed by immutable fixtures in `src/consumer_wire.rs`.

#![allow(dead_code, unused_imports)]

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use automations::consumer_wire::{Party, chat, tasks};
use sdk::{Ctx, Error, Module, ModuleId, Msg, Origin, StagedStore, StateRoot};
use sdk::refusal;
use sdk_testkit::MemStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const STATE_KEY: &[u8] = b"protocol-fixture";

async fn load<T>(store: &StagedStore) -> Result<Option<T>, Error>
where
    T: for<'a> Deserialize<'a>,
{
    let Some(bytes) = store.get(STATE_KEY).await? else {
        return Ok(None);
    };
    sdk::wire::decode(&bytes).map(Some).map_err(|sentence| Error::Module {
        reason: refusal::CORRUPT.into(),
        sentence,
    })
}

fn stage<T: Serialize>(store: &mut StagedStore, value: &T) {
    store.stage(STATE_KEY.to_vec(), sdk::wire::encode(value));
}

fn actor(origin: &Origin) -> Party {
    match origin {
        Origin::External(key) => Party::Key(key.clone()),
        Origin::Module(id) => Party::Module(id.clone()),
        Origin::Program(account) => Party::Account(*account),
        Origin::System => Party::System,
    }
}

fn origin_value(party: &Party) -> Value {
    match party {
        Party::Account(account) => json!({"Program": account}),
        Party::Key(key) => json!({"External": key}),
        Party::Module(id) => json!({"Module": id}),
        Party::System => json!("System"),
    }
}

fn full_channel_reply(channel_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "channel": {
            "id": channel_id,
            "name": channel_id,
            "created_at": 0,
            "head_seq": 0,
            "post_policy": "open",
            "hooks": ["automations"],
            "pinned": [],
            "huddle": [],
            "voice": false,
            "owner": "system",
            "archived": false,
            "revision": 0
        }
    }))
    .expect("fixture channel reply")
}

fn full_message_value(channel_id: &str, seq: u64, message: &ChatMessage) -> Value {
    let origin = origin_value(&message.author);
    json!({
        "channel_id": channel_id,
        "seq": seq,
        "head": {
            "message_id": message.id,
            "author": message.author,
            "origin": origin,
            "content_origin": origin,
            "blocks": message.blocks,
            "created_at": 0,
            "rev": 0,
            "revision": 1,
            "edited_at": null,
            "base_rev": null,
            "deleted": false,
            "thread": null,
            "reply_count": 0,
            "last_reply_seq": null
        }
    })
}

fn full_messages_reply(values: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({"messages": values})).expect("fixture messages reply")
}

fn full_message_reply(channel_id: &str, seq: u64, message: &ChatMessage) -> Vec<u8> {
    serde_json::to_vec(&json!({"message": full_message_value(channel_id, seq, message)}))
        .expect("fixture message reply")
}

fn full_task_reply(task: &tasks::Task) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "task": {
            "task": {
                "id": task.id,
                "title": task.title,
                "status": "open",
                "owner": task.owner,
                "created_at": 0,
                "updated_at": 0
            }
        }
    }))
    .expect("fixture task reply")
}

#[derive(Default, Serialize, Deserialize)]
struct ChatState {
    channels: BTreeSet<String>,
    hooks: BTreeMap<String, Vec<String>>,
    messages: BTreeMap<String, Vec<ChatMessage>>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    id: String,
    author: Party,
    blocks: Vec<chat::Block>,
}

pub struct ProtocolChat {
    staged: StagedStore,
}

impl ProtocolChat {
    pub fn new() -> Self {
        Self {
            staged: StagedStore::new(Box::new(MemStore::new())),
        }
    }

    async fn state(&self) -> Result<ChatState, Error> {
        Ok(load(&self.staged).await?.unwrap_or_default())
    }
}

#[async_trait(?Send)]
impl Module for ProtocolChat {
    fn id(&self) -> ModuleId {
        "chat".into()
    }

    fn root(&self) -> StateRoot {
        self.staged.root()
    }

    async fn execute(&mut self, ctx: &mut dyn Ctx, msg: &Msg) -> Result<(), Error> {
        let mut state = self.state().await?;
        match chat::decode_msg(&msg.payload).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })? {
            chat::ChatMsg::CreateChannel { channel_id, .. } => {
                state.channels.insert(channel_id);
            }
            chat::ChatMsg::RegisterHook {
                channel_id,
                module_id,
            } => {
                state.hooks.entry(channel_id).or_default().push(module_id);
            }
            chat::ChatMsg::PostMessage {
                channel_id,
                message_id,
                blocks,
                thread: _,
            } => {
                if !state.channels.contains(&channel_id) {
                    return Err(Error::Module {
                        reason: refusal::NOT_FOUND.into(),
                        sentence: format!("unknown channel: {channel_id}"),
                    });
                }
                if state
                    .messages
                    .values()
                    .flatten()
                    .any(|message| message.id == message_id)
                {
                    return Err(Error::Module {
                        reason: refusal::ALREADY_EXISTS.into(),
                        sentence: format!("message id already exists: {message_id}"),
                    });
                }
                let messages = state.messages.entry(channel_id.clone()).or_default();
                let seq = messages.len() as u64 + 1;
                let author = actor(&ctx.env().origin);
                messages.push(ChatMessage {
                    id: message_id,
                    author: author.clone(),
                    blocks,
                });
                for target in state.hooks.get(&channel_id).into_iter().flatten() {
                    ctx.emit_msg(Msg {
                        target: target.clone().into(),
                        payload: chat::encode_event(&chat::ChatEvent::MessagePosted {
                            channel_id: channel_id.clone(),
                            seq,
                            thread_root: None,
                            author: author.clone(),
                            mentions: Vec::new(),
                        }),
                    });
                }
            }
        }
        stage(&mut self.staged, &state);
        Ok(())
    }

    async fn query(&self, req: &[u8]) -> Result<Vec<u8>, Error> {
        let state = self.state().await?;
        let query = chat::decode_query(req).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        match query {
            chat::ChatQuery::Channel { channel_id } => {
                if state.channels.contains(&channel_id) {
                    return Ok(full_channel_reply(&channel_id));
                }
                return Ok(chat::encode_reply(&chat::ChatReply::Channel(None)));
            }
            chat::ChatQuery::Message { message_id } => {
                for (channel_id, messages) in &state.messages {
                    if let Some((seq, message)) = messages
                        .iter()
                        .enumerate()
                        .find(|(_, message)| message.id == message_id)
                    {
                        return Ok(full_message_reply(channel_id, seq as u64 + 1, message));
                    }
                }
                return Ok(chat::encode_reply(&chat::ChatReply::Message(None)));
            }
            chat::ChatQuery::MessagesRange {
                channel_id,
                from_seq,
                limit,
            } => {
                let values = state
                    .messages
                    .get(&channel_id)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .skip(from_seq.saturating_sub(1) as usize)
                    .take(limit as usize)
                    .map(|(index, message)| full_message_value(&channel_id, index as u64 + 1, message))
                    .collect();
                return Ok(full_messages_reply(values));
            }
        }
    }

    async fn commit_block(&mut self) -> Result<(), Error> {
        self.staged.commit().await
    }

    async fn abort_block(&mut self) -> Result<(), Error> {
        self.staged.abort();
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct TasksState {
    tasks: BTreeMap<String, tasks::Task>,
}

pub struct ProtocolTasks {
    staged: StagedStore,
}

impl ProtocolTasks {
    pub fn new() -> Self {
        Self {
            staged: StagedStore::new(Box::new(MemStore::new())),
        }
    }

    async fn state(&self) -> Result<TasksState, Error> {
        Ok(load(&self.staged).await?.unwrap_or_default())
    }
}

#[async_trait(?Send)]
impl Module for ProtocolTasks {
    fn id(&self) -> ModuleId {
        "tasks".into()
    }

    fn root(&self) -> StateRoot {
        self.staged.root()
    }

    async fn execute(&mut self, ctx: &mut dyn Ctx, msg: &Msg) -> Result<(), Error> {
        let mut state = self.state().await?;
        let tasks::TaskMsg::CreateTask {
            task_id,
            title,
            owner,
        } = tasks::decode_task_msg(&msg.payload).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        if state.tasks.contains_key(&task_id) {
            return Err(Error::Module {
                reason: refusal::ALREADY_EXISTS.into(),
                sentence: format!("task already exists: {task_id}"),
            });
        }
        let owner = owner
            .map(Party::Account)
            .unwrap_or_else(|| actor(&ctx.env().origin));
        let owned = state.tasks.values().filter(|task| task.owner == owner).count();
        if owned >= tasks::MAX_OPEN_TASKS_PER_OWNER {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!("task owner at cap: {} open tasks", tasks::MAX_OPEN_TASKS_PER_OWNER),
            });
        }
        state.tasks.insert(
            task_id.clone(),
            tasks::Task {
                id: task_id,
                title,
                owner,
            },
        );
        stage(&mut self.staged, &state);
        Ok(())
    }

    async fn query(&self, req: &[u8]) -> Result<Vec<u8>, Error> {
        let state = self.state().await?;
        let query = tasks::decode_task_query(req).map_err(|sentence| Error::Module {
            reason: refusal::INVALID_INPUT.into(),
            sentence,
        })?;
        let reply = match query {
            tasks::TaskQuery::Get { task_id } => {
                if let Some(task) = state.tasks.get(&task_id) {
                    return Ok(full_task_reply(task));
                }
                return Ok(tasks::encode_task_reply(&tasks::TaskReply::Task(None)));
            }
            tasks::TaskQuery::List { limit, after } => tasks::TaskReply::Tasks(
                state
                    .tasks
                    .iter()
                    .filter(|(id, _)| after.as_deref().is_none_or(|cursor| id.as_str() > cursor))
                    .take(limit.clamp(1, tasks::MAX_LIST_LIMIT) as usize)
                    .map(|(_, task)| task.clone())
                    .collect(),
            ),
            tasks::TaskQuery::OwnerOpenCount { owner } => tasks::TaskReply::OwnerOpenCount(
                state.tasks.values().filter(|task| task.owner == owner).count() as u64,
            ),
        };
        Ok(tasks::encode_task_reply(&reply))
    }

    async fn commit_block(&mut self) -> Result<(), Error> {
        self.staged.commit().await
    }

    async fn abort_block(&mut self) -> Result<(), Error> {
        self.staged.abort();
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct InboxState {
    delivered: u64,
}

pub struct ProtocolInbox {
    staged: StagedStore,
}

impl ProtocolInbox {
    pub fn new() -> Self {
        Self {
            staged: StagedStore::new(Box::new(MemStore::new())),
        }
    }
}

#[async_trait(?Send)]
impl Module for ProtocolInbox {
    fn id(&self) -> ModuleId {
        "inbox".into()
    }

    fn root(&self) -> StateRoot {
        self.staged.root()
    }

    async fn execute(&mut self, _: &mut dyn Ctx, _: &Msg) -> Result<(), Error> {
        let mut state: InboxState = load(&self.staged).await?.unwrap_or_default();
        state.delivered += 1;
        stage(&mut self.staged, &state);
        Ok(())
    }

    async fn query(&self, _: &[u8]) -> Result<Vec<u8>, Error> {
        Err(Error::QueryUnsupported)
    }

    async fn commit_block(&mut self) -> Result<(), Error> {
        self.staged.commit().await
    }

    async fn abort_block(&mut self) -> Result<(), Error> {
        self.staged.abort();
        Ok(())
    }
}
