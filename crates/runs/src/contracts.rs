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

pub mod attribution {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ObjectRef {
        pub kind: String,
        pub object: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(deny_unknown_fields)]
    pub struct Source {
        pub module: String,
        pub kind: String,
        pub object: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Actor {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Reason {
        Mention,
        Authorship,
        Ownership,
        Assignment,
        Credit,
        Result,
        Report,
        Defined(String),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Relation {
        pub recipient: u64,
        pub reason: Reason,
        pub detail: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionMsg {
        Attribute {
            object: ObjectRef,
            revision: u64,
            actor: Actor,
            relations: Vec<Relation>,
            transfers: Vec<Transfer>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Transfer {
        pub reason: Reason,
        pub from: u64,
        pub to: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ChangeKind {
        Added,
        Withdrawn,
        TransferredIn { from: u64 },
        TransferredOut { to: u64 },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Change {
        pub seq: u64,
        pub source: Source,
        pub revision: u64,
        pub recipient: u64,
        pub reason: Reason,
        pub kind: ChangeKind,
        pub detail: Vec<u8>,
        pub actor: Actor,
        pub cause: sdk::Cause,
        pub height: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ChangeEntry {
        pub at: u64,
        pub change: Change,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DeliveryState {
        Queued,
        Retired(sdk::DeliveryOutcome),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Delivery {
        pub item: u64,
        pub subscriber: sdk::ModuleId,
        pub seq: u64,
        pub root: sdk::Root,
        pub state: DeliveryState,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionQuery {
        Changes {
            after: u64,
            limit: u64,
        },
        ChangesOf {
            source: Source,
            after: u64,
            limit: u64,
        },
        DeliveryOf {
            subscriber: sdk::ModuleId,
            seq: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AttributionReply {
        Changes(Vec<ChangeEntry>),
        Delivery(Option<Delivery>),
    }

    pub fn encode_msg(value: &AttributionMsg) -> Vec<u8> {
        super::encode(value)
    }
    pub fn encode_query(value: &AttributionQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<AttributionReply, String> {
        super::decode(bytes)
    }
}

pub mod capability {
    pub const MAX_TAG_LEN: usize = 64;

    pub fn validate_tag(tag: &str) -> Result<(), String> {
        if tag.is_empty() {
            return Err("capability tag must be non-empty".into());
        }
        if tag.len() > MAX_TAG_LEN {
            return Err(format!(
                "capability tag exceeds {MAX_TAG_LEN} bytes: {} bytes",
                tag.len()
            ));
        }
        if !tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(format!(
                "capability tag has invalid characters (want [a-z0-9._-]): {tag:?}"
            ));
        }
        Ok(())
    }
}

pub mod collaboration {
    use super::*;

    pub type Credential = u64;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Party {
        Account(u64),
        Key(Vec<u8>),
        Module(String),
        System,
    }

    pub fn party_handle(party: &Party) -> Option<String> {
        match party {
            Party::Account(account) => Some(format!("acct:{account}")),
            Party::Key(key) => Some(format!("key:{}", hex(key))),
            Party::Module(_) | Party::System => None,
        }
    }

    pub fn parse_party_handle(handle: &str) -> Result<Party, String> {
        match handle.split_once(':') {
            Some(("acct", number)) => number
                .parse::<u64>()
                .map(Party::Account)
                .map_err(|_| format!("{handle:?} is not acct:<number>")),
            Some(("key", encoded)) => {
                if !encoded.len().is_multiple_of(2) {
                    return Err(format!("{handle:?} is not key:<hex>"));
                }
                let bytes = (0..encoded.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&encoded[at..at + 2], 16).ok())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| format!("{handle:?} is not key:<hex>"))?;
                if bytes.is_empty() {
                    return Err("a key handle names no bytes".into());
                }
                Ok(Party::Key(bytes))
            }
            _ => Err(format!(
                "{handle:?} is not a participant handle (acct:<number> or key:<hex>)"
            )),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum MessageKind {
        Notice,
        Question,
        TaskRequest,
        TaskUpdate,
        Result,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct TaskRef {
        pub id: String,
        pub expected_attempt: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Reference {
        Commit { repo: String, commit: String },
        Blob { hash: String },
        Duck { url: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DeliveryState {
        Stored,
        Queued,
        AdapterAccepted,
        Held,
        Refused,
        Expired,
        DeliveryUnknown,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct DeliverRequest {
        pub channel_id: String,
        pub message_id: String,
        pub recipient: Party,
        pub kind: MessageKind,
        #[serde(default)]
        pub task: Option<TaskRef>,
        #[serde(default)]
        pub references: Vec<Reference>,
        pub expires_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum CollaborationMsg {
        Deliver(DeliverRequest),
        Acknowledge {
            channel_id: String,
            seq: u64,
            recipient: Party,
            binding_credential: Credential,
            state: DeliveryState,
            #[serde(default)]
            reason: Option<String>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Request {
        pub network: String,
        pub op: CollaborationMsg,
    }

    impl Request {
        pub fn new(network: impl Into<String>, op: CollaborationMsg) -> Self {
            Self {
                network: network.into(),
                op,
            }
        }
    }

    pub fn encode_msg(value: &Request) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<Request, String> {
        super::decode(bytes)
    }
}

pub mod dispatch {
    use super::*;
    use std::collections::BTreeMap;

    pub const MAX_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;
    pub const MAX_ID_BYTES: usize = 128;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Routing {
        Rendezvous,
        Pinned(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum OutputContract {
        Text,
        Json,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DispatchStatus {
        AwaitingResult { saga_id: String },
        AwaitingDelivery,
        Delivered { delivery: sdk::DeliveryOutcome },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct DispatchView {
        pub dispatch_id: String,
        pub recipe_id: String,
        pub receiver: String,
        pub cause: sdk::Cause,
        pub status: DispatchStatus,
        pub outcome: Option<Result<Vec<u8>, String>>,
        pub created_at: u64,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum AdmissionPolicy {
        #[default]
        Queue,
        FailFast,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ResultEvent {
        pub dispatch_id: String,
        pub recipe_id: String,
        pub outcome: Result<Vec<u8>, String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Refusal {
        NotAProgram,
        Revoked,
        Suspended,
        StaleGeneration,
        WrongExecutor,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Attempt {
        Applied,
        Rejected,
        Refused,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum CallOutcomeSummary {
        Applied {
            output_digest: [u8; 32],
            assigned: Vec<u8>,
        },
        Rejected {
            reason: String,
        },
        Refused(Refusal),
        Unrepresentable {
            attempted: Attempt,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum CallStatus {
        Queued,
        Completed {
            outcome: CallOutcomeSummary,
        },
        Delivered {
            outcome: CallOutcomeSummary,
            delivery: sdk::DeliveryOutcome,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct CallView {
        pub enqueued: u64,
        pub id: sdk::CallId,
        pub account: u64,
        pub generation: u64,
        pub target: sdk::ModuleId,
        pub payload_digest: [u8; 32],
        pub cause: sdk::Cause,
        pub status: CallStatus,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Delivery {
        Result(ResultEvent),
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DispatchMsg {
        RegisterRecipe {
            recipe_id: String,
            description: String,
            capability: String,
            routing: Routing,
            output_contract: OutputContract,
            max_attempts: u32,
            deadline_views: Option<u64>,
            lease_views: Option<u64>,
        },
        UpdateRecipe {
            recipe_id: String,
            description: Option<String>,
            capability: Option<String>,
            routing: Option<Routing>,
            output_contract: Option<OutputContract>,
            max_attempts: Option<u32>,
        },
        RemoveRecipe {
            recipe_id: String,
        },
        Dispatch {
            dispatch_id: String,
            recipe_id: String,
            payload: Vec<u8>,
            demands: BTreeMap<String, u64>,
            #[serde(default, skip_serializing_if = "is_queue")]
            admission: AdmissionPolicy,
        },
        CancelDispatch {
            dispatch_id: String,
        },
        ReassignDispatch {
            dispatch_id: String,
            attempt: u32,
        },
    }

    fn is_queue(value: &AdmissionPolicy) -> bool {
        *value == AdmissionPolicy::Queue
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DispatchQuery {
        Dispatch {
            receiver: String,
            dispatch_id: String,
        },
        Call {
            id: sdk::CallId,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum DispatchReply {
        Dispatch(Option<DispatchView>),
        Call(Option<CallView>),
    }

    pub fn encode_msg(value: &DispatchMsg) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<DispatchMsg, String> {
        super::decode(bytes)
    }
    pub fn encode_delivery(value: &Delivery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_delivery(bytes: &[u8]) -> Result<Delivery, String> {
        super::decode(bytes)
    }
    pub fn encode_query(value: &DispatchQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<DispatchQuery, String> {
        super::decode(bytes)
    }
    pub fn encode_reply(value: &DispatchReply) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<DispatchReply, String> {
        super::decode(bytes)
    }
}

pub mod governance {
    use super::*;

    pub const MIN_ACTIVATION_LEAD: u64 = 4;
    pub const MAX_ACTIVATION_LEAD: u64 = 1_000_000_000;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum GovAction {
        Signal {
            text: String,
        },
        UpdateModule {
            name: String,
            module_id: String,
            activation_lead: u64,
            code_hash: Vec<u8>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum VoterKind {
        ValidatorNode,
        Account,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum VotingRule {
        Threshold { required_yes: u64 },
        ParticipatingMajority { quorum: u64 },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum GovMsg {
        Propose {
            proposal_id: String,
            action: GovAction,
            voting_period: u64,
        },
        Vote {
            proposal_id: String,
            approve: bool,
        },
        Execute {
            proposal_id: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProposalStatus {
        Open,
        Passed,
        Rejected,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ProposalView {
        pub proposal_id: String,
        pub action: GovAction,
        pub proposer: Vec<u8>,
        pub created_at: u64,
        pub deadline: u64,
        pub status: ProposalStatus,
        pub votes: Vec<(Vec<u8>, bool)>,
        pub voter_kind: VoterKind,
        pub electorate: Vec<(Vec<u8>, u64)>,
        pub voting_rule: VotingRule,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct SharesView {
        pub active: bool,
        pub allocations: Vec<ShareAllocation>,
        pub total: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ShareAllocation {
        pub account_id: u64,
        pub shares: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum GovQuery {
        Proposal { proposal_id: String },
        Shares,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum GovReply {
        Proposal(Option<ProposalView>),
        Shares(SharesView),
    }

    pub fn encode_msg(value: &GovMsg) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<GovMsg, String> {
        super::decode(bytes)
    }
    pub fn encode_query(value: &GovQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<GovReply, String> {
        super::decode(bytes)
    }
}

pub mod identity {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct KeyView {
        pub scheme: serde_json::Value,
        pub pubkey: Vec<u8>,
        pub label: Option<String>,
        pub added_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ProgramStanding {
        Active,
        Suspended,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum Control {
        Keys,
        Program {
            controller: u64,
            executor: sdk::ModuleId,
            generation: u64,
            standing: ProgramStanding,
        },
        Revoked {
            controller: u64,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct AccountView {
        pub number: u64,
        pub name: String,
        pub control: Control,
        pub keys: Vec<KeyView>,
        pub avatar: Option<String>,
        pub bio: Option<String>,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityQuery {
        Get { number: u64 },
        OfKey { key: Vec<u8> },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum IdentityReply {
        Account(Option<AccountView>),
    }

    pub fn encode_query(value: &IdentityQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<IdentityQuery, String> {
        super::decode(bytes)
    }
    pub fn encode_reply(value: &IdentityReply) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<IdentityReply, String> {
        super::decode(bytes)
    }
}

pub mod modules {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ScheduledSwap {
        pub name: String,
        pub activation_height: u64,
        pub code_hash: Vec<u8>,
        pub readiness: Vec<Vec<u8>>,
        pub ready_at: Option<u64>,
    }

    impl ScheduledSwap {
        pub fn stale_at(&self, height: u64) -> bool {
            self.activation_height <= height && self.ready_at.is_none()
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct Activation {
        pub height: u64,
        pub code_hash: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct ModuleCode {
        pub module_id: String,
        pub kind: serde_json::Value,
        pub active_code_hash: Vec<u8>,
        pub pending: Option<ScheduledSwap>,
        pub history: Vec<Activation>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ModulesQuery {
        ModuleStatus,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ModulesReply {
        ModuleStatus { modules: Vec<ModuleCode> },
    }

    pub fn encode_query(value: &ModulesQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<ModulesReply, String> {
        super::decode(bytes)
    }
}

pub mod valset {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ValsetQuery {
        Validators,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum ValsetReply {
        Validators(Vec<Vec<u8>>),
    }

    pub fn encode_query(value: &ValsetQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<ValsetReply, String> {
        super::decode(bytes)
    }

    pub async fn members(ctx: &dyn sdk::Ctx, valset: &str) -> Result<Vec<Vec<u8>>, sdk::Error> {
        let reply = ctx
            .query(valset, &encode_query(&ValsetQuery::Validators))
            .await?;
        match decode_reply(&reply)
            .map_err(|e| sdk::Error::module(sdk::refusal::UNEXPECTED_REPLY, e))?
        {
            ValsetReply::Validators(members) => Ok(members),
        }
    }
}

/// The duckfs surface Runs speaks: the three read queries it issues (`Stat`,
/// `Read`, `Refs`), the two writes it emits (`Commit`, `CompareExchangeRetention`),
/// and the path grammar its validators enforce. The `files` module owns the
/// canonical codec; the integration suite drives the REAL `files::Files` and
/// decodes these bytes with `files::decode_msg`, so the mirror stays pinned.
pub mod files {
    use super::*;
    use std::collections::BTreeMap;

    /// a sha256-derived object id rendered as 64-char lowercase hex on the wire.
    pub type DigestHex = String;

    pub const MAX_NAME_BYTES: usize = 255;
    pub const MAX_PATH_BYTES: usize = 4096;
    pub const MAX_DEPTH: usize = 128;

    pub mod paths {
        use super::{MAX_DEPTH, MAX_NAME_BYTES, MAX_PATH_BYTES};
        use unicode_normalization::UnicodeNormalization;

        /// validate a consensus path and split it into its segments. paths are
        /// strict consensus data: utf-8, NFC-normalized, absolute,
        /// `/`-separated, with no empty / `.` / `..` segments and no NUL bytes.
        /// this only ever rejects — it never rewrites. a bare `/` (root) yields
        /// an empty segment list.
        pub fn canonical(path: &str) -> Result<Vec<String>, String> {
            if !path.starts_with('/') {
                return Err("path must be absolute (start with '/')".to_string());
            }
            if path.chars().nfc().collect::<String>() != path {
                return Err("path is not NFC-normalized".to_string());
            }
            if path.contains('\0') {
                return Err("path must not contain a NUL byte".to_string());
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
                    return Err("path contains an empty or dot segment".to_string());
                }
                if segment.len() > MAX_NAME_BYTES {
                    return Err(format!(
                        "segment name exceeds the {MAX_NAME_BYTES}-byte limit"
                    ));
                }
                segments.push(segment.to_string());
            }
            if segments.len() > MAX_DEPTH {
                return Err(format!("path exceeds the maximum depth of {MAX_DEPTH}"));
            }
            Ok(segments)
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum FilesMsg {
        /// atomic multi-path commit. `base_snapshot: None` means the empty tree
        /// (first commit). per-path CAS: every changed path must be identical
        /// between base and the live head or the whole commit rejects.
        Commit {
            base_snapshot: Option<DigestHex>,
            message: String,
            changes: Vec<Change>,
        },
        /// atomically replace a module-owned retention reference. the
        /// authenticated module origin supplies the namespace.
        CompareExchangeRetention {
            key: String,
            expected: Option<RetentionReference>,
            replacement: Option<RetentionReference>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum Change {
        Put {
            path: String,
            exec: bool,
            meta: BTreeMap<String, String>,
            content: Content,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum Content {
        /// small files ride inside the commit op; the module chunks + hashes.
        Inline { b64: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum FilesQuery {
        Stat {
            path: String,
            snapshot: Option<DigestHex>,
        },
        Read {
            path: String,
            snapshot: Option<DigestHex>,
            offset: u64,
            len: u64,
        },
        Refs {},
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum EntryKindWire {
        File,
        Dir,
        Symlink,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct EntryInfo {
        pub path: String,
        pub kind: EntryKindWire,
        pub size: u64,
        pub exec: bool,
        pub object: DigestHex,
        pub meta: BTreeMap<String, String>,
    }

    /// a module-owned immutable snapshot reference. replacements compare the
    /// whole previous value and advance its nonzero revision.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct RetentionReference {
        pub snapshot: DigestHex,
        pub revision: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    pub struct RefsInfo {
        pub head: Option<DigestHex>,
        pub pins: BTreeMap<String, DigestHex>,
        pub window_len: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum FilesReply {
        Stat(Option<EntryInfo>),
        Read { b64: String, eof: bool },
        Refs(RefsInfo),
    }

    pub fn encode_msg(value: &FilesMsg) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_msg(bytes: &[u8]) -> Result<FilesMsg, String> {
        super::decode(bytes)
    }
    pub fn encode_query(value: &FilesQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<FilesQuery, String> {
        super::decode(bytes)
    }
    pub fn encode_reply(value: &FilesReply) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<FilesReply, String> {
        super::decode(bytes)
    }
}

/// The saga read Runs performs: the committed lease on the saga a run's
/// dispatch names ([`SagaQuery::Get`]), which answers the session's
/// execution-lease check and the delivery path's executing-node attribution.
/// Runs never writes to saga — the dispatch module triggers on its behalf.
pub mod saga {
    use super::*;

    pub type SagaId = String;

    /// the canonical, serializable mirror of `sdk::Origin`, recorded on every
    /// saga at trigger time.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum SagaOrigin {
        External(Vec<u8>),
        Module(String),
        System,
    }

    /// where a saga is in its (deterministic) lifecycle.
    #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum SagaStatus {
        Pending,
        Done,
        Failed,
        TimedOut,
        Cancelled,
    }

    impl SagaStatus {
        /// true for every state a saga can never leave.
        pub fn is_terminal(&self) -> bool {
            !matches!(self, SagaStatus::Pending)
        }
    }

    /// a saga's observable state — the full read projection. COMPLETE, because
    /// the producing type is `deny_unknown_fields`: a partial mirror would
    /// refuse to decode a real reply.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    pub struct SagaView {
        pub origin: SagaOrigin,
        pub reply_to: Option<String>,
        pub reply_payload: Vec<u8>,
        pub spec: Vec<u8>,
        pub capability: Option<String>,
        pub status: SagaStatus,
        pub attempt: u32,
        pub max_attempts: u32,
        pub assignee: Option<Vec<u8>>,
        pub pinned_assignee: Option<Vec<u8>>,
        pub lease_views: Option<u64>,
        pub lease_expires_at: Option<u64>,
        pub deadline: Option<u64>,
        pub result: Option<Vec<u8>>,
        pub error: Option<String>,
        pub created_at: u64,
        pub updated_at: u64,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum SagaQuery {
        Get { saga_id: SagaId },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    pub enum SagaReply {
        Saga(Option<SagaView>),
    }

    pub fn encode_query(value: &SagaQuery) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_query(bytes: &[u8]) -> Result<SagaQuery, String> {
        super::decode(bytes)
    }
    pub fn encode_reply(value: &SagaReply) -> Vec<u8> {
        super::encode(value)
    }
    pub fn decode_reply(bytes: &[u8]) -> Result<SagaReply, String> {
        super::decode(bytes)
    }
}

/// The producer-side pins for the two mirrors added when Runs stopped linking
/// the `files` and `saga` module crates. Both stay DEV dependencies precisely
/// so these can drive the real codecs: a mirror that drifts in a field name,
/// a variant tag, or a refusal sentence fails here.
#[cfg(test)]
mod mirror_conformance {
    use std::collections::BTreeMap;

    #[test]
    fn duckfs_path_canonicalization_matches_the_files_module() {
        // ok shapes, then one case per refusal the mirror can emit.
        let cases = [
            "/",
            "/shared",
            "/shared/skills/review",
            "/home/acct:7/notes/todo.md",
            "shared/relative",
            "/shared//double",
            "/shared/./dot",
            "/shared/../parent",
            "/shared/e\u{301}combiné",
            "/shared/nul\0byte",
        ];
        for path in cases {
            assert_eq!(
                super::files::paths::canonical(path),
                ::files::paths::canonical(path),
                "canonical({path:?}) diverged from the files module"
            );
        }
        // the byte/name/depth caps, checked at their boundaries.
        let long_name = format!("/shared/{}", "a".repeat(256));
        let deep = format!("/{}", vec!["d"; 129].join("/"));
        let long_path = format!("/shared/{}", "a".repeat(200) + "/").repeat(64);
        for path in [long_name, deep, long_path] {
            assert_eq!(
                super::files::paths::canonical(&path),
                ::files::paths::canonical(&path),
                "canonical({path:?}) diverged from the files module"
            );
        }
    }

    #[test]
    fn the_files_mirror_encodes_the_bytes_the_files_module_decodes() {
        let reference = super::files::RetentionReference {
            snapshot: "ab".repeat(32),
            revision: 3,
        };
        let commit = super::files::FilesMsg::Commit {
            base_snapshot: Some("cd".repeat(32)),
            message: "agent duckfs.write_text".into(),
            changes: vec![super::files::Change::Put {
                path: "/shared/notes/a.md".into(),
                exec: false,
                meta: BTreeMap::from([("k".to_string(), "v".to_string())]),
                content: super::files::Content::Inline { b64: "aGk=".into() },
            }],
        };
        let cas = super::files::FilesMsg::CompareExchangeRetention {
            key: "conv/resident".into(),
            expected: None,
            replacement: Some(reference.clone()),
        };
        for msg in [commit, cas] {
            let bytes = super::files::encode_msg(&msg);
            ::files::decode_msg(&bytes).expect("the files module decodes the mirror's bytes");
        }

        for query in [
            super::files::FilesQuery::Stat {
                path: "/shared/notes/a.md".into(),
                snapshot: Some("ef".repeat(32)),
            },
            super::files::FilesQuery::Read {
                path: "/shared/notes/a.md".into(),
                snapshot: None,
                offset: 0,
                len: 1024,
            },
            super::files::FilesQuery::Refs {},
        ] {
            let bytes = super::files::encode_query(&query);
            ::files::decode_query(&bytes).expect("the files module decodes the mirror's query");
        }
    }

    #[test]
    fn the_files_mirror_decodes_what_the_files_module_replies() {
        let entry = ::files::EntryInfo {
            path: "/shared/notes/a.md".into(),
            kind: ::files::EntryKindWire::File,
            size: 7,
            exec: false,
            object: "00".repeat(32),
            meta: BTreeMap::from([("k".to_string(), "v".to_string())]),
        };
        let replies = [
            ::files::FilesReply::Stat(Some(entry.clone())),
            ::files::FilesReply::Read {
                b64: "aGk=".into(),
                eof: true,
            },
            ::files::FilesReply::Refs(::files::RefsInfo {
                head: Some("ab".repeat(32)),
                pins: BTreeMap::from([("p".to_string(), "cd".repeat(32))]),
                window_len: 4,
            }),
        ];
        for reply in replies {
            let bytes = ::files::encode_reply(&reply);
            let mirrored = super::files::decode_reply(&bytes)
                .expect("the mirror decodes the files module's reply");
            // round-trips back to the identical bytes, so nothing is dropped.
            assert_eq!(super::files::encode_reply(&mirrored), bytes);
        }

        let retention = ::files::RetentionReference {
            snapshot: "ab".repeat(32),
            revision: 3,
        };
        let bytes = sdk::wire::encode(&retention);
        let mirrored: super::files::RetentionReference = sdk::wire::decode(&bytes).unwrap();
        assert_eq!(sdk::wire::encode(&mirrored), bytes);
    }

    #[test]
    fn the_saga_mirror_round_trips_against_the_saga_module() {
        let query = super::saga::SagaQuery::Get {
            saga_id: "dispatch\u{1f}runs\u{1f}run-1".into(),
        };
        let bytes = super::saga::encode_query(&query);
        assert_eq!(
            ::saga::decode_query(&bytes).expect("the saga module decodes the mirror's query"),
            ::saga::SagaQuery::Get {
                saga_id: "dispatch\u{1f}runs\u{1f}run-1".into(),
            }
        );

        let view = ::saga::SagaView {
            origin: ::saga::SagaOrigin::Module("dispatch".into()),
            reply_to: Some("dispatch".into()),
            reply_payload: vec![1, 2, 3],
            spec: vec![4, 5],
            capability: Some("model-1".into()),
            status: ::saga::SagaStatus::Done,
            attempt: 2,
            max_attempts: 3,
            assignee: Some(vec![0xaa, 0xbb]),
            pinned_assignee: None,
            lease_views: Some(8),
            lease_expires_at: Some(64),
            deadline: Some(128),
            result: Some(vec![6]),
            error: None,
            created_at: 10,
            updated_at: 12,
        };
        let bytes = ::saga::encode_reply(&::saga::SagaReply::Saga(Some(view)));
        let super::saga::SagaReply::Saga(Some(mirrored)) =
            super::saga::decode_reply(&bytes).expect("the mirror decodes the saga module's reply")
        else {
            panic!("expected a saga view");
        };
        // every field survives: `SagaView` is `deny_unknown_fields` on both
        // sides, so a partial mirror would have failed the decode above, and
        // re-encoding proves nothing was defaulted away.
        assert!(mirrored.status.is_terminal());
        assert_eq!(mirrored.assignee.as_deref(), Some(&[0xaa, 0xbb][..]));
        assert_eq!(mirrored.attempt, 2);
        assert_eq!(
            super::saga::encode_reply(&super::saga::SagaReply::Saga(Some(mirrored))),
            bytes
        );
    }
}
