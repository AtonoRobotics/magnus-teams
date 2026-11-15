//! The projection of the event log: tasks, runs, inbox, approvals, policy.
//! It holds nothing the log does not; `State::apply` is the only writer, and a
//! boot replays every event through it.

use crate::events::{Ev, RunConfig};
use crate::log::Event;
use crate::tools::Label;
use chrono::{DateTime, Utc};
use magnus_adapter_proto::Block;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub mod apps;
pub mod arrangements;
pub mod library;
pub mod org;
pub mod server_ops;
pub mod thread_extras;
pub mod vault;
pub mod watch;
pub use apps::ConnectorSetting;
pub use arrangements::{Answer, CardState, Commitment, CommitmentStep, Grant, GrantAction, Schedule};
pub use library::{BundleDoc, KnowledgeEntry, LibFile, MemoryEntry, PlaybookItem, Project};
pub use org::PolicyVersion;
pub use server_ops::{ScrubPending, Update};
pub use thread_extras::{Branch, Disagreement};
pub use vault::Revoked;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    pub fn terminal(self) -> bool {
        matches!(self, Status::Completed | Status::Failed | Status::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct Reply {
    pub step: u32,
    pub seq: i64,
    pub blocks: Vec<Block>,
    pub state: Option<Value>,
}

impl Reply {
    /// The reply's text, its blocks joined.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    pub fn calls(&self) -> Vec<(String, String, Value)> {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::ToolCall { id, name, input } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ResultRec {
    pub seq: i64,
    pub text: String,
    pub is_error: bool,
    pub status: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct Intent {
    pub call_id: String,
    pub tool: String,
    /// When it was recorded.
    pub at: DateTime<Utc>,
    pub input: Value,
    pub issuer: String,
    /// pending until a result for this key is recorded
    pub result: Option<ResultRec>,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub id: String,
    pub task: String,
    pub person: String,
    pub config: RunConfig,
    pub status: Status,
    pub reason: Option<String>,
    pub replies: Vec<Reply>,
    pub results: HashMap<String, ResultRec>,
    pub notices: Vec<(u32, i64, String)>,
    pub frames: Vec<(u32, String)>,
    pub contract: Option<Value>,
    pub contract_version: u32,
    /// The agent has started the work (its first action that changes anything).
    pub work_started: bool,
    /// The version frozen when work started; later changes need approval.
    pub contract_frozen: Option<u32>,
    /// The run's taint when the contract in force was written: its checks
    /// were written then, so the Rule of Two judges them by it. A version a
    /// person approved keeps the taint the run had when they approved it.
    pub contract_taint: (bool, bool),
    /// Who approved the contract in force, when it was a change.
    pub contract_approved_by: Option<String>,
    pub verified: BTreeMap<String, (bool, Value, String)>,
    /// How often the reviewer module judged for this run (a send, or the
    /// completion criteria): the dev registry's acted count.
    pub reviewed: u32,
    pub untrusted: bool,
    pub private: bool,
    /// Something from outside the workspace reached it through a tool.
    pub outside: bool,
    /// Apps whose private content this run has read.
    pub private_from: std::collections::BTreeSet<String>,
    pub model_calls: u32,
    /// Of `model_calls`, those the kernel's own modules made (verifier,
    /// reviewer, memory, extraction, read aloud, drafting): see `run::loop_calls`.
    pub aux_calls: u32,
    pub actions: u32,
    pub usd: f64,
    /// What its sub-agents spent, against its budget (`Run::spent_usd`).
    pub sub_usd: f64,
    pub intents: HashMap<String, Intent>,
    pub wait_until: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    /// Delivered files: (id, name, media type, size in bytes).
    pub outputs: Vec<(String, String, String, usize)>,
    pub last_seq: i64,
    pub experiment: Option<crate::events::ExperimentTag>,
    pub plan: Option<Value>,
    pub plan_version: u32,
    pub outcome_view: Option<Value>,
    pub spoken: Option<String>,
    /// Graders' verdicts: (grader, passed, score, details).
    pub grades: Vec<(String, bool, Option<f64>, Value)>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// When it first ended or stopped (for a person, or to wait): how long a
    /// request took, whatever was done with it afterwards.
    pub first_stop: Option<DateTime<Utc>>,
    /// Time of the latest event for this run, to spot stalled runs.
    pub last_at: DateTime<Utc>,
    /// Boundary decisions, in order (E4).
    pub decisions: Vec<DecisionRec>,
    /// Every reason this run was ever suspended for.
    pub suspensions: Vec<String>,
    /// Provider-reported tokens over all replies.
    pub tokens: Tokens,
    pub model_errors: u32,
    /// Contract checks that failed, over every completion attempt.
    pub checks_failed: u32,
    /// The inbox item a suspended run waits on.
    pub waiting_item: Option<String>,
    /// Replies the lost-reset module took out of the model's context.
    pub cuts: Vec<Cut>,
    /// The reply lost-detection reads from: after the latest reset or
    /// escalation, what came before is no longer the run's current attempt.
    pub lost_since: u32,
    /// The stronger model the escalation module moved this run to; its
    /// pinned configuration stays what it started with.
    pub escalated: Option<(String, String)>,
    /// Its person resumed it after its grant's budget for the period was
    /// spent: it goes on under its own budget alone (#280).
    pub past_grant: bool,
    /// A model call of the run carried its person's relationship context
    /// (`ContextRef::personal`), so what it writes for others is theirs.
    pub personal: bool,
}

#[derive(Clone, Debug)]
pub struct Cut {
    pub from: u32,
    pub to: u32,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct DecisionRec {
    pub seq: i64,
    pub call_id: String,
    pub tool: String,
    pub stage: String,
    pub outcome: String,
    pub rule: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Run {
    /// The adapter and model the run's next call goes to.
    pub fn model(&self) -> (&str, &str) {
        match &self.escalated {
            Some((a, m)) => (a, m),
            None => (&self.config.adapter, &self.config.model),
        }
    }

    /// Tool calls from the latest reply that have no result yet, in order.
    /// Every tool call with its result, in the order the model made them.
    pub fn results_in_order(&self) -> Vec<Value> {
        self.replies
            .iter()
            .flat_map(|r| r.calls())
            .filter_map(|(id, name, _)| {
                self.results.get(&id).map(|x| serde_json::json!({ "tool": name, "text": x.text, "is_error": x.is_error, "status": x.status, "seq": x.seq }))
            })
            .collect()
    }

    /// What a chat turn showed below its answer: the components of its
    /// last `show` the kernel accepted.
    pub fn shown(&self) -> Option<Value> {
        self.replies
            .iter()
            .flat_map(|r| r.calls())
            .filter(|(id, name, _)| name == "show" && self.results.get(id).is_some_and(|x| !x.is_error))
            .map(|(_, _, input)| input["components"].clone())
            .last()
    }

    pub fn pending_calls(&self) -> Vec<(usize, String, String, Value)> {
        let Some(r) = self.replies.last() else { return vec![] };
        r.calls().into_iter().enumerate().filter(|(_, (id, _, _))| !self.results.contains_key(id)).map(|(i, (id, n, v))| (i, id, n, v)).collect()
    }
}

#[derive(Clone, Debug, Serialize)]
/// A task's record. What the person asked (its text), the title the model
/// gave it and the scope the person confirmed are not kept here: they are
/// recalled from the content store, for its person, when a request needs
/// them (recall only, step 3a), from the event `seq` names.
pub struct Task {
    pub task_id: String,
    pub run_id: String,
    pub person: String,
    pub created_at: DateTime<Utc>,
    pub chat: Option<crate::events::ChatOrigin>,
    /// The schedule that started it, if any.
    pub schedule: Option<String>,
    /// A proactive review run's task: the loop's own wake, not work (#237).
    pub review: bool,
    /// A scheduled watch poll's task: a quiet read, hidden from the person's lists (#406).
    pub poll: bool,
    /// The event that created it: where its words are recalled from.
    pub seq: i64,
    /// Library files the person gave it.
    pub files: Vec<String>,
    /// The project it started in, as it stood then.
    pub project: Option<crate::events::ProjectContext>,
    /// The person's latest verdict on its outcome.
    pub verdict: Option<Verdict>,
    /// The grant it runs under (#235).
    pub grant: Option<String>,
}

/// A person's verdict on an outcome.
#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub verdict: String,
    pub note: Option<String>,
    pub at: DateTime<Utc>,
}

/// The verdicts a person can give, best first.
pub const VERDICTS: [&str; 4] = ["right", "partly", "wrong", "undone"];

impl Task {
    /// A chat turn: answered in the thread, not listed as a task.
    pub fn is_chat_turn(&self) -> bool {
        self.chat.as_ref().is_some_and(|c| c.turn)
    }

    /// Magnus's own background work (a proactive review, a watch poll):
    /// it reaches the person only through what it produces for them and
    /// its asks (`background_work`), never through its own reviews,
    /// questions or notices. Developers see all of it in the run inspector.
    pub fn background(&self) -> bool {
        self.review || self.poll
    }
}

/// What a background run may put in front of its person: what it produced
/// for them. Anything else about it is its own plumbing (owner, 2026-11-07).
pub const BACKGROUND_OUTPUTS: [&str; 6] = ["observation", "interrupt", "suggestion", "schedule_proposal", "task_proposal", "message"];

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub task: Option<String>,
    pub run: Option<String>,
    pub person: String,
    pub summary: String,
    pub details: Value,
    pub created_at: DateTime<Utc>,
    pub resolution: Option<(String, Option<String>, Option<String>, String)>,
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub id: String,
    pub person: String,
    pub task: String,
    pub tool: String,
    pub input_hash: Option<String>,
    pub scope: String,
    pub until: Option<DateTime<Utc>>,
    pub used: bool,
    pub by: String,
    /// Why the approval was asked for: the rule, or `unknown_outcome`.
    pub reason: Option<String>,
    /// The risk class the action had when it was approved (its item's): an
    /// approval covers only that class.
    pub risk: Option<String>,
    /// Where the approved action went; None for approvals from before.
    pub destinations: Option<Vec<String>>,
}

/// What a scoped drafting step wrote from: the threads (none for a new
/// draft) and their participants' addresses (#178).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftSources {
    pub person: String,
    pub threads: Vec<String>,
    pub participants: Vec<String>,
}

/// An item in the trash: out of every view until it is restored or removed.
#[derive(Clone, Debug)]
pub struct Trashed {
    pub person: String,
    /// Its title, when the state has it; else `title_from` says where it is
    /// recalled from.
    pub title: String,
    pub title_from: Option<TitleFrom>,
    pub at: DateTime<Utc>,
    held: Held,
}

/// Where a trashed item's title is recalled from: the event that holds a
/// task's words, its title (else its text), or its text alone.
#[derive(Clone, Copy, Debug)]
pub enum TitleFrom {
    Title(i64),
    Text(i64),
}

impl TitleFrom {
    pub fn seq(&self) -> i64 {
        match self {
            TitleFrom::Title(s) | TitleFrom::Text(s) => *s,
        }
    }

    /// The title, from the recalled content of `seq()`.
    pub fn title(&self, words: &Value) -> String {
        let text = words["text"].as_str().unwrap_or_default();
        match self {
            TitleFrom::Title(_) => words["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(text).to_string(),
            TitleFrom::Text(_) => text.to_string(),
        }
    }
}

/// What a trashed item took out of the state, to put back if restored. A
/// chat message or answer takes nothing: its thread leaves it out.
#[derive(Clone, Debug)]
enum Held {
    Task(Box<Task>, Option<Box<Run>>, Vec<Item>),
    Thread { id: String, turns: Vec<(Task, Option<Run>)>, branch: Option<Branch> },
    Entry { thread: String },
    File(Box<LibFile>),
    Project(Project),
    Memory(MemoryEntry),
    Knowledge(Box<KnowledgeEntry>),
}

impl Trashed {
    /// A trashed memory's record: its content is recalled, not kept.
    pub fn memory(&self) -> Option<&MemoryEntry> {
        match &self.held {
            Held::Memory(m) => Some(m),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self.held {
            Held::Task(..) => "task",
            Held::Thread { .. } => "conversation",
            Held::Entry { .. } => "message",
            Held::File(_) => "file",
            Held::Project(_) => "project",
            Held::Memory(_) => "memory",
            Held::Knowledge(_) => "knowledge",
        }
    }

    /// The thread a trashed message or answer was in.
    pub fn thread(&self) -> Option<&str> {
        match &self.held {
            Held::Entry { thread } => Some(thread),
            _ => None,
        }
    }
}

/// Why an item cannot go to the trash.
#[derive(Debug, PartialEq)]
pub enum NotTrashable {
    /// No such item of the person's.
    NotFound,
    /// A task still running: cancel it first, so a crash can resume it (C3).
    Running,
}

/// A person erased with content: the removal that took it, their provider
/// subject then (the console's name for them), and whether a backup still
/// holds it.
#[derive(Clone, Debug)]
pub struct Erasure {
    pub removal: i64,
    pub subject: Option<String>,
    pub pending: bool,
}

/// Debug, so a test can look through all of it for what erasing a person
/// left behind (`forget_tests.rs`).
#[derive(Default, Debug)]
pub struct State {
    pub computers: crate::computer::Ledger,
    pub payments: crate::payments::Ledger,
    pub rules: crate::rules::Ledger,
    pub evidence: crate::evidence::Ledger,
    pub sub_agents: crate::sub_agents::Ledger,
    pub outcomes: crate::outcomes::Ledger,
    pub local: crate::local::Ledger,
    pub team: crate::team::Ledger,
    pub account_links: crate::account_links::Ledger,
    pub legal: crate::legal::Ledger,
    pub push: crate::push::Ledger,
    pub halts: crate::halts::Ledger,
    /// Trashed items by id.
    pub trash: BTreeMap<String, Trashed>,
    /// Conversations removed from the trash: their tasks stay, the thread is gone.
    pub removed_threads: HashSet<String>,
    pub tasks: BTreeMap<String, Task>,
    pub runs: HashMap<String, Run>,
    pub items: BTreeMap<String, Item>,
    pub approvals: Vec<Approval>,
    /// The organisation's settings, the admin audit and version notices.
    pub org: org::OrgSettings,
    /// Secrets' names and the kill switch.
    pub vault: vault::Ledger,
    /// Everyone who has ever signed in here. Someone not in it signs in the
    /// first time on the identity provider's own page, which can walk them
    /// through what their account still needs (a new password, two-factor).
    pub signed_in_before: HashSet<String>,
    /// When each person last signed in here (the console's Users list).
    pub last_sign_in: HashMap<String, DateTime<Utc>>,
    /// When each person last opened the app's home: the feed starts
    /// here (#367).
    pub home_last_open: HashMap<String, DateTime<Utc>>,
    /// Watched reads' baselines and poll clocks (#369, #406).
    pub watch: watch::Watch,
    /// When each inbox item was first presented to its person (#367).
    pub items_opened: HashMap<String, DateTime<Utc>>,
    /// When each inbox item was decided, for the feed (#367).
    pub items_resolved_at: HashMap<String, DateTime<Utc>>,
    /// The conversations Magnus opened, by person then task: each starts
    /// once ever, so deleting one does not start it again (review of #274).
    pub openers: std::collections::BTreeSet<(String, String)>,
    /// People turned off at the identity provider or by an admin, until
    /// they sign in again: their apps answer no calls for them.
    pub turned_off: std::collections::BTreeSet<String>,
    /// The server's updates and backups.
    pub server_ops: server_ops::ServerOps,
    /// Each person erased with content, by their id then.
    pub erasures: BTreeMap<String, Erasure>,
    /// The apps the owner added, their settings, credentials and registrations.
    pub apps: apps::Apps,
    /// Groups and roles from the identity provider, per person, as of their
    /// latest sign-in.
    pub provider_rights: HashMap<String, (Option<Vec<String>>, Option<Vec<String>>)>,
    /// The identity provider account (`sub`) each person is bound to.
    pub subjects: HashMap<String, String>,
    /// Roles the owner set in Settings, by person.
    pub roles_set: HashMap<String, Vec<String>>,
    /// Whether people's data is private from the admin (by default it is not).
    pub data_private: bool,
    /// Each person's own settings, by person.
    pub preferences: HashMap<String, Value>,
    /// Hidden entries, reactions, disagreements and branches.
    pub thread_extras: thread_extras::ThreadExtras,
    /// Schedules, grants, commitments and answered cards.
    pub arrangements: arrangements::Arrangements,
    /// Runs a person stopped, as against the system.
    pub stopped_by_person: HashSet<String>,
    /// Drafts written by scoped steps, by draft id: the threads whose
    /// content they may hold and who is entitled to it (#178).
    pub draft_sources: HashMap<String, DraftSources>,
    /// The proactive loop's scheduling, by person (#237): the last review
    /// wake, the day of the last overnight review, and today's delivered
    /// interruptions (day, count).
    pub reviews_last: HashMap<String, DateTime<Utc>>,
    pub reviews_overnight: HashMap<String, String>,
    pub interrupts: HashMap<String, (String, u32)>,
    /// Review runs still going, run to person: one per person at a time.
    pub review_runs: HashMap<String, String>,
    /// One-a-day wake marks (a coming-up schedule, a due commitment).
    pub wake_marks: HashMap<String, String>,
    /// App tool definitions as screened, those held back, and who connected which app through its connect screen (spec rev 140).
    pub tools: crate::tool_gate::Ledger,
    pub spend: crate::spend::Ledger,
    pub catalog: crate::catalog_withdrawal::Ledger,
    pub admin_settings: crate::admin_app::settings::Settings,
    /// Files, projects, knowledge, memory, the playbook and the bundle.
    pub library: library::Library,
    /// Files attached to each task, by task id.
    pub attachments: HashMap<String, Vec<Value>>,
    pub last_seq: i64,
}

/// A text file's content in lower case for library search, up to as much as
/// a page shows of a file; other files are found by name.
pub fn search_text(media_type: &str, data_b64: &str) -> String {
    use base64::Engine;
    let textual = media_type.starts_with("text/") || ["application/json", "application/xml", "application/csv"].contains(&media_type);
    let Some(bytes) = textual.then(|| base64::engine::general_purpose::STANDARD.decode(data_b64).ok()).flatten() else { return String::new() };
    String::from_utf8_lossy(&bytes).chars().take(crate::compose::PREVIEW_CHARS).collect::<String>().to_lowercase()
}

impl State {
    /// The person an event's content belongs to: the person it names, else
    /// the person of its run, item or schedule (a team change: `team::owner`). `runs` holds the runs of
    /// events written in the same batch, before them. The organisation's
    /// own content (knowledge entries, the playbook) belongs to no one
    /// person: None.
    pub fn content_owner(&self, ev: &crate::events::Ev, runs: &HashMap<String, String>) -> Option<String> {
        use crate::events::Ev;
        match ev {
            Ev::KnowledgeAdded { .. } | Ev::Playbook { .. } => return None,
            // An org document is nobody's; a person's own is theirs.
            Ev::BundleWritten { owner_user, .. } => return owner_user.clone(),
            Ev::ItemResolved { id, .. } | Ev::ItemOpened { id } => return self.items.get(id).map(|i| i.person.clone()),
            Ev::Team(c) => return self.team.content_owner(c),
            Ev::ScheduleEnded { id, .. } => return self.arrangements.schedules().get(id).map(|s| s.person.clone()),
            _ => {}
        }
        if let Some(p) = serde_json::to_value(ev).ok().and_then(|v| v["person"].as_str().map(String::from)) {
            return Some(p);
        }
        let run = ev.run()?;
        runs.get(run).cloned().or_else(|| self.runs.get(run).map(|r| r.person.clone()))
    }

    /// The project an item is in: where the person moved it, else where it
    /// began. A task begins in the project it started in; a thread in its
    /// first message's; a delivered file in its task's; a file uploaded in
    /// chat in its thread's.
    pub fn project_of(&self, item: &str) -> Option<String> {
        if let Some(p) = self.library.moved().get(item) {
            return p.clone().filter(|p| self.library.projects().contains_key(p));
        }
        if let Some(t) = self.tasks.get(item) {
            return t.project.as_ref().map(|p| p.id.clone());
        }
        if let Some(f) = self.library.files().get(item) {
            if let Some(task) = f.run.as_ref().and_then(|r| self.runs.get(r)).map(|r| r.task.clone()) {
                return self.project_of(&task);
            }
            return f.thread.as_ref().and_then(|t| self.project_of(t));
        }
        self.tasks
            .values()
            .filter(|t| t.chat.as_ref().is_some_and(|c| c.thread == item && c.turn))
            .min_by_key(|t| t.created_at)
            .and_then(|t| t.project.as_ref().map(|p| p.id.clone()))
    }

    /// A schedule's run that is waiting on a person (an approval, a
    /// question, a review, a stop for its budget or an error): the task and
    /// why. Waiting for a time, an update or the kill switch is not.
    pub fn held_by_person(&self, schedule: &str) -> Option<(String, String)> {
        self.tasks.values().filter(|t| t.schedule.as_deref() == Some(schedule)).find_map(|t| {
            let r = self.runs.get(&t.run_id).filter(|r| r.ended_at.is_none() && r.status == Status::Suspended)?;
            let reason = r.reason.as_deref().unwrap_or("someone");
            (!["timer", "update"].contains(&reason) && !crate::halts::is_kill(reason)).then(|| (t.task_id.clone(), format!("its last run is waiting on you ({reason})")))
        })
    }

    /// A schedule's run that has not ended, and why it is still open.
    pub fn open_run_of(&self, schedule: &str) -> Option<(String, String)> {
        self.tasks.values().filter(|t| t.schedule.as_deref() == Some(schedule)).find_map(|t| {
            let r = self.runs.get(&t.run_id).filter(|r| r.ended_at.is_none())?;
            let why = match (r.status == Status::Suspended, r.reason.as_deref()) {
                (true, Some("timer")) => "its last run is waiting for a time it set".to_string(),
                (true, Some(reason)) => format!("its last run is waiting on someone ({reason})"),
                _ => "its last run is still running".to_string(),
            };
            Some((t.task_id.clone(), why))
        })
    }

    /// The schedule a task was started by, while it is still set.
    pub fn task_schedule(&self, task: &str) -> Option<(String, Schedule)> {
        let id = self.tasks.get(task)?.schedule.clone()?;
        self.arrangements.schedules().get(&id).map(|x| (id.clone(), x.clone()))
    }

    /// How many of a schedule's latest firings in a row failed (since it was
    /// last resumed): runs that ended failed, and firings that started none.
    pub fn failures_in_a_row(&self, schedule: &str) -> u32 {
        let Some(sc) = self.arrangements.schedules().get(schedule) else { return 0 };
        let since = sc.resumed.unwrap_or(sc.created);
        let mut ends: Vec<(DateTime<Utc>, bool)> = sc.failures.iter().filter(|(at, _)| *at >= since).map(|(at, _)| (*at, true)).collect();
        ends.extend(
            self.tasks
                .values()
                .filter(|t| t.schedule.as_deref() == Some(schedule) && t.created_at >= since)
                .filter_map(|t| self.runs.get(&t.run_id))
                .filter(|r| r.ended_at.is_some())
                .map(|r| (r.ended_at.unwrap_or_default(), r.status == Status::Failed)),
        );
        ends.sort_by(|a, b| b.0.cmp(&a.0));
        ends.iter().take_while(|(_, failed)| *failed).count() as u32
    }

    /// A model call's cost, charged to the grant the run acts under, if
    /// any (#280), and to the runs above a sub-agent (`sub_agents::charge`).
    fn charge_grant(&mut self, run: &str, at: DateTime<Utc>, usd: f64) {
        self.charge(run, at, usd);
    }

    /// Take an item out of every view, into the trash.
    fn take(&mut self, item: &str) -> Option<(String, Option<TitleFrom>, Held)> {
        if let Some((_, task)) = item.split_once(':') {
            let t = self.tasks.get(task)?;
            let thread = t.chat.as_ref()?.thread.clone();
            // A message's title is what the person said, recalled when listed.
            let opener = t.chat.as_ref().and_then(|c| c.opener.as_deref()).map(crate::openers::title);
            let (title, from) = match item.starts_with("m:") {
                // Magnus's own opening: its text is the appliance's, not theirs.
                true if opener.is_some() => (opener.unwrap_or_default(), None),
                true => (String::new(), Some(TitleFrom::Text(t.seq))),
                false => (self.runs.get(&t.run_id).and_then(|r| r.summary.clone()).unwrap_or_default(), None),
            };
            return Some((title, from, Held::Entry { thread }));
        }
        if item.starts_with("task_") {
            let t = self.tasks.remove(item)?;
            let r = self.runs.remove(&t.run_id).map(Box::new);
            let ids: Vec<String> = self.items.values().filter(|i| i.task.as_deref() == Some(item)).map(|i| i.id.clone()).collect();
            let items = ids.iter().filter_map(|id| self.items.remove(id)).collect();
            return Some((String::new(), Some(TitleFrom::Title(t.seq)), Held::Task(Box::new(t), r, items)));
        }
        if item.starts_with("thread_") {
            let ids: Vec<String> = self.tasks.values().filter(|t| t.chat.as_ref().is_some_and(|c| c.turn && c.thread == item)).map(|t| t.task_id.clone()).collect();
            let mut turns: Vec<(Task, Option<Run>)> = ids
                .iter()
                .filter_map(|id| self.tasks.remove(id))
                .map(|t| {
                    let r = self.runs.remove(&t.run_id);
                    (t, r)
                })
                .collect();
            turns.sort_by_key(|(t, _)| t.created_at);
            let branch = self.thread_extras.take_branch(item);
            // A conversation's title is its first message, recalled when
            // listed; one Magnus opened is called what the app calls it, not
            // by the appliance's instruction (review of #274).
            let (title, from) = match turns.first() {
                Some((t, _)) => match t.chat.as_ref().and_then(|c| c.opener.as_deref()) {
                    Some(o) => (crate::openers::title(o), None),
                    None => (String::new(), Some(TitleFrom::Text(t.seq))),
                },
                None => (branch.as_ref()?.title.clone(), None),
            };
            return Some((title, from, Held::Thread { id: item.into(), turns, branch }));
        }
        if let Some(f) = self.library.take_file(item) {
            return Some((f.name.clone(), None, Held::File(Box::new(f))));
        }
        if let Some(p) = self.library.take_project(item) {
            return Some((p.name.clone(), None, Held::Project(p)));
        }
        // Its title in the trash is its fact, recalled when listed.
        if let Some(m) = self.library.take_memory(item) {
            return Some((String::new(), None, Held::Memory(m)));
        }
        let k = self.library.take_knowledge(item)?;
        Some((k.title.clone(), None, Held::Knowledge(Box::new(k))))
    }

    /// Which events hold a trashed item's own content. A task's or a
    /// conversation's runs keep what they made that is an item of its own:
    /// files they delivered, memories and playbook items they wrote.
    /// `fact` is a trashed memory's fact, recalled by the caller.
    pub fn holding(&self, item: &str, fact: Option<&str>) -> Option<crate::log::Holding> {
        use crate::log::Holding;
        const OWN: [&str; 3] = ["output", "memory", "playbook"];
        let named = |kinds: Vec<&'static str>, field: &'static str, id: &str| Holding { kinds, field, id: id.into(), ..Default::default() };
        // What belongs to a task but carries no run: its verdict's note and
        // the person's answers to its cards.
        let theirs = |task: &str, items: &mut dyn Iterator<Item = &Item>| -> Vec<(&'static str, &'static str, String)> {
            let mut m = vec![("verdict", "task", task.to_string())];
            m.extend(items.map(|i| ("item_resolved", "id", i.id.clone())));
            m
        };
        let items_of = |task: &str| self.items.values().filter(|i| i.task.as_deref() == Some(task)).cloned().collect::<Vec<_>>();
        Some(match &self.trash.get(item)?.held {
            Held::Task(t, _, items) => Holding { runs: vec![t.run_id.clone()], except: OWN.to_vec(), matches: theirs(&t.task_id, &mut items.iter()), ..Default::default() },
            Held::Thread { id, turns, .. } => {
                let mut matches: Vec<_> = turns.iter().flat_map(|(t, _)| theirs(&t.task_id, &mut items_of(&t.task_id).iter())).collect();
                // The reasons the person gave declining the cards in it.
                let cards = turns.iter().filter_map(|(_, r)| r.as_ref()).flat_map(|r| r.results.values().filter_map(|x| x.data.as_ref()?["card"].as_str().map(String::from)));
                matches.extend(cards.map(|c| ("card_declined", "card", c)));
                Holding { runs: turns.iter().map(|(t, _)| t.run_id.clone()).collect(), except: OWN.to_vec(), matches, ..named(vec!["thread_branched"], "thread", id) }
            }
            Held::Entry { .. } => match item.split_once(':')? {
                ("m", task) => named(vec!["task_created"], "task", task),
                (_, task) => Holding {
                    runs: vec![self.tasks.get(task)?.run_id.clone()],
                    except: [&OWN[..], &["task_created"]].concat(),
                    matches: theirs(task, &mut items_of(task).iter()),
                    ..Default::default()
                },
            },
            Held::File(_) => named(vec!["file_uploaded", "output"], "id", item),
            Held::Project(_) => named(vec!["project_set"], "id", item),
            // A memory and where it was written: the call that wrote it, and
            // the reply that made the call (the run's model, or the memory
            // pass), found by the fact's text in that run.
            Held::Memory(m) => {
                let fact = fact.unwrap_or_default();
                let text = serde_json::to_string(fact).unwrap_or_default();
                let text = text.trim_matches('"').to_string();
                let containing =
                    ["intent", "tool_result", "model_reply", "memory_pass"].into_iter().map(|k| (k, m.run.clone(), text.clone())).filter(|_| !fact.trim().is_empty()).collect();
                Holding { containing, ..named(vec!["memory"], "id", item) }
            }
            Held::Knowledge(_) => named(vec!["knowledge_added"], "id", item),
        })
    }

    /// A trashed item back where it was.
    /// Which erasures a backup still holds, after a scrub's record: each
    /// change is in the audit, as the console names the person (#195).
    fn erasures_pending(&mut self, e: &Event) {
        let pending = self.server_ops.scrub_pending().cloned();
        for (person, er) in self.erasures.iter_mut() {
            let now = pending.as_ref().is_some_and(|p| p.from < er.removal && er.removal <= p.through);
            if now == er.pending {
                continue;
            }
            er.pending = now;
            let (action, after) = match &pending {
                Some(p) if now => ("user_erasure_incomplete", serde_json::json!({ "backups": p.files, "detail": p.detail })),
                _ => ("user_erasure_completed", serde_json::json!({ "backups": "every backup kept is rewritten without their content" })),
            };
            let target = er.subject.clone().unwrap_or_else(|| person.clone());
            self.org.note_audit(e, serde_json::json!({ "kind": "admin_action", "admin": "kernel", "name": "Magnus", "action": action, "target": target, "source": null, "before": null, "after": after }));
        }
    }

    fn put_back(&mut self, held: Held) {
        match held {
            Held::Task(t, r, items) => {
                if let Some(r) = r {
                    self.runs.insert(r.id.clone(), *r);
                }
                for i in items {
                    self.items.insert(i.id.clone(), i);
                }
                self.tasks.insert(t.task_id.clone(), *t);
            }
            Held::Thread { id, turns, branch } => {
                for (t, r) in turns {
                    if let Some(r) = r {
                        self.runs.insert(r.id.clone(), r);
                    }
                    self.tasks.insert(t.task_id.clone(), t);
                }
                if let Some(b) = branch {
                    self.thread_extras.put_branch(id, b);
                }
            }
            Held::Entry { .. } => {}
            Held::File(f) => self.library.put_file(*f),
            Held::Project(p) => self.library.put_project(p),
            Held::Memory(m) => self.library.put_memory(m),
            Held::Knowledge(k) => self.library.put_knowledge(*k),
        }
    }

    /// A person's file, or a delivered file of a task they may see.
    pub fn file_for(&self, person: &str, owner: bool, id: &str) -> Option<&LibFile> {
        self.library.files().get(id).filter(|f| f.person == person || (owner && f.run.is_some()))
    }

    pub fn apply(&mut self, e: &Event) {
        self.last_seq = e.seq;
        if let Some(r) = e.ev.run().and_then(|id| self.runs.get_mut(id)) {
            r.last_seq = e.seq;
            r.last_at = e.at;
        }
        match &e.ev {
            Ev::KernelBoot { .. }
            | Ev::PolicySet { .. }
            | Ev::ModuleSet { .. }
            | Ev::ProviderSet { .. }
            | Ev::DesktopChannelSet { .. }
            | Ev::ConsoleSettings { .. }
            | Ev::PolicyDrafted { .. }
            | Ev::AdminAction { .. }
            | Ev::ModelSet { .. }
            | Ev::AdapterVerified { .. } => self.org.fold(e),
            Ev::SecretSet { .. } | Ev::KillSwitch { .. } | Ev::VaultSealed { .. } | Ev::SecretRevoked { .. } => self.vault.fold(&e.ev, &e.actor, e.at),
            Ev::Computer(c) => self.computers.apply_at(c, e.at),
            Ev::Payment(c) => self.payments.apply(c),
            Ev::Rule(c) => self.rules.apply(c),
            Ev::SubAgent(c) => self.sub_agents.apply(c),
            Ev::LocalComputer(c) => self.local.apply(c),
            Ev::LocalFile(c) => self.local.files.apply(c),
            Ev::Desktop(c) => self.local.desktop.apply(c),
            Ev::Team(c) => self.team.apply_at(c, e.seq),
            Ev::AccountLink(c) => self.account_links.apply(c),
            Ev::Legal(c) => self.legal.apply(c),
            Ev::Push(c) => self.push.apply(c),
            Ev::Halt(c) => self.halts.apply(c, &e.actor, e.at),
            Ev::AdminSetting(c) => self.admin_settings.apply(c),
            Ev::Handback { run, step, image_b64 } => self.add_frame(run, *step, image_b64),
            Ev::BackupMade { .. }
            | Ev::UpdateStarted { .. }
            | Ev::UpdateFinished { .. }
            | Ev::UpdateRolledBack { .. }
            | Ev::UpdateFailed { .. }
            | Ev::BackupChecked { .. }
            | Ev::BackupCopied { .. } => self.server_ops.fold(&e.ev, e.at),
            // Which erasures a backup still holds follows the scrub.
            Ev::BackupScrubPending { .. } | Ev::BackupsScrubbed { .. } => {
                self.server_ops.fold(&e.ev, e.at);
                self.erasures_pending(e);
            }
            Ev::ConnectorAdded { .. }
            | Ev::ConnectorSettings { .. }
            | Ev::AppConnected { .. }
            | Ev::AppDisconnected { .. }
            | Ev::CredentialSet { .. }
            | Ev::RegistrationSet { .. }
            | Ev::ToolClassified { .. } => self.apps.fold(&e.ev, e.at),
            // Its people's acceptance of its connect screen goes with it.
            Ev::ConnectorRemoved { name } => {
                self.apps.fold(&e.ev, e.at);
                self.tools.consents.remove(name);
            }
            Ev::ContextReset { run, from, to, note, .. } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.cuts.push(Cut { from: *from, to: *to, note: note.clone() });
                    r.lost_since = *to;
                }
            }
            Ev::Escalated { run, adapter, model, .. } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.escalated = Some((adapter.clone(), model.clone()));
                    r.lost_since = r.replies.len() as u32;
                }
            }
            Ev::SubjectBound { person, subject } => match subject {
                Some(sub) => {
                    self.subjects.insert(person.clone(), sub.clone());
                }
                None => {
                    self.subjects.remove(person);
                }
            },
            Ev::RightsFromProvider { person, groups, roles } => {
                self.provider_rights.insert(person.clone(), (groups.clone(), roles.clone()));
            }
            Ev::DataAccessSet { private } => self.data_private = *private,
            Ev::PersonDataRead { .. } => {}
            // Every per-person part forgets them (`forget.rs`).
            Ev::PersonErased { person } => self.forget_person(person),
            Ev::RolesSet { person, roles } => {
                self.roles_set.insert(person.clone(), roles.clone());
            }
            Ev::PreferencesSet { person, prefs } => {
                let p = self.preferences.entry(person.clone()).or_insert_with(|| serde_json::json!({}));
                if let (Some(p), Some(new)) = (p.as_object_mut(), prefs.as_object()) {
                    for (k, v) in new {
                        p.insert(k.clone(), v.clone());
                    }
                }
                // Daily times follow the person's zone (owner, 2026-11-14).
                self.arrangements.set_zone(person, crate::cadence::Zone::of_preferences(self.preferences.get(person)));
            }
            Ev::ChatHidden { .. } | Ev::Reacted { .. } | Ev::DisagreementRecorded { .. } | Ev::DisagreementOutcome { .. } | Ev::ThreadBranched { .. } => {
                self.thread_extras.fold(&e.ev, e.at)
            }
            Ev::SignedIn { person, .. } => {
                self.signed_in_before.insert(person.clone());
                self.last_sign_in.insert(person.clone(), e.at);
                // Signed in again: the provider has them back.
                self.turned_off.remove(person);
            }
            Ev::HomeOpened { person } => {
                self.home_last_open.insert(person.clone(), e.at);
            }
            Ev::WatchedSeen { person, app, tool, ids, full } => self.watch.fold_seen(person, app, tool, ids, *full),
            Ev::WatchPolled { person, app, tool } => self.watch.fold_polled(person, app, tool, e.at),
            Ev::ItemOpened { id } => {
                // First presentation stands: opened is when it was
                // first seen, and the app reports it on every open.
                self.items_opened.entry(id.clone()).or_insert(e.at);
            }
            Ev::SignedOut { .. } | Ev::PasswordChanged { .. } | Ev::DevicePaired { .. } | Ev::DeviceRevoked { .. } => {}
            Ev::PersonTurnedOff { person, .. } => {
                self.turned_off.insert(person.clone());
            }
            Ev::ScheduleSet { .. }
            | Ev::ScheduleRemoved { .. }
            | Ev::ScheduleEnded { .. }
            | Ev::ScheduleSkipped { .. }
            | Ev::SchedulePaused { .. }
            | Ev::ScheduleChanged { .. }
            | Ev::ScheduleResumed { .. }
            | Ev::ScheduleFailed { .. }
            | Ev::GrantSet { .. }
            | Ev::GrantRevoked { .. }
            | Ev::GrantUsed { .. }
            | Ev::CommitmentRecorded { .. }
            | Ev::CommitmentChanged { .. }
            | Ev::CommitmentRemoved { .. }
            | Ev::CardDeclined { .. }
            | Ev::CardDeferred { .. }
            | Ev::CardAccepted { .. } => self.arrangements.fold(e, &self.preferences),
            Ev::Trashed { item, person } => {
                if let Some((title, title_from, held)) = self.take(item) {
                    self.trash.insert(item.clone(), Trashed { person: person.clone(), title, title_from, at: e.at, held });
                }
            }
            Ev::Restored { item } => {
                if let Some(t) = self.trash.remove(item) {
                    self.put_back(t.held);
                }
            }
            Ev::Removed { item, .. } => {
                // A person's erasure: their subject is still bound here.
                if let Some(p) = item.strip_prefix("person:") {
                    self.erasures.insert(p.to_string(), Erasure { removal: e.seq, subject: self.subjects.get(p).cloned(), pending: false });
                }
                if let Some(task) = item.strip_prefix(crate::team::upkeep::NOTES) {
                    self.team.drop_notes(task);
                }
                match self.trash.remove(item).map(|t| t.held) {
                    Some(Held::Entry { thread }) => {
                        self.thread_extras.hide(thread, item.clone());
                    }
                    Some(Held::Thread { id, .. }) => {
                        self.removed_threads.insert(id);
                    }
                    _ => {}
                }
            }
            Ev::FileUploaded { .. }
            | Ev::ProjectSet { .. }
            | Ev::Moved { .. }
            | Ev::KnowledgeAdded { .. }
            | Ev::KnowledgeShared { .. }
            | Ev::KnowledgeEndorsed { .. }
            | Ev::Memory { .. }
            | Ev::MemoryForgotten { .. }
            | Ev::BundleWritten { .. }
            | Ev::BundleShared { .. }
            | Ev::BundleRemoved { .. } => self.library.fold(e, &self.runs),
            // Each playbook change is a new version, noticed (U5).
            Ev::Playbook { .. } => {
                self.library.fold(e, &self.runs);
                self.org.note_version(e, "playbook", format!("v{}", self.library.playbook_version()));
            }
            Ev::ScheduleFired { id, task } => {
                // A run its schedule created was counted when it was created
                // (below), so a firing lost after it never lets one more run.
                let counted = self.tasks.get(task).is_some_and(|t| t.schedule.as_deref() == Some(id.as_str()));
                self.arrangements.fired(id, counted, e.at);
                if let Some(t) = self.tasks.get_mut(task) {
                    t.schedule = Some(id.clone());
                }
            }
            Ev::ToolPinned { tool, hash, description, input_schema, approved } => self.tools.pinned(tool, hash, description, input_schema, *approved),
            Ev::ToolChanged { tool, hash, .. } => self.tools.changed(tool, hash),
            Ev::Tools(c) => self.tools.apply(e.at, c),
            Ev::Evidence(c) => self.evidence.apply(e.at, c),
            Ev::Spend(c) => self.spend.apply(e.at, c),
            Ev::Catalog(c) => self.catalog.apply(c),
            Ev::TaskCreated { task, run, person, config, attachments, experiment, chat, label, files, project, grant, .. } => {
                // Playbook items the run starts with carry the taint of the
                // runs that wrote them (I2, M1), and a task from chat that of
                // the turn that started it.
                let pb = self.playbook_at(person, config.playbook).into_iter().fold(label.unwrap_or_default(), |l, i| l.join(i.label));
                // So do the person's open commitments, which its context
                // carries as they stand now (#236): words read in a mailbox
                // do not enter a clean run untainted.
                let pb = self.arrangements.commitments().values().filter(|c| &c.person == person && c.status == "open").fold(pb, |l, c| l.join(c.label));
                let uploads = !attachments.is_empty() || !files.is_empty();
                // A schedule's run counts against it as it is created.
                if let Some(id) = e.actor.strip_prefix("schedule:") {
                    self.arrangements.started(id, e.at);
                }
                if !attachments.is_empty() {
                    self.attachments.insert(task.clone(), attachments.clone());
                }
                if chat.as_ref().is_some_and(|c| c.opener.is_some()) {
                    self.openers.insert((person.clone(), task.clone()));
                }
                self.tasks.insert(
                    task.clone(),
                    Task {
                        task_id: task.clone(),
                        run_id: run.clone(),
                        person: person.clone(),
                        created_at: e.at,
                        chat: chat.clone(),
                        // Known from the start, so its first step knows it is scheduled.
                        schedule: e.actor.strip_prefix("schedule:").map(String::from),
                        review: e.actor == "review",
                        poll: e.actor == "poll",
                        seq: e.seq,
                        files: files.clone(),
                        project: project.clone(),
                        verdict: None,
                        grant: grant.clone(),
                    },
                );
                self.runs.insert(
                    run.clone(),
                    Run {
                        id: run.clone(),
                        task: task.clone(),
                        person: person.clone(),
                        config: config.clone(),
                        status: Status::Running,
                        reason: None,
                        replies: vec![],
                        results: HashMap::new(),
                        notices: vec![],
                        frames: vec![],
                        contract: None,
                        contract_version: 0,
                        work_started: false,
                        contract_frozen: None,
                        contract_taint: (false, false),
                        contract_approved_by: None,
                        verified: BTreeMap::new(),
                        reviewed: 0,
                        // Uploaded files are the person's private data, and
                        // content from elsewhere: data, not instructions.
                        untrusted: uploads || pb.untrusted,
                        private: uploads || pb.private,
                        outside: false,
                        // Privacy carried in from where the run started (a
                        // thread, a schedule's source) has no app to name:
                        // the unnamed source, as for a result without one,
                        // so readers and the audience rule still see it.
                        private_from: if uploads {
                            ["library".to_string()].into()
                        } else if pb.private {
                            [String::new()].into()
                        } else {
                            Default::default()
                        },
                        model_calls: 0,
                        aux_calls: 0,
                        actions: 0,
                        usd: 0.0,
                        sub_usd: 0.0,
                        intents: HashMap::new(),
                        wait_until: None,
                        summary: None,
                        outputs: vec![],
                        last_seq: e.seq,
                        experiment: experiment.clone(),
                        plan: None,
                        plan_version: 0,
                        outcome_view: None,
                        spoken: None,
                        grades: vec![],
                        created_at: e.at,
                        ended_at: None,
                        last_at: e.at,
                        first_stop: None,
                        decisions: vec![],
                        suspensions: vec![],
                        tokens: Tokens::default(),
                        model_errors: 0,
                        checks_failed: 0,
                        waiting_item: None,
                        cuts: vec![],
                        lost_since: 0,
                        escalated: None,
                        past_grant: false,
                        personal: false,
                    },
                );
            }
            Ev::ModelCall { run, context, .. } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.model_calls += 1;
                    r.personal |= context.carried_person();
                }
            }
            Ev::ModelReply { run, step, blocks, state, usage, .. } => {
                self.charge_grant(run, e.at, usage.cost_usd);
                if let Some(r) = self.runs.get_mut(run) {
                    r.usd += usage.cost_usd;
                    r.tokens.input += usage.input_tokens;
                    r.tokens.output += usage.output_tokens;
                    r.tokens.cache_read += usage.cache_read_tokens;
                    r.tokens.cache_write += usage.cache_write_tokens;
                    r.replies.push(Reply { step: *step, seq: e.seq, blocks: blocks.clone(), state: state.clone() });
                }
            }
            Ev::VerifierReply { run, usage, .. }
            | Ev::ReviewerReply { run, usage, .. }
            | Ev::MemoryPass { run, usage, .. }
            | Ev::SpokenWritten { run, usage, .. }
            | Ev::KnowledgeExtraction { run, usage, .. }
            | Ev::DraftScoped { run, usage, .. } => {
                self.charge_grant(run, e.at, usage.cost_usd);
                if let Ev::DraftScoped { draft, person, threads, participants, .. } = &e.ev {
                    self.draft_sources.insert(draft.clone(), DraftSources { person: person.clone(), threads: threads.clone(), participants: participants.clone() });
                }
                if let Some(r) = self.runs.get_mut(run) {
                    if let Ev::SpokenWritten { script, .. } = &e.ev {
                        r.spoken = Some(script.clone());
                    }
                    // What the reviewer module did, not the checks that share its model.
                    if matches!(&e.ev, Ev::ReviewerReply { purpose, .. } if crate::run::reviewer::MODULE_PURPOSES.contains(&purpose.as_str())) {
                        r.reviewed += 1;
                    }
                    r.model_calls += 1;
                    r.aux_calls += 1;
                    r.usd += usage.cost_usd;
                    r.tokens.input += usage.input_tokens;
                    r.tokens.output += usage.output_tokens;
                    r.tokens.cache_read += usage.cache_read_tokens;
                    r.tokens.cache_write += usage.cache_write_tokens;
                }
            }
            Ev::ModelError { run, usage, .. } => {
                if let Some(u) = usage {
                    self.charge_grant(run, e.at, u.cost_usd);
                }
                if let Some(r) = self.runs.get_mut(run) {
                    r.model_errors += 1;
                    if let Some(usage) = usage {
                        r.usd += usage.cost_usd;
                        r.tokens.input += usage.input_tokens;
                        r.tokens.output += usage.output_tokens;
                        r.tokens.cache_read += usage.cache_read_tokens;
                        r.tokens.cache_write += usage.cache_write_tokens;
                    }
                }
            }
            Ev::Notice { run, step, text } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.notices.push((*step, e.seq, text.clone()));
                }
            }
            Ev::Intent { run, call_id, key, tool, input, issuer, .. } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.actions += 1;
                    r.intents.insert(key.clone(), Intent { call_id: call_id.clone(), tool: tool.clone(), at: e.at, input: input.clone(), issuer: issuer.clone(), result: None });
                }
            }
            Ev::Decision { run, call_id, tool, outcome, stage, rule, .. } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.decisions.push(DecisionRec { seq: e.seq, call_id: call_id.clone(), tool: tool.clone(), stage: stage.clone(), outcome: outcome.clone(), rule: rule.clone() });
                }
            }
            Ev::ToolResult { run, call_id, key, status, text, is_error, label, data, sources, check } => {
                if let Some(r) = self.runs.get_mut(run) {
                    let rec = ResultRec { seq: e.seq, text: text.clone(), is_error: *is_error, status: status.clone(), data: data.clone() };
                    if let Some(k) = key {
                        if let Some(i) = r.intents.get_mut(k) {
                            i.result = Some(rec.clone());
                        }
                    }
                    if !call_id.is_empty() {
                        r.results.insert(call_id.clone(), rec);
                    }
                    // A check's result is compared, never read by the model:
                    // taint limits authority, not reading.
                    if label.untrusted && !*check {
                        r.untrusted = true;
                        r.outside = true;
                    }
                    if label.private && !*check {
                        r.private = true;
                        r.private_from.extend(sources.iter().cloned());
                        if sources.is_empty() {
                            r.private_from.insert(String::new());
                        }
                    }
                }
            }
            Ev::Contract { run, version, contract, approved_by } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.contract = Some(contract.clone());
                    r.contract_version = *version;
                    r.verified.clear();
                    // An approved change keeps the run's taint: the person approved
                    // what counts as done, not each send its checks make.
                    r.contract_taint = (r.untrusted, r.private);
                    if approved_by.is_some() {
                        r.contract_frozen = Some(*version);
                    }
                    r.contract_approved_by = approved_by.clone();
                }
            }
            // Retired with contracts (spec rev 147): read in old logs only.
            Ev::WorkStarted { run } => self.runs.get_mut(run).into_iter().for_each(|r| r.work_started = true),
            Ev::ContractFrozen { run, version } => self.runs.get_mut(run).into_iter().for_each(|r| r.contract_frozen = Some(*version)),
            Ev::Plan { run, version, plan } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.plan = Some(plan.clone());
                    r.plan_version = *version;
                }
            }
            Ev::Verdict { task, person, verdict, note } => {
                let trial = self.tasks.get(task).and_then(|t| self.runs.get(&t.run_id)).is_some_and(|r| r.experiment.is_some());
                if let Some(t) = self.tasks.get_mut(task).filter(|t| &t.person == person || trial) {
                    t.verdict = Some(Verdict { verdict: verdict.clone(), note: note.clone(), at: e.at });
                }
            }
            Ev::Graded { run, grader, passed, score, details } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.grades.push((grader.clone(), *passed, *score, details.clone()));
                }
            }
            Ev::Verification { run, criterion, passed, evidence, by } => {
                if let Some(r) = self.runs.get_mut(run) {
                    if !passed {
                        r.checks_failed += 1;
                    }
                    r.verified.insert(criterion.clone(), (*passed, evidence.clone(), by.clone()));
                }
            }
            Ev::Suspended { run, reason, item } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.suspensions.push(reason.clone());
                    r.first_stop.get_or_insert(e.at);
                    if !r.status.terminal() {
                        r.status = Status::Suspended;
                        r.reason = Some(reason.clone());
                        r.waiting_item = item.clone();
                    }
                }
            }
            Ev::Resumed { run } => {
                if let Some(r) = self.runs.get_mut(run) {
                    if r.reason.as_deref() == Some("grant_budget") {
                        r.past_grant = true;
                    }
                    if !r.status.terminal() {
                        r.status = Status::Running;
                        r.reason = None;
                        r.wait_until = None;
                        r.waiting_item = None;
                    }
                }
            }
            // A run ends once: the first of these stands, so a cancel that
            // loses the race to the run's own end changes nothing, nor does
            // the run's end after a cancel.
            Ev::Completed { run, summary, view, spoken } => {
                self.review_runs.remove(run);
                if let Some(r) = self.runs.get_mut(run).filter(|r| !r.status.terminal()) {
                    r.status = Status::Completed;
                    r.summary = Some(summary.clone());
                    r.outcome_view = view.clone();
                    r.spoken = spoken.clone();
                    r.reason = None;
                    r.ended_at = Some(e.at);
                    r.first_stop.get_or_insert(e.at);
                }
            }
            Ev::Failed { run, reason, .. } => {
                self.review_runs.remove(run);
                if let Some(r) = self.runs.get_mut(run).filter(|r| !r.status.terminal()) {
                    r.status = Status::Failed;
                    r.summary = Some(reason.clone());
                    r.reason = None;
                    r.ended_at = Some(e.at);
                    r.first_stop.get_or_insert(e.at);
                }
            }
            Ev::Cancelled { run, .. } => {
                self.review_runs.remove(run);
                if let Some(r) = self.runs.get_mut(run).filter(|r| !r.status.terminal()) {
                    if e.actor.starts_with("person:") {
                        self.stopped_by_person.insert(run.clone());
                    }
                    r.status = Status::Cancelled;
                    r.reason = None;
                    r.ended_at = Some(e.at);
                    r.first_stop.get_or_insert(e.at);
                }
            }
            Ev::Item { id, item_kind, task, run, person, summary, details } => {
                self.items.insert(
                    id.clone(),
                    Item {
                        id: id.clone(),
                        kind: item_kind.clone(),
                        task: task.clone(),
                        run: run.clone(),
                        person: person.clone(),
                        summary: summary.clone(),
                        details: details.clone(),
                        created_at: e.at,
                        // A retired review of tool descriptions waits on no one
                        // now: definitions are screened (spec rev 140).
                        resolution: crate::consent::RETIRED_KINDS
                            .contains(&item_kind.as_str())
                            .then(|| ("superseded".into(), None, Some(crate::consent::RETIRED_TEXT.into()), "kernel".into())),
                    },
                );
                // A delivered interruption counts against the person's day
                // (#237); observations and notices never do.
                if item_kind == "interrupt" {
                    let tz = self.preferences.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                    let today = crate::proactive::day_key(e.at, tz);
                    self.interrupts
                        .entry(person.clone())
                        .and_modify(|(day, n)| {
                            if *day == today {
                                *n += 1
                            } else {
                                *day = today.clone();
                                *n = 1
                            }
                        })
                        .or_insert((today, 1));
                }
            }
            Ev::ReviewWoken { run, person, reason, .. } => {
                self.reviews_last.insert(person.clone(), e.at);
                self.review_runs.insert(run.clone(), person.clone());
                if reason == "overnight" {
                    let tz = self.preferences.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                    self.reviews_overnight.insert(person.clone(), crate::proactive::day_key(e.at, tz));
                }
                for (prefix, suffix) in [("schedule ", " coming up"), ("commitment ", " due"), ("approval ", " waiting"), ("task ", " done"), ("task ", " failed")] {
                    if let Some(id) = reason.strip_prefix(prefix).and_then(|r| r.strip_suffix(suffix)) {
                        let tz = self.preferences.get(person).map(crate::proactive::tz_of).unwrap_or(chrono_tz::Tz::UTC);
                        self.wake_marks.insert(format!("{person}:{prefix}{id}"), crate::proactive::day_key(e.at, tz));
                    }
                }
            }
            Ev::ItemResolved { id, decision, scope, text } => {
                self.items_resolved_at.insert(id.clone(), e.at);
                if let Some(i) = self.items.get_mut(id) {
                    i.resolution = Some((decision.clone(), scope.clone(), text.clone(), e.actor.clone()));
                    // A stopped run's notice put away: the run waits on no
                    // card now, so its own Resume (or Cancel) is what is left.
                    if decision == "dismiss" {
                        if let Some(r) = i.run.as_ref().and_then(|r| self.runs.get_mut(r)).filter(|r| r.waiting_item.as_deref() == Some(id.as_str())) {
                            r.waiting_item = None;
                        }
                    }
                }
            }
            Ev::Approval { id, item, person, task, tool, input_hash, scope, until, reason, risk, destinations } => self.approvals.push(Approval {
                id: id.clone(),
                person: person.clone(),
                task: task.clone(),
                tool: tool.clone(),
                input_hash: input_hash.clone(),
                scope: scope.clone(),
                until: *until,
                used: false,
                by: e.actor.clone(),
                reason: reason.clone(),
                // Approvals written before the event kept it read it from
                // their item; with that deleted, they ask again.
                risk: risk.clone().or_else(|| self.items.get(item).and_then(|i| i.details["risk"].as_str()).map(String::from)),
                destinations: destinations.clone(),
            }),
            Ev::ApprovalUsed { id, .. } => {
                if let Some(a) = self.approvals.iter_mut().find(|a| &a.id == id) {
                    a.used = true;
                }
            }
            Ev::Wait { run, until } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.wait_until = Some(*until);
                }
            }
            Ev::BudgetExtended { run, budget } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.config.budget = budget.clone();
                }
            }
            Ev::Output { id, run, name, media_type, data_b64 } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.outputs.push((id.clone(), name.clone(), media_type.clone(), data_b64.len() * 3 / 4));
                    let f = LibFile {
                        id: id.clone(),
                        person: r.person.clone(),
                        name: name.clone(),
                        media_type: media_type.clone(),
                        size: data_b64.len() * 3 / 4,
                        at: e.at,
                        uploaded: false,
                        seq: e.seq,
                        run: Some(run.clone()),
                        thread: None,
                        label: Label { untrusted: r.untrusted, private: r.private },
                    };
                    // The run delivered it: a file in its person's library.
                    self.library.put_file(f);
                }
            }
        }
        crate::outcomes::observe(self, e);
    }

    /// The person's playbook items that were live at playbook version `v`.
    pub fn playbook_at(&self, person: &str, v: u64) -> Vec<&PlaybookItem> {
        self.library.playbook_at(person, v)
    }

    pub fn open_item_for_run(&self, run: &str, kind: &str) -> Option<&Item> {
        self.items.values().find(|i| i.run.as_deref() == Some(run) && i.kind == kind && i.resolution.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::State;
    use crate::log::Event;
    use serde_json::json;

    fn ev(seq: i64, actor: &str, ev: serde_json::Value) -> Event {
        Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
    }

    /// A schedule's run counts against its limit as it is created, so a
    /// crash before the firing is recorded never lets it run once more.
    #[test]
    fn a_scheduled_run_counts_when_it_is_created() {
        let mut s = State::default();
        s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(2, "schedule:s1", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
        // The firing itself was lost: the schedule is still out of runs.
        assert_eq!(s.arrangements.schedules()["s1"].started, 1);
        assert!(s.arrangements.schedules()["s1"].next_due().is_none(), "a lost firing let the schedule run again");
        // And when the firing is recorded, it is not counted twice.
        s.apply(&ev(3, "kernel", json!({ "kind": "schedule_fired", "id": "s1", "task": "t1" })));
        assert_eq!(s.arrangements.schedules()["s1"].started, 1);
    }

    /// An admin's suspension marks the schedule until a resume clears it;
    /// pauses written before suspensions existed replay unmarked (#262).
    #[test]
    fn an_admin_suspension_marks_the_schedule_until_resumed() {
        let mut s = State::default();
        s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
        s.apply(&ev(2, "kernel", json!({ "kind": "schedule_paused", "id": "s1", "reason": "its last 3 runs failed" })));
        assert!(!s.arrangements.schedules()["s1"].paused_by_admin, "an old pause replayed as an admin's suspension");
        s.apply(&ev(3, "kernel", json!({ "kind": "schedule_resumed", "id": "s1" })));
        s.apply(&ev(4, "admin:root", json!({ "kind": "schedule_paused", "id": "s1", "reason": "Suspended by Root, an admin", "admin": true })));
        assert!(s.arrangements.schedules()["s1"].paused_by_admin, "the suspension left no mark");
        s.apply(&ev(5, "admin:root", json!({ "kind": "schedule_resumed", "id": "s1" })));
        assert!(!s.arrangements.schedules()["s1"].paused_by_admin, "the resume kept the mark");
    }

    /// A model call the provider charged for and then failed (a reply it
    /// could not use) counts against the run's dollars and tokens.
    #[test]
    fn a_charged_call_that_failed_counts_against_the_budget() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
        let usage = json!({ "input_tokens": 1000, "output_tokens": 200, "cost_usd": 0.25 });
        s.apply(&ev(2, "kernel", json!({ "kind": "model_error", "run": "r1", "step": 0, "attempt": 0, "class": "malformed", "message": "no model named", "usage": usage })));
        assert_eq!((s.runs["r1"].usd, s.runs["r1"].tokens.input, s.runs["r1"].tokens.output), (0.25, 1000, 200), "a charged failure was not counted");
        // An error recorded before usage was kept still loads, and costs nothing.
        s.apply(&ev(3, "kernel", json!({ "kind": "model_error", "run": "r1", "step": 0, "attempt": 1, "class": "unavailable", "message": "down" })));
        assert_eq!(s.runs["r1"].usd, 0.25);
    }

    /// Dismissing the notice a stopped run waits on leaves the run waiting on
    /// no card, so the task's own Resume is not refused for it.
    #[test]
    fn a_dismissed_stop_notice_frees_the_run() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Say hi.", "config": config })));
        s.apply(&ev(
            2,
            "kernel",
            json!({ "kind": "item", "id": "i1", "item_kind": "notice", "task": "t1", "run": "r1", "person": "alice", "summary": "Stopped", "details": { "reason": "budget" } }),
        ));
        s.apply(&ev(3, "kernel", json!({ "kind": "suspended", "run": "r1", "reason": "budget", "item": "i1" })));
        assert_eq!(s.runs["r1"].waiting_item.as_deref(), Some("i1"));
        s.apply(&ev(4, "person:alice", json!({ "kind": "item_resolved", "id": "i1", "decision": "dismiss" })));
        assert_eq!(s.runs["r1"].waiting_item, None, "the run still waits on a card put away");
        assert_eq!(s.runs["r1"].reason.as_deref(), Some("budget"), "dismissing it resumed nothing");
    }

    /// A run ends once: the first of completed, failed and cancelled stands.
    /// A cancel that loses the race to the run's own end does not turn a
    /// finished task into a cancelled one, nor its end into a cancel.
    #[test]
    fn a_run_ends_once_the_first_end_stands() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        for (seq, t) in [(1, "1"), (2, "2")] {
            s.apply(&ev(
                seq,
                "person:alice",
                json!({ "kind": "task_created", "task": format!("t{t}"), "run": format!("r{t}"), "person": "alice", "text": "Say hi.", "config": config }),
            ));
        }
        s.apply(&ev(3, "kernel", json!({ "kind": "completed", "run": "r1", "summary": "Said hi." })));
        let ended = s.runs["r1"].ended_at;
        s.apply(&ev(4, "person:alice", json!({ "kind": "cancelled", "run": "r1" })));
        assert_eq!(s.runs["r1"].status, super::Status::Completed, "a cancel after the run completed made it cancelled");
        assert_eq!((s.runs["r1"].summary.as_deref(), s.runs["r1"].ended_at), (Some("Said hi."), ended));
        s.apply(&ev(5, "person:alice", json!({ "kind": "cancelled", "run": "r2" })));
        s.apply(&ev(6, "kernel", json!({ "kind": "failed", "run": "r2", "reason": "stopped", "by": "kernel" })));
        assert_eq!(s.runs["r2"].status, super::Status::Cancelled, "the run's own end after a cancel made it failed");
        assert_eq!(s.runs["r2"].summary, None);
    }

    /// A person erased leaves nothing of theirs in the state: not the
    /// conversation a branch of theirs starts with, nor what their tasks
    /// had attached; someone else's stay.
    #[test]
    fn a_person_erased_leaves_no_branch_or_attachment_of_theirs() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        for (seq, p) in [(1, "bob"), (2, "alice")] {
            let file = json!([{ "id": format!("f-{p}"), "name": format!("{p}-salary.pdf") }]);
            s.apply(&ev(
                seq,
                &format!("person:{p}"),
                json!({ "kind": "task_created", "task": format!("t-{p}"), "run": format!("r-{p}"), "person": p, "text": "Read this.", "attachments": file, "config": config }),
            ));
            s.apply(&ev(seq + 10, &format!("person:{p}"), json!({ "kind": "thread_branched", "person": p, "thread": format!("b-{p}"), "from": format!("t-{p}"), "upto": "e1", "entries": [{ "id": "e1", "text": format!("{p} earns 91,000") }], "title": "Pay" })));
        }
        s.apply(&ev(30, "admin:root", json!({ "kind": "person_erased", "person": "bob" })));
        assert!(!s.thread_extras.branches().contains_key("b-bob"), "bob's branch kept his words");
        assert!(!s.attachments.contains_key("t-bob"), "bob's task kept what it had attached");
        assert!(s.thread_extras.branches().contains_key("b-alice") && s.attachments.contains_key("t-alice"), "alice's went with bob's");
    }

    /// A scoped draft records its threads and who is entitled to them, and
    /// charges the run's call; erasing the person clears it (#178).
    #[test]
    fn a_scoped_draft_records_its_sources_and_charges_the_call() {
        let mut s = State::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        s.apply(&ev(1, "person:alice", json!({ "kind": "task_created", "task": "t1", "run": "r1", "person": "alice", "text": "Answer it.", "config": config })));
        let usage = json!({ "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.01 });
        s.apply(&ev(
            2,
            "kernel",
            json!({ "kind": "draft_scoped", "run": "r1", "person": "alice", "draft": "d1", "threads": ["tA"], "participants": ["bob@corp.example"], "usage": usage }),
        ));
        assert_eq!(s.draft_sources["d1"], super::DraftSources { person: "alice".into(), threads: vec!["tA".into()], participants: vec!["bob@corp.example".into()] });
        assert_eq!((s.runs["r1"].model_calls, s.runs["r1"].usd), (1, 0.01));
        s.apply(&ev(3, "admin:root", json!({ "kind": "person_erased", "person": "alice" })));
        assert!(!s.draft_sources.contains_key("d1"), "alice's draft kept its sources");
    }

    /// An ended schedule stays in state, marked with why, its pause
    /// cleared: only a cancelled one is gone. It is not finished twice,
    /// and an edit puts it back (#264).
    #[test]
    fn an_ended_schedule_stays_marked_with_why_until_cancelled() {
        let mut s = State::default();
        s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
        s.apply(&ev(2, "person:alice", json!({ "kind": "schedule_paused", "id": "s1", "reason": "Paused by you" })));
        s.apply(&ev(3, "kernel", json!({ "kind": "schedule_ended", "id": "s1", "reason": "it ran 1 time, as set" })));
        let sc = s.arrangements.schedules().get("s1").expect("an ended schedule was removed");
        assert_eq!(sc.ended.as_deref(), Some("it ran 1 time, as set"), "the end kept no reason");
        assert!(sc.paused.is_none(), "the end kept the pause");
        assert!(sc.finished(chrono::Utc::now()).is_none(), "an ended schedule finishes again");
        // An edit puts it back: a new end, no mark.
        s.apply(&ev(4, "person:alice", json!({ "kind": "schedule_changed", "id": "s1", "every_minutes": 60, "daily_at": null, "until": null, "max_runs": 2 })));
        assert!(s.arrangements.schedules()["s1"].ended.is_none(), "the edit kept the end mark");
        // Cancelling removes it, as before.
        s.apply(&ev(5, "person:alice", json!({ "kind": "schedule_removed", "id": "s1" })));
        assert!(!s.arrangements.schedules().contains_key("s1"), "a cancelled schedule stayed");
    }

    /// Erasure drops watched baselines, home marks and opened marks
    /// (#412): whoever is next known by the name starts fresh.
    #[test]
    fn erasure_drops_baselines_and_home_and_opened_marks() {
        let mut s = State::default();
        s.apply(&ev(1, "person:bob", json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ["a"], "full": true })));
        s.apply(&ev(2, "person:bob", json!({ "kind": "home_opened", "person": "bob" })));
        s.apply(&ev(3, "person:bob", json!({ "kind": "item", "id": "i1", "item_kind": "approval", "task": null, "run": null, "person": "bob" })));
        s.apply(&ev(4, "person:bob", json!({ "kind": "item_opened", "id": "i1" })));
        s.apply(&ev(5, "person:bob", json!({ "kind": "item_resolved", "id": "i1", "decision": "approved" })));
        assert!(!s.watch.is_empty() && !s.home_last_open.is_empty() && !s.items_opened.is_empty() && !s.items_resolved_at.is_empty(), "the setup left nothing to erase");
        s.apply(&ev(6, "kernel", json!({ "kind": "person_erased", "person": "bob" })));
        assert!(s.watch.is_empty(), "the erased baseline stayed: {:?}", s.watch);
        assert!(!s.home_last_open.contains_key("bob"), "the erased home mark stayed");
        assert!(!s.items_opened.contains_key("i1"), "the erased opened mark stayed");
        assert!(!s.items_resolved_at.contains_key("i1"), "the erased resolved mark stayed");
    }
}
