use super::Ev;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A run's place in a chat thread.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatOrigin {
    pub thread: String,
    /// A chat turn (the person's message, answered without a contract), or a task started from chat.
    pub turn: bool,
    /// For a task: the chat turn that started it.
    #[serde(default)]
    pub from_run: Option<String>,
    /// For a task: the conversation that led to it, carried into its contract.
    #[serde(default)]
    pub conversation: Vec<Value>,
    /// A turn Magnus started itself (behavior spec, cold start and a newly
    /// connected app): its text is the appliance's instruction, never the
    /// person's message. Absent in logs from before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opener: Option<Box<Opener>>,
}

/// Why Magnus opened a conversation itself: the person's cold start (no app), or their first connection of `app`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Opener {
    /// The app's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The app as the person sees it named, when it was connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_title: Option<String>,
}

/// The project a task started in, as it stood then: the person's
/// instructions for everything in it and its reference files. Fixed at the
/// start, so the task's context only grows (C1, K5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProjectContext {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub instructions: Option<String>,
    /// Library file ids, read on demand with `project_read` (M2).
    #[serde(default)]
    pub files: Vec<String>,
}

/// A run's pinned configuration (K9), recorded at its start.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunConfig {
    pub adapter: String,
    pub model: String,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub budget: crate::config::Budget,
    pub verifier: bool,
    pub instructions: String,
    pub harness: String,
    /// The playbook version the run started with (I3).
    #[serde(default)]
    pub playbook: u64,
    /// Removable modules on or off for this run, pinned at its start (K9).
    #[serde(default)]
    pub modules: std::collections::BTreeMap<String, bool>,
    /// Where a lost run escalates to (`adapter/model`), if anywhere.
    #[serde(default)]
    pub escalate_to: Option<String>,
    #[serde(default)]
    pub lost: crate::run::lost::Thresholds,
    /// The verifier's model (`adapter/model`); the run's own when unset.
    #[serde(default)]
    pub verifier_model: Option<String>,
    /// Per-run reasoning effort (`off`, `low`, `medium`, `high`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Set when the run reads untrusted output under quarantine (an
    /// unattended schedule whose envelope reaches someone else): the schema
    /// its untrusted reads are cut down to. Pinned at its start, so it holds
    /// from its first step and through a restart (`quarantine_runtime`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<serde_json::Value>,
}

impl RunConfig {
    /// A removable module's switch for this run. Runs from before modules
    /// were pinned had none of them, except the verifier's flag.
    pub fn module(&self, name: &str) -> bool {
        self.modules.get(name).copied().unwrap_or_else(|| name == "verifier" && self.verifier)
    }
}

/// A task started as one trial of an experiment case (V2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExperimentTag {
    pub id: String,
    pub case: String,
    pub trial: u32,
}

/// How to rebuild exactly what one model call was sent (K5): the run's
/// events up to `through_seq`, with these instructions and tools.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContextRef {
    pub through_seq: i64,
    pub instructions: String,
    pub tools_hash: String,
    /// The version of the person's relationship notes the call carried in
    /// its system context (`relationship.rs`); none when they have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<u32>,
    /// Whether the call carried anything of its person's (notes, recent
    /// messages, reactions, commitments): what the run then writes for
    /// others is theirs (#258, #301).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub personal: bool,
    /// The character the reply spoke as, so alpha feedback traces to it
    /// (#269). None on calls logged before it: they spoke with it too,
    /// but only inside the hashed instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
}

impl ContextRef {
    /// Whether the call carried anything of its person's. A call logged
    /// before `personal` was recorded says so by the notes version it
    /// carried (#304).
    pub fn carried_person(&self) -> bool {
        self.personal || self.relationship.is_some()
    }
}

impl Ev {
    /// The version a document was written as (0 for any other event).
    pub fn bundle_version(&self) -> u32 {
        let Ev::BundleWritten(crate::events::BundleWrittenPayload { version, .. }) = self else { return 0 };
        *version
    }

    pub fn run(&self) -> Option<&str> {
        use super::payloads::*;
        use Ev::*;
        match self {
            TaskCreated { run, .. }
            | ModelCall { run, .. }
            | ModelReply(ModelReplyPayload { run, .. })
            | ModelError(ModelErrorPayload { run, .. })
            | Notice { run, .. }
            | Handback { run, .. }
            | Intent(IntentPayload { run, .. })
            | Decision(DecisionPayload { run, .. })
            | ToolResult { run, .. }
            | Contract(ContractPayload { run, .. })
            | WorkStarted { run }
            | ContractFrozen { run, .. }
            | Plan { run, .. }
            | Playbook(PlaybookPayload { run, .. })
            | Graded(GradedPayload { run, .. })
            | Verification(VerificationPayload { run, .. })
            | ContextReset(ContextResetPayload { run, .. })
            | Escalated { run, .. }
            | VerifierReply(VerifierReplyPayload { run, .. })
            | ReviewerReply(ReviewerReplyPayload { run, .. })
            | Suspended { run, .. }
            | Resumed { run }
            | Completed { run, .. }
            | Failed { run, .. }
            | Cancelled { run, .. }
            | ApprovalUsed { run, .. }
            | GrantUsed(GrantUsedPayload { run, .. })
            | Wait { run, .. }
            | BudgetExtended { run, .. }
            | Memory(MemoryPayload { run, .. })
            | MemoryPass(MemoryPassPayload { run, .. })
            | SpokenWritten(SpokenWrittenPayload { run, .. })
            | KnowledgeExtraction(KnowledgeExtractionPayload { run, .. })
            | DraftScoped(DraftScopedPayload { run, .. })
            | ReviewWoken { run, .. }
            | Output(OutputPayload { run, .. }) => Some(run),
            Item { run, .. } | ScheduleEnded(ScheduleEndedPayload { run, .. }) | Tools(crate::tool_gate::Change::Screened { run, .. }) => run.as_deref(),
            Evidence(c) => Some(c.run()),
            _ => None,
        }
    }
}
