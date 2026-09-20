//! The small sibling surfaces Runs actually reads and emits.
//!
//! These are deliberately local mirrors, not a replacement wire crate. Their
//! serde shapes track the current platform codec; the owning modules keep the
//! canonical encoders without making Runs link their implementations.

use serde::{Deserialize, Serialize};

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    sdk::wire::encode(value)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    sdk::wire::decode(bytes)
}

pub fn as_wire<T: Serialize, U: serde::de::DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("local contract serializes"))
        .expect("local contract matches the Runs wire shape")
}

pub mod agent {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Program {
        pub steps: Vec<Step>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Step {
        Query {
            module: String,
            query: Value,
            bind: String,
        },
        Call {
            module: String,
            msg: Value,
            bind: String,
            decode: Decode,
            on_failure: Continuation,
        },
        Dispatch {
            recipe_id: String,
            payload: Value,
            bind: String,
            decode: Decode,
            on_failure: Continuation,
        },
        Branch {
            test: Predicate,
            then: u64,
            or: u64,
        },
        Report {
            recipient: Value,
            reason: attribution::Reason,
            detail: Value,
        },
        Finish,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Decode {
        Json,
        Text,
        Bytes,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Continuation {
        Step(u64),
        Unhandled,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(i128),
        Text(String),
        Bytes(Vec<u8>),
        List(Vec<Value>),
        Map(BTreeMap<String, Value>),
        Ref(Vec<String>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Predicate {
        Equals { left: Value, right: Value },
        Defined(Value),
        Not(Box<Predicate>),
        All(Vec<Predicate>),
        Any(Vec<Predicate>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AgentMsg {
        Provision {
            request_id: String,
            name: String,
            program: Program,
        },
        Replace {
            account: u64,
            program: Program,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum CallResult {
        Applied {
            output: serde_json::Value,
            assigned: serde_json::Value,
        },
        Rejected {
            reason: String,
        },
        Refused(serde_json::Value),
        Unrepresentable {
            attempted: serde_json::Value,
        },
        StaleGeneration,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Outstanding {
        Call(sdk::CallId),
        Dispatch { dispatch_id: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Status {
        Running {
            step: u64,
            awaiting: Outstanding,
        },
        Finished {
            at_step: u64,
        },
        Failed {
            step: u64,
            failure: serde_json::Value,
        },
        Aborted {
            at_step: u64,
            reason: serde_json::Value,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct InvocationView {
        pub status: Status,
        pub bindings: BTreeMap<String, serde_json::Value>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct InvocationEntry {
        pub at: u64,
        pub invocation: InvocationView,
    }

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct InvocationPage {
        pub entries: Vec<InvocationEntry>,
        pub has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub next_after: Option<u64>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AgentQuery {
        Invocation {
            account: u64,
            seq: u64,
        },
        Invocations {
            account: u64,
            after: u64,
            limit: u64,
        },
    }

    #[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AgentReply {
        Invocation(Option<InvocationView>),
        Invocations(InvocationPage),
    }

    pub fn encode_query(value: &AgentQuery) -> Vec<u8> {
        super::encode(value)
    }

    pub fn decode_reply(bytes: &[u8]) -> Result<AgentReply, String> {
        super::decode(bytes)
    }

    pub fn from_wire<T: Serialize, U: serde::de::DeserializeOwned>(value: T) -> U {
        serde_json::from_value(serde_json::to_value(value).expect("wire value serializes"))
            .expect("local contract matches the owner wire shape")
    }
}

pub mod chat {
    use super::*;

    pub const MAX_EMOJI_BYTES: usize = 64;
    pub const MAX_THREAD_REPLIES: usize = 4096;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Party {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Mark {
        Bold,
        Italic,
        Link(String),
        Mention(Party),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Span {
        pub text: String,
        pub marks: Vec<Mark>,
    }

    impl Span {
        pub fn plain(text: impl Into<String>) -> Self {
            Self {
                text: text.into(),
                marks: Vec::new(),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Block {
        Paragraph(Vec<Span>),
        Code { lang: Option<String>, text: String },
        Quote(Vec<Span>),
        Divider,
    }

    impl Block {
        pub fn paragraph(text: impl Into<String>) -> Self {
            Self::Paragraph(vec![Span::plain(text)])
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum PostPolicy {
        Open,
        MembersOnly,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct HuddleMember {
        pub party: Party,
        pub node: Vec<u8>,
        pub joined_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Channel {
        pub id: String,
        pub name: String,
        pub created_at: u64,
        pub head_seq: u64,
        pub post_policy: PostPolicy,
        pub hooks: Vec<String>,
        pub pinned: Vec<u64>,
        pub huddle: Vec<HuddleMember>,
        pub voice: bool,
        pub owner: Party,
        pub archived: bool,
        pub revision: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct MessageHead {
        pub message_id: String,
        pub author: Party,
        pub origin: sdk::Origin,
        pub content_origin: sdk::Origin,
        pub blocks: Vec<Block>,
        pub created_at: u64,
        pub rev: u32,
        pub revision: u64,
        pub edited_at: Option<u64>,
        pub base_rev: Option<u32>,
        pub deleted: bool,
        pub thread: Option<u64>,
        pub reply_count: u64,
        pub last_reply_seq: Option<u64>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct MessageView {
        pub channel_id: String,
        pub seq: u64,
        pub head: MessageHead,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatMsg {
        CreateChannel {
            channel_id: String,
            name: String,
            post_policy: PostPolicy,
        },
        PostMessage {
            channel_id: String,
            message_id: String,
            blocks: Vec<Block>,
            thread: Option<u64>,
        },
        AddReaction {
            channel_id: String,
            seq: u64,
            emoji: String,
        },
        RemoveReaction {
            channel_id: String,
            seq: u64,
            emoji: String,
        },
        RegisterHook {
            channel_id: String,
            module_id: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatQuery {
        Channel {
            channel_id: String,
        },
        MessagesRange {
            channel_id: String,
            from_seq: u64,
            limit: u64,
        },
        Message {
            message_id: String,
        },
        Access {
            channel_id: String,
            party: Party,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChannelAccess {
        pub may_read: bool,
        pub may_post: bool,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatReply {
        Channel(Option<Channel>),
        Messages(Vec<MessageView>),
        Message(Option<MessageView>),
        Access(ChannelAccess),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChatEvent {
        MessagePosted {
            channel_id: String,
            seq: u64,
            thread_root: Option<u64>,
            author: Party,
            mentions: Vec<u64>,
        },
    }

    pub fn encode_msg(value: &ChatMsg) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<ChatMsg, String> {
        decode(bytes)
    }
    pub fn encode_query(value: &ChatQuery) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<ChatQuery, String> {
        decode(bytes)
    }
    pub fn encode_reply(value: &ChatReply) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<ChatReply, String> {
        decode(bytes)
    }
    pub fn encode_event(value: &ChatEvent) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_event(bytes: &[u8]) -> Result<ChatEvent, String> {
        decode(bytes)
    }
}

pub mod pages {
    use super::*;
    pub use duck_address::pages::PageAddress;

    pub const MAX_PAGE_TITLE_LEN: usize = 512;
    pub const MAX_BLOCK_LEN: usize = 768 * 1024;
    pub const MAX_PAGES: usize = 2048;
    pub const MAX_COMMENT_TEXT_BYTES: usize = 64 * 1024;
    pub const MAX_COMMENTS_PER_THREAD: usize = 3_498;
    pub const MAX_THREADS_PER_TARGET: usize = 1024;
    pub const MAX_PAGE_QUERY_LIMIT: u16 = 256;
    pub const MAX_THREAD_ID_BYTES: usize = 512;
    pub const MAX_COMMENT_ID_BYTES: usize = 128;
    pub const MANAGED_RECORD_COMMENT_REASON: &str = "managed_record_comment";

    pub fn id_is_index_safe(value: &str) -> bool {
        !value
            .chars()
            .any(|c| c == '"' || c == '\\' || (c as u32) < 0x20)
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum BlockKind {
        Page,
        Paragraph,
        Heading1,
        Heading2,
        Heading3,
        Bulleted,
        Numbered,
        Todo,
        Toggle,
        Quote,
        Code,
        Callout,
        Divider,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum InlineMark {
        Bold,
        Italic,
        Underline,
        Strikethrough,
        Code,
        Mention(u64),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct SpanMark {
        pub start: u32,
        pub end: u32,
        pub kind: InlineMark,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct RelativeAnchor {
        pub start: u32,
        pub end: u32,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Party {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Block {
        pub author: Party,
        pub id: String,
        pub parent: Option<String>,
        pub page: String,
        pub kind: BlockKind,
        pub text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub marks: Vec<SpanMark>,
        pub checked: bool,
        pub children: Vec<String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DiscussionMutation {
        Created,
        Edited,
        Retargeted,
        Recreated,
        ContextChanged,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Thread {
        pub id: String,
        pub target: String,
        pub opener: Party,
        pub created_at: u64,
        pub anchor: Option<RelativeAnchor>,
        pub resolved: bool,
        pub resolved_by: Option<Party>,
        pub comment_ids: Vec<String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Comment {
        pub id: String,
        pub thread_id: String,
        pub author: Party,
        pub text: String,
        pub mentions: Vec<u64>,
        pub created_at: u64,
        pub edited_at: Option<u64>,
        pub deleted: bool,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ThreadView {
        pub thread: Thread,
        pub comments: Vec<Comment>,
        pub has_more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub next_after: Option<String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct DiscussionThreadSnapshot {
        pub id: String,
        pub target: String,
        pub opener: Party,
        pub created_at: u64,
        pub anchor: Option<RelativeAnchor>,
        pub resolved: bool,
        pub resolved_by: Option<Party>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ManagedDiscussionSnapshot {
        pub mutation: DiscussionMutation,
        pub collection_page_id: String,
        pub page_id: String,
        pub comment: Comment,
        pub thread: DiscussionThreadSnapshot,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct NewBlock {
        pub id: String,
        pub kind: BlockKind,
        pub text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub marks: Vec<SpanMark>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum PageMsg {
        CreatePage {
            page_id: String,
            title: String,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            blocks: Vec<NewBlock>,
        },
        InsertBlock {
            parent: String,
            after: Option<String>,
            block: NewBlock,
        },
        SetSpanMark {
            block_id: String,
            start: u32,
            end: u32,
            kind: InlineMark,
            active: bool,
        },
        SetChecked {
            block_id: String,
            checked: bool,
        },
        AddComment {
            thread_id: String,
            comment_id: String,
            target: String,
            text: String,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            anchor: Option<RelativeAnchor>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            mentions: Vec<u64>,
        },
        DeleteComment {
            comment_id: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum PageQuery {
        RecordCollection {
            page_id: String,
        },
        Records {
            page_id: String,
            after: Option<String>,
            limit: u16,
        },
        Record {
            page_id: String,
            record_id: String,
        },
        RecordReceipt {
            page_id: String,
            request_id: String,
        },
        RecordState {
            page_id: String,
            key: String,
        },
        GetPage {
            page_id: String,
            after: Option<String>,
            limit: u16,
        },
        GetBlock {
            block_id: String,
        },
        CommentThreadHead {
            thread_id: String,
        },
        CommentThread {
            thread_id: String,
            #[serde(default)]
            after: Option<String>,
            limit: u64,
        },
        GetComment {
            comment_id: String,
        },
        TargetThreadCount {
            target: String,
        },
        PageCount,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct CommentThreadHead {
        pub target: String,
        pub comment_count: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct PageBlockPage {
        pub blocks: Vec<Block>,
        pub next_after: Option<String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum PageReply {
        Page(Option<PageBlockPage>),
        Block(Option<Block>),
        CommentThread(Option<ThreadView>),
        CommentThreadHead(Option<CommentThreadHead>),
        Comment(Option<Comment>),
        TargetThreadCount(u64),
        PageCount(u64),
    }

    pub fn encode_msg(value: &PageMsg) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<PageMsg, String> {
        decode(bytes)
    }
    pub fn encode_query(value: &PageQuery) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<PageQuery, String> {
        decode(bytes)
    }
    pub fn encode_reply(value: &PageReply) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<PageReply, String> {
        decode(bytes)
    }
}

pub mod tasks {
    use super::*;

    pub const MAX_LIST_LIMIT: u64 = 256;
    pub const MAX_TASK_ID: usize = 256;
    pub const MAX_JOB_ID: usize = 256;
    pub const MAX_JOB_COMMENT_TEXT_BYTES: usize = 4096;
    pub const MAX_JOB_COMMENTS: usize = 64;
    pub const MAX_WORKER_TEXT_BYTES: usize = 4096;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Party {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum TaskStatus {
        Open,
        InProgress,
        Done,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Task {
        pub id: String,
        pub title: String,
        pub status: TaskStatus,
        pub owner: Party,
        pub created_at: u64,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum TaskMsg {
        CreateTask {
            task_id: String,
            title: String,
            #[serde(default)]
            owner: Option<u64>,
        },
        UpdateStatus {
            task_id: String,
            status: TaskStatus,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum TaskQuery {
        Get {
            task_id: String,
        },
        List {
            limit: u64,
            #[serde(default)]
            after: Option<String>,
        },
        OwnerOpenCount {
            owner: Party,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum TaskReply {
        Task(Option<Task>),
        Tasks(Vec<Task>),
        OwnerOpenCount(u64),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobStatus {
        Pending,
        Processing,
        Done,
        Failed,
        Cancelled,
    }

    impl JobStatus {
        pub fn is_terminal(&self) -> bool {
            matches!(self, Self::Done | Self::Failed | Self::Cancelled)
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Claim {
        pub worker: Party,
        pub claimed_at_height: u64,
        pub lease_views: u64,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct JobResult {
        pub ok: bool,
        pub payload: String,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct JobComment {
        pub id: String,
        pub author: Party,
        pub text: String,
        pub height: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobControlInput {
        Steer { text: String },
        Cancel,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ControlAcknowledgement {
        pub worker: Party,
        pub attempt: u64,
        pub height: u64,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct JobControl {
        pub operation_id: String,
        pub input: JobControlInput,
        pub author: Party,
        pub height: u64,
        pub acknowledgements: Vec<ControlAcknowledgement>,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum WorkerReportKind {
        Checkpoint,
        Report,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct WorkerReport {
        pub operation_id: String,
        pub worker: Party,
        pub attempt: u64,
        pub height: u64,
        pub kind: WorkerReportKind,
        pub payload: String,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct NativeHistoryHead {
        pub job_attempt: u64,
        pub worker: Party,
        pub run_id: String,
        pub execution_attempt: u32,
        pub revision: u64,
        pub snapshot: String,
        pub height: u64,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct WorkerHistory {
        pub conversation_id: String,
        pub executions: Vec<Job>,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobExecution {
        OneShot,
        Conversation,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Job {
        pub job_id: String,
        pub execution: JobExecution,
        pub conversation_id: String,
        pub previous_job_id: Option<String>,
        pub continuation_operation_id: Option<String>,
        pub controls: Vec<JobControl>,
        pub reports: Vec<WorkerReport>,
        pub native_history: Option<NativeHistoryHead>,
        pub kind: String,
        pub spec: String,
        pub submitter: Party,
        pub status: JobStatus,
        pub attempt: u64,
        pub claim: Option<Claim>,
        pub result: Option<JobResult>,
        pub comments: Vec<JobComment>,
        pub created_at_revision: u64,
        pub created_at_height: u64,
        pub updated_at_height: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsMsg {
        Comment {
            job_id: String,
            created_at_revision: u64,
            comment_id: String,
            text: String,
        },
        Submit {
            job_id: String,
            kind: String,
            spec: String,
        },
        SubmitConversation {
            job_id: String,
            kind: String,
            spec: String,
        },
        Control {
            job_id: String,
            operation_id: String,
            input: JobControlInput,
        },
        AcknowledgeControl {
            job_id: String,
            operation_id: String,
            attempt: u64,
        },
        SettleCancellation {
            job_id: String,
            operation_id: String,
            attempt: u64,
            payload: String,
        },
        Checkpoint {
            job_id: String,
            operation_id: String,
            attempt: u64,
            kind: WorkerReportKind,
            payload: String,
        },
        CheckpointNativeHistory {
            job_id: String,
            attempt: u64,
            run_id: String,
            execution_attempt: u32,
            revision: u64,
            snapshot: String,
        },
        Claim {
            job_id: String,
            lease_views: u64,
        },
        Finalize {
            job_id: String,
            ok: bool,
            payload: String,
        },
        Reclaim {
            job_id: String,
        },
        Prune {
            job_id: String,
        },
        RegisterWorker {},
        UnregisterWorker {},
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct JobEventDetail {
        pub job_id: String,
        pub conversation_id: String,
        pub job_kind: String,
        pub created_at_revision: u64,
        pub job_attempt: u64,
        pub submitter: Party,
        pub actor: Party,
        pub height: u64,
        pub operation: JobsMsg,
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsEvent {
        Submitted {
            job_id: String,
            kind: String,
            submitter: Party,
            spec: String,
            spec_hash: Vec<u8>,
        },
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum JobsQuery {
        Get { job_id: String },
        GetWorker { conversation_id: String },
        Controls { job_id: String },
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    #[allow(
        clippy::large_enum_variant,
        reason = "the local wire mirror preserves the owner reply shape"
    )]
    pub enum JobsReply {
        Job(Option<Job>),
        Worker(Option<WorkerHistory>),
        Controls(Vec<JobControl>),
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WorkMsg {
        Task(TaskMsg),
        Job(JobsMsg),
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum WorkQuery {
        Task(TaskQuery),
        Job(JobsQuery),
    }
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    #[allow(
        clippy::large_enum_variant,
        reason = "the local wire mirror preserves the owner envelope shape"
    )]
    enum WorkReply {
        Task(TaskReply),
        Job(JobsReply),
    }

    pub fn encode_task_msg(value: &TaskMsg) -> Vec<u8> {
        encode(&WorkMsg::Task(value.clone()))
    }
    pub fn encode_task_query(value: &TaskQuery) -> Vec<u8> {
        encode(&WorkQuery::Task(value.clone()))
    }
    pub fn encode_task_reply(value: &TaskReply) -> Vec<u8> {
        encode(&WorkReply::Task(value.clone()))
    }
    pub fn decode_task_msg(bytes: &[u8]) -> Result<TaskMsg, String> {
        match decode(bytes)? {
            WorkMsg::Task(value) => Ok(value),
            WorkMsg::Job(_) => Err("expected task op".into()),
        }
    }
    pub fn decode_task_query(bytes: &[u8]) -> Result<TaskQuery, String> {
        match decode(bytes)? {
            WorkQuery::Task(value) => Ok(value),
            WorkQuery::Job(_) => Err("expected task query".into()),
        }
    }
    pub fn decode_task_reply(bytes: &[u8]) -> Result<TaskReply, String> {
        match decode(bytes)? {
            WorkReply::Task(value) => Ok(value),
            WorkReply::Job(_) => Err("expected task reply".into()),
        }
    }
    pub fn encode_job_msg(value: &JobsMsg) -> Vec<u8> {
        encode(&WorkMsg::Job(value.clone()))
    }
    pub fn encode_job_query(value: &JobsQuery) -> Vec<u8> {
        encode(&WorkQuery::Job(value.clone()))
    }
    pub fn encode_job_reply(value: &JobsReply) -> Vec<u8> {
        encode(&WorkReply::Job(value.clone()))
    }
    pub fn decode_job_msg(bytes: &[u8]) -> Result<JobsMsg, String> {
        match decode(bytes)? {
            WorkMsg::Job(value) => Ok(value),
            WorkMsg::Task(_) => Err("expected job op".into()),
        }
    }
    pub fn decode_job_query(bytes: &[u8]) -> Result<JobsQuery, String> {
        match decode(bytes)? {
            WorkQuery::Job(value) => Ok(value),
            WorkQuery::Task(_) => Err("expected job query".into()),
        }
    }
    pub fn decode_job_reply(bytes: &[u8]) -> Result<JobsReply, String> {
        match decode(bytes)? {
            WorkReply::Job(value) => Ok(value),
            WorkReply::Task(_) => Err("expected job reply".into()),
        }
    }
    pub fn encode_job_event(value: &JobsEvent) -> Vec<u8> {
        encode(value)
    }
    pub fn decode_job_event(bytes: &[u8]) -> Result<JobsEvent, String> {
        decode(bytes)
    }
}
