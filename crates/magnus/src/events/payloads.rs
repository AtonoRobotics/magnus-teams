//! Event payload structs extracted from `events.rs` to keep every source file under 800 lines.
//! Wire format is identical: `#[serde(tag = "kind", rename_all = "snake_case")]` flattens each struct beside `kind`.

use crate::tools::Label;
use magnus_adapter_proto::{Block, ErrorClass, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConsoleSettingsPayload {
    pub idle_minutes: i64,
    pub session_hours: i64,
    /// The subnets it takes connections from, as an admin set them under Operations › Network (None: the config's).
    #[serde(default)]
    pub allow: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AdminActionPayload {
    pub admin: String,
    #[serde(default)]
    pub name: String,
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub before: Option<Value>,
    #[serde(default)]
    pub after: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContextResetPayload {
    pub run: String,
    pub from: u32,
    pub to: u32,
    pub signal: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerifierReplyPayload {
    pub run: String,
    pub purpose: String,
    pub model: String,
    #[serde(default)]
    pub blocks: Vec<magnus_adapter_proto::Block>,
    pub usage: magnus_adapter_proto::Usage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReviewerReplyPayload {
    pub run: String,
    pub purpose: String,
    pub model: String,
    #[serde(default)]
    pub blocks: Vec<magnus_adapter_proto::Block>,
    pub usage: magnus_adapter_proto::Usage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DraftScopedPayload {
    pub run: String,
    pub person: String,
    pub draft: String,
    pub threads: Vec<String>,
    #[serde(default)]
    pub participants: Vec<String>,
    pub usage: magnus_adapter_proto::Usage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConnectorSettingsPayload {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub access_groups: Option<Vec<String>>,
    #[serde(default)]
    pub blocked_groups: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RightsFromProviderPayload {
    pub person: String,
    #[serde(default)]
    pub groups: Option<Vec<String>>,
    #[serde(default)]
    pub roles: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersonDataReadPayload {
    pub person: String,
    pub what: String,
    /// One task, conversation, file or memory, when one was opened.
    #[serde(default)]
    pub item: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReactedPayload {
    pub person: String,
    pub thread: String,
    pub entry: String,
    #[serde(default)]
    pub reaction: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisagreementRecordedPayload {
    pub id: String,
    pub person: String,
    pub run: String,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub decision: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DisagreementOutcomePayload {
    pub id: String,
    pub person: String,
    pub run: String,
    #[serde(default)]
    pub outcome: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ThreadBranchedPayload {
    pub person: String,
    pub thread: String,
    pub from: String,
    pub upto: String,
    #[serde(default)]
    pub entries: Vec<Value>,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleEndedPayload {
    pub id: String,
    /// Why: the kernel's words, or what the run that ended it wrote (content, removed with that run).
    #[serde(default)]
    pub reason: String,
    /// The run that ended it, when one did.
    #[serde(default)]
    pub run: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SchedulePausedPayload {
    pub id: String,
    pub reason: String,
    /// The task whose run waits on a person: the schedule resumes by
    /// itself once it no longer does. None: paused for failures,
    /// until its person resumes it.
    #[serde(default)]
    pub task: Option<String>,
    /// An admin's suspension: only an admin lifts it.
    #[serde(default)]
    pub admin: bool,
    /// How it was done: "button" (the app), "conversation" (chat), or
    /// "console" (the admin UI). None: automatic, or logged before #267.
    #[serde(default)]
    pub source: Option<String>,
    /// The chat run that did it, when by conversation.
    #[serde(default)]
    pub turn: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleChangedPayload {
    pub id: String,
    pub every_minutes: Option<u32>,
    pub daily_at: Option<String>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    pub max_runs: Option<u32>,
    /// How it was done: "button", "conversation" or "console" (#267).
    #[serde(default)]
    pub source: Option<String>,
    /// The chat run that did it, when by conversation.
    #[serde(default)]
    pub turn: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleResumedPayload {
    pub id: String,
    /// How it was done: "button", "conversation" or "console" (#267).
    #[serde(default)]
    pub source: Option<String>,
    /// The chat run that did it, when by conversation.
    #[serde(default)]
    pub turn: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CardDeclinedPayload {
    pub card: String,
    #[serde(default)]
    pub person: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleRemovedPayload {
    pub id: String,
    /// How it was done: "button", "conversation" or "console" (#267).
    #[serde(default)]
    pub source: Option<String>,
    /// The chat run that did it, when by conversation.
    #[serde(default)]
    pub turn: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrantSetPayload {
    pub id: String,
    pub person: String,
    /// The category of work, in words (content).
    #[serde(default)]
    pub title: String,
    /// The apps it covers, by name.
    pub apps: Vec<String>,
    /// Exclusions the person lifted: `send`, `irreversible`.
    #[serde(default)]
    pub allow: Vec<String>,
    /// `day`, `week` or `month`: a rolling 1, 7 or 30 days.
    pub period: String,
    /// Model cost its tasks may spend in a period.
    pub max_usd: f64,
    /// The card it was set from, if any.
    #[serde(default)]
    pub card: Option<String>,
    pub cedar: String,
    /// The taint of the chat turn whose words its title is, when they
    /// were not the person's own.
    #[serde(default)]
    pub label: Option<Label>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrantUsedPayload {
    pub grant: String,
    pub task: String,
    pub run: String,
    pub call_id: String,
    pub tool: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitmentRecordedPayload {
    pub id: String,
    pub person: String,
    pub who: String,
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub by: Option<String>,
    /// `{kind: conversation, thread, task}` or `{kind: app, app, tool, task, call}`.
    #[serde(default)]
    pub source: Value,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub label: Label,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitmentChangedPayload {
    pub id: String,
    pub person: String,
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolPinnedPayload {
    pub tool: String,
    pub hash: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default)]
    pub approved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolChangedPayload {
    pub tool: String,
    pub hash: String,
    pub description: String,
    pub input_schema: Value,
    pub item: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileUploadedPayload {
    pub id: String,
    pub person: String,
    #[serde(default)]
    pub name: String,
    pub media_type: String,
    #[serde(default)]
    pub data_b64: String,
    /// Where it was added, when that was a chat thread.
    #[serde(default)]
    pub thread: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectSetPayload {
    pub id: String,
    pub person: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub instructions: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelReplyPayload {
    pub run: String,
    pub step: u32,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub state: Option<Value>,
    pub usage: Usage,
    pub served_model: String,
    pub stop: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelErrorPayload {
    pub run: String,
    pub step: u32,
    pub attempt: u32,
    pub class: ErrorClass,
    pub message: String,
    /// What the provider charged before the call failed (it counts
    /// against the budget like a reply's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IntentPayload {
    pub run: String,
    pub call_id: String,
    pub step_id: String,
    pub key: String,
    pub tool: String,
    #[serde(default)]
    pub input: Value,
    pub issuer: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DecisionPayload {
    pub run: String,
    pub call_id: String,
    pub tool: String,
    pub outcome: String,
    pub stage: String,
    pub reason: String,
    #[serde(default)]
    pub rule: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContractPayload {
    pub run: String,
    pub version: u32,
    #[serde(default)]
    pub contract: Value,
    /// A person's approval of a change to a frozen contract (retired, rev 147).
    #[serde(default)]
    pub approved_by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerificationPayload {
    pub run: String,
    #[serde(default)]
    pub criterion: String,
    pub passed: bool,
    #[serde(default)]
    pub evidence: Value,
    pub by: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WatchedSeenPayload {
    pub person: String,
    pub app: String,
    pub tool: String,
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub full: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApprovalPayload {
    pub id: String,
    pub item: String,
    pub person: String,
    pub task: String,
    pub tool: String,
    #[serde(default)]
    pub input_hash: Option<String>,
    pub scope: String,
    #[serde(default)]
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Kept here, as the item goes with its task: the one risk class it covers and (rev 140) the destinations it covers. Absent before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destinations: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OutputPayload {
    pub id: String,
    pub run: String,
    #[serde(default)]
    pub name: String,
    pub media_type: String,
    #[serde(default)]
    pub data_b64: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AdapterVerifiedPayload {
    pub adapter_id: String,
    pub adapter: String,
    pub version: String,
    pub model: String,
    pub passed: bool,
    pub report: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlaybookPayload {
    pub item: String,
    pub op: String,
    pub person: String,
    pub run: String,
    #[serde(default)]
    pub text: Option<String>,
    /// The failure the item addresses.
    #[serde(default)]
    pub addresses: Option<String>,
    pub label: Label,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerdictPayload {
    pub task: String,
    pub person: String,
    pub verdict: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GradedPayload {
    pub run: String,
    pub grader: String,
    pub passed: bool,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryPayload {
    pub id: String,
    pub person: String,
    pub run: String,
    #[serde(default)]
    pub fact: String,
    #[serde(default)]
    pub topic: Option<String>,
    /// The writing run's taint; reading the entry back carries it (M1).
    pub label: Label,
    #[serde(default)]
    pub sources: Vec<String>,
    /// Who filed it: the run's model (None) or the memory pass.
    #[serde(default)]
    pub by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryPassPayload {
    pub run: String,
    pub model: String,
    #[serde(default)]
    pub blocks: Vec<magnus_adapter_proto::Block>,
    pub usage: magnus_adapter_proto::Usage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BundleWrittenPayload {
    pub path: String,
    pub version: u32,
    pub doc_kind: String,
    /// `org`, or `user` for a person's own document under `/users/<owner>/`.
    pub scope: String,
    #[serde(default)]
    pub owner_user: Option<String>,
    /// The OKF actor that wrote it (`magnus/<version>`, `human:<id>`).
    pub actor: String,
    #[serde(default)]
    pub run: Option<String>,
    /// The taint of what it was written from (M1): read back with it.
    pub label: Label,
    /// The items it lists in `sources` (their `resource`s): who may see
    /// it follows from who may read these (K2).
    #[serde(default)]
    pub sources: Vec<String>,
    /// The person whose run read its sources (K2): an agent's version
    /// names its run's, a version put back the one it was written
    /// with; an admin's edit names none and keeps the document's.
    #[serde(default)]
    pub writer: Option<String>,
    #[serde(default)]
    pub frontmatter: Value,
    #[serde(default)]
    pub body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BundleSharedPayload {
    pub path: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub everyone: bool,
    pub by: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KnowledgeExtractionPayload {
    pub run: String,
    pub model: String,
    #[serde(default)]
    pub reply: String,
    pub usage: magnus_adapter_proto::Usage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpokenWrittenPayload {
    pub run: String,
    pub model: String,
    #[serde(default)]
    pub script: String,
    pub usage: magnus_adapter_proto::Usage,
    /// The character the script was rewritten as (#269).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrefetchRecordedPayload {
    pub run: String,
    pub person: String,
    #[serde(default)]
    pub pointers: String,
    #[serde(default)]
    pub label: Label,
}
