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
    /// Apps whose private content this run has read.
    pub private_from: std::collections::BTreeSet<String>,
    pub model_calls: u32,
    pub actions: u32,
    pub usd: f64,
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
    /// A scheduled watch poll's task: a quiet read, hidden from the
    /// person's lists (#406).
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

/// A disagreement Magnus recorded: what it advised, what the person
/// decided, and later how it turned out.
#[derive(Clone, Debug, Serialize)]
pub struct Disagreement {
    pub id: String,
    pub person: String,
    pub position: String,
    pub decision: String,
    pub at: DateTime<Utc>,
    pub outcome: Option<String>,
    pub outcome_at: Option<DateTime<Utc>>,
}

/// The verdicts a person can give, best first.
/// A secret the kill switch took out: `at_provider` is None until the
/// provider answered.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Revoked {
    pub adapter: Option<String>,
    pub at_provider: Option<bool>,
    pub detail: Option<String>,
}

pub const VERDICTS: [&str; 3] = ["right", "partly", "wrong"];

/// A file in a person's library: one they uploaded, or one a task delivered.
#[derive(Clone, Debug)]
pub struct LibFile {
    pub id: String,
    pub person: String,
    pub name: String,
    pub media_type: String,
    pub size: usize,
    pub at: DateTime<Utc>,
    /// An upload (it can be given to a task or message), not a file a run
    /// delivered.
    pub uploaded: bool,
    /// The event that holds its content (the upload, or the run's output):
    /// its bytes are recalled from there for its person when a request
    /// needs them, never kept here (recall only, step 5).
    pub seq: i64,
    /// The run that delivered it.
    pub run: Option<String>,
    /// The chat thread it was uploaded in.
    pub thread: Option<String>,
    /// Its content's taint: an upload is private and untrusted; a delivered
    /// file has the taint its run had when it delivered it.
    pub label: Label,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub person: String,
    pub name: String,
    pub instructions: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Task {
    /// A chat turn: answered in the thread, not listed as a task.
    pub fn is_chat_turn(&self) -> bool {
        self.chat.as_ref().is_some_and(|c| c.turn)
    }

    /// Magnus's own background work (a proactive review, a watch poll):
    /// it reaches the person only through what it produces for them, never
    /// through its own approvals, reviews, questions or notices (owner,
    /// 2026-11-07). Developers see all of it in the run inspector.
    pub fn background(&self) -> bool {
        self.review || self.poll
    }
}

/// What a background run may put in front of its person: what it produced
/// for them. Anything else about it is its own plumbing (owner, 2026-11-07).
pub const BACKGROUND_OUTPUTS: [&str; 5] = ["observation", "interrupt", "schedule_proposal", "task_proposal", "message"];

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
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub hash: String,
    pub description: String,
    pub input_schema: Value,
    pub approved: bool,
}

/// A knowledge entry's record. What it says (its text, source, evidence
/// and value) is not kept here: it is read from the content store when the
/// entry is indexed, and every reader reads it from the index, as the
/// readers its row security allows (recall only, step 6). The title stays
/// until step 3 settles the titles shown in lists.
#[derive(Clone, Debug, Serialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub readers: Vec<Vec<String>>,
    pub integrity: String,
    pub author: String,
    pub run: Option<String>,
    pub endorsed: Option<Value>,
    /// The last share, by whom and when: who widened the readers.
    pub shared: Option<Value>,
    pub at: DateTime<Utc>,
    /// The last event that changed it, so the index knows it is current.
    pub version: i64,
    /// The event that added it: where its content is.
    pub seq: i64,
}

#[derive(Clone, Debug)]
/// A remembered fact's record. What it says (the fact and its topic) is
/// not kept here: it is recalled from the content store, for its person,
/// when a request needs it (recall only, step 4).
pub struct MemoryEntry {
    pub id: String,
    pub person: String,
    pub run: String,
    pub label: Label,
    pub sources: Vec<String>,
    /// The memory pass, or None for the run's own model.
    pub by: Option<String>,
    pub at: DateTime<Utc>,
    /// The event that wrote it: where its content is recalled from.
    pub seq: i64,
}

/// A document in the org context bundle, as the kernel guards it: where it
/// is, whose, its versions, and what it lists as sources. Its frontmatter
/// and body are in the content store (`bundle.rs`).
#[derive(Clone, Debug)]
pub struct BundleDoc {
    pub path: String,
    pub kind: String,
    pub scope: String,
    pub owner_user: Option<String>,
    pub label: Label,
    pub sources: Vec<String>,
    /// Each version: its number, the event holding it, when and by whom.
    pub versions: Vec<(u32, i64, DateTime<Utc>, String)>,
    /// The person whose run wrote the current version, under whose grant
    /// its sources were read.
    pub writer: Option<String>,
    /// Principals (`person:<id>`, `group:<id>`) the writer shared the
    /// current version with, besides; cleared by every new version, so a
    /// share covers only the text seen (#299).
    pub shared: Vec<String>,
    /// Shared with everyone (as `shared`, for the current version).
    pub shared_everyone: bool,
    /// Who shared the current version.
    pub shared_by: Option<String>,
}

impl BundleDoc {
    pub fn version(&self) -> u32 {
        self.versions.last().map_or(0, |v| v.0)
    }
    pub fn seq(&self) -> i64 {
        self.versions.last().map_or(0, |v| v.1)
    }
}

#[derive(Clone, Debug)]
pub struct PolicyVersion {
    pub layer: String,
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub by: String,
    pub cedar: String,
}

/// One item of the agent's playbook (I2).
#[derive(Clone, Debug)]
pub struct PlaybookItem {
    pub id: String,
    pub person: String,
    pub text: String,
    pub addresses: Option<String>,
    /// The run that added it, and the runs that edited it.
    pub runs: Vec<String>,
    pub label: Label,
    /// Playbook version at which the item was added, and retired.
    pub added: u64,
    pub retired: Option<u64>,
    pub added_at: DateTime<Utc>,
}

/// A schedule's record. What its runs are asked (its text), its title and
/// scope are recalled from the event `seq` names, as a task's are.
#[derive(Clone, Debug)]
pub struct Schedule {
    pub id: String,
    pub person: String,
    pub every_minutes: Option<u32>,
    pub daily_at: Option<String>,
    pub created: DateTime<Utc>,
    pub last_fired: Option<DateTime<Utc>>,
    pub budget: Option<crate::config::Budget>,
    pub card: Option<String>,
    pub label: Label,
    /// The event that set it: where its words are recalled from.
    pub seq: i64,
    /// Firings that could not start a task: when, and why.
    pub failures: Vec<(DateTime<Utc>, String)>,
    pub until: Option<DateTime<Utc>>,
    pub max_runs: Option<u32>,
    /// Runs it has started.
    pub started: u32,
    /// The latest firing that started nothing, because the last run was
    /// still open: when, which task, why.
    pub skipped: Option<(DateTime<Utc>, String, String)>,
    /// Why it stopped starting runs, until its person resumes it.
    pub paused: Option<String>,
    /// The task whose run it is paused on, waiting on a person.
    pub paused_on: Option<String>,
    /// An admin suspended it: only an admin lifts it.
    pub paused_by_admin: bool,
    /// From when failures count toward pausing it again.
    pub resumed: Option<DateTime<Utc>>,
    /// Why it is over, once it is: an ended schedule stays, marked.
    pub ended: Option<String>,
}

/// A person's standing authority over a category of their own work
/// (#235). Its title is recalled from the event `seq` names (the latest
/// setting), as a schedule's words are.
#[derive(Clone, Debug)]
pub struct Grant {
    pub id: String,
    pub person: String,
    pub apps: Vec<String>,
    /// Exclusions the person lifted: `send`, `irreversible`.
    pub allow: Vec<String>,
    /// `day`, `week` or `month`.
    pub period: String,
    pub max_usd: f64,
    /// The policy the kernel wrote from these: what it covers.
    pub cedar: String,
    /// The card it was first set from.
    pub card: Option<String>,
    /// The taint of the words of its title, when not the person's own.
    pub label: Label,
    pub seq: i64,
    /// When it was set first, and by whom (as the log names them).
    pub at: DateTime<Utc>,
    pub by: String,
    /// When it was last edited.
    pub edited: Option<DateTime<Utc>>,
    pub revoked: Option<DateTime<Utc>>,
    /// The actions taken under it, oldest first.
    pub actions: Vec<GrantAction>,
    /// What its tasks' model calls cost, as each was charged: kept with the
    /// grant, so deleting a task gives none of it back (review of #278).
    pub charges: Vec<(DateTime<Utc>, f64)>,
}

/// An action a grant covered.
#[derive(Clone, Debug, Serialize)]
pub struct GrantAction {
    pub tool: String,
    pub task: String,
    pub run: String,
    pub call_id: String,
    pub at: DateTime<Utc>,
}

/// Something the person or Magnus said they would do (#236). Its words
/// (what, for whom, the quote, notes) are recalled from its events, by
/// `steps`; the rest is here. Overdue is never stored: it is open with
/// `by` past (`commitments::overdue`).
#[derive(Clone, Debug)]
pub struct Commitment {
    pub id: String,
    pub person: String,
    /// `person` or `magnus`.
    pub who: String,
    pub source: Value,
    /// The taint of the run that recorded it.
    pub label: Label,
    /// When it was recorded, and by whom (as the log names them).
    pub at: DateTime<Utc>,
    pub by_actor: String,
    /// When it last changed.
    pub changed: DateTime<Utc>,
    /// Its recording event.
    pub seq: i64,
    pub status: String,
    pub by: Option<String>,
    /// Each event of it, oldest first, with its status and date after it:
    /// what it was as a run that started at any point saw it.
    pub steps: Vec<CommitmentStep>,
}

#[derive(Clone, Debug)]
pub struct CommitmentStep {
    pub seq: i64,
    pub status: String,
    pub by: Option<String>,
}

impl Commitment {
    /// Its status and date as they stood before the event `seq`, and the
    /// events that made it so; None when it was recorded after.
    pub fn before(&self, seq: i64) -> Option<(&str, Option<&str>, Vec<i64>)> {
        let steps: Vec<&CommitmentStep> = self.steps.iter().filter(|s| s.seq < seq).collect();
        let last = steps.last()?;
        Some((last.status.as_str(), last.by.as_deref(), steps.iter().map(|s| s.seq).collect()))
    }
}

/// A thread branched from another at one of its entries.
#[derive(Clone, Debug)]
pub struct Branch {
    pub person: String,
    pub from: String,
    pub upto: String,
    /// The entries up to `upto`, as they were shown then.
    pub entries: Vec<Value>,
    pub title: String,
    pub at: DateTime<Utc>,
}

/// What a scoped drafting step wrote from: the threads (none for a new
/// draft) and their participants' addresses (#178).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftSources {
    pub person: String,
    pub threads: Vec<String>,
    pub participants: Vec<String>,
}

/// The person's answer to a card proposed in chat (a schedule or a
/// suggestion), when they gave it and whose card it was.
#[derive(Clone, Debug, PartialEq)]
pub struct CardState {
    pub answer: Answer,
    pub at: DateTime<Utc>,
    pub person: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Accepted: the schedule it made.
    Confirmed(String),
    /// Accepted: the task it started.
    Started(String),
    /// Accepted: the grant it set (#235).
    Granted(String),
    /// "Not now".
    Later,
    /// Declined, with their reason.
    Declined(Option<String>),
}

impl Schedule {
    /// Whether it may start no more runs: past its end, or out of runs.
    pub fn finished(&self, now: DateTime<Utc>) -> Option<String> {
        if self.ended.is_some() {
            return None;
        }
        if let Some(m) = self.max_runs.filter(|m| self.started >= *m) {
            return Some(format!("it ran {m} time{}, as set", if m == 1 { "" } else { "s" }));
        }
        self.until.filter(|u| *u <= now).map(|u| format!("it reached its end, {}", u.to_rfc3339()))
    }

    /// The next time this schedule is due after its last run (or creation),
    /// unless it is paused or over.
    pub fn next_due(&self) -> Option<DateTime<Utc>> {
        if self.paused.is_some() || self.max_runs.is_some_and(|m| self.started >= m) {
            return None;
        }
        let next = self.next_time()?;
        self.until.is_none_or(|u| next < u).then_some(next)
    }

    fn next_time(&self) -> Option<DateTime<Utc>> {
        let after = self.last_fired.unwrap_or(self.created);
        if let Some(m) = self.every_minutes {
            return Some(after + chrono::Duration::minutes(m.max(1) as i64));
        }
        let (h, m) = self.daily_at.as_deref()?.split_once(':')?;
        let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
        next_daily(after, self.last_fired.is_some(), h, m, &chrono::Local)
    }
}

/// The next time a daily schedule at `h:m` (wall-clock time in `tz`) is due
/// after `after`, which is its last firing when `fired`, else its creation.
/// When clocks go back, the time comes twice: the schedule is due at the first
/// still ahead, once a day (made in between, at the second; never at both).
/// When clocks go forward past it, it is due as the clocks resume, an hour on.
/// The two times of a repeated hour are taken in time order, whatever order
/// the time zone gives them in.
fn next_daily<Tz: chrono::TimeZone>(after: DateTime<Utc>, fired: bool, h: u32, m: u32, tz: &Tz) -> Option<DateTime<Utc>> {
    use chrono::LocalResult;
    let local = after.with_timezone(tz);
    let mut day = local.date_naive();
    for _ in 0..3 {
        let at = day.and_hms_opt(h, m, 0)?;
        let t = match tz.from_local_datetime(&at) {
            LocalResult::Single(t) => t,
            // In time order: chrono's `Local` gives the later first.
            LocalResult::Ambiguous(x, y) => {
                let (first, second) = if x <= y { (x, y) } else { (y, x) };
                if first > local {
                    first
                } else if !fired {
                    second
                } else {
                    day = day.succ_opt()?;
                    continue;
                }
            }
            LocalResult::None => {
                // An hour on is never in a gap; take its earlier time if repeated.
                match tz.from_local_datetime(&(at + chrono::Duration::hours(1))) {
                    LocalResult::Single(t) => t,
                    LocalResult::Ambiguous(x, y) => x.min(y),
                    LocalResult::None => return None,
                }
            }
        };
        if t > local {
            return Some(t.with_timezone(&Utc));
        }
        day = day.succ_opt()?;
    }
    None
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Update {
    /// "updated", "rolled_back" or "failed".
    pub what: String,
    pub from: String,
    pub to: String,
    pub detail: String,
    pub at: DateTime<Utc>,
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

/// Removed content still in backups kept, while it is (#195): the backups
/// whose rewrite failed have had the removals up to `from`, not those after
/// it through `through`.
#[derive(Clone, Debug, Serialize)]
pub struct ScrubPending {
    pub from: i64,
    pub through: i64,
    pub files: Vec<String>,
    pub detail: String,
    pub at: DateTime<Utc>,
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

/// An admin's settings for one of the organisation's apps: on or off, and
/// who may use it (None: the app's own groups).
#[derive(Clone, Debug, Default)]
pub struct ConnectorSetting {
    pub enabled: bool,
    pub access_groups: Option<Vec<String>>,
    pub blocked_groups: Option<Vec<String>>,
}

#[derive(Default)]
pub struct State {
    pub computers: crate::computer::Ledger,
    pub rules: crate::rules::Ledger,
    pub sub_agents: crate::sub_agents::Ledger,
    pub local: crate::local::Ledger,
    /// Trashed items by id.
    pub trash: BTreeMap<String, Trashed>,
    /// Conversations removed from the trash: their tasks stay, the thread is gone.
    pub removed_threads: HashSet<String>,
    pub tasks: BTreeMap<String, Task>,
    pub runs: HashMap<String, Run>,
    pub items: BTreeMap<String, Item>,
    pub approvals: Vec<Approval>,
    pub policies: BTreeMap<String, String>,
    pub kill_switch: bool,
    /// Who last engaged or released it, and when (as the log names them).
    pub kill_switch_by: Option<(String, DateTime<Utc>)>,
    /// Everyone who has ever signed in here. Someone not in it signs in the
    /// first time on the identity provider's own page, which can walk them
    /// through what their account still needs (a new password, two-factor).
    pub signed_in_before: HashSet<String>,
    /// When each person last signed in here (the console's Users list).
    pub last_sign_in: HashMap<String, DateTime<Utc>>,
    /// When each person last opened the app's home: the feed starts
    /// here (#367).
    pub home_last_open: HashMap<String, DateTime<Utc>>,
    /// Watched baselines (#369): the last two answers' item ids per
    /// person, app and tool, newest last. New ids wake a review. A
    /// window, not an accumulation (#409): two answers bound the
    /// memory whatever the source holds, an unchanged answer always
    /// diffs empty against its own window, and an item absent for one
    /// read stays known while one gone for two wakes again on return.
    pub watched_seen: HashMap<(String, String, String), Vec<HashSet<String>>>,
    /// When each watched read was last polled on schedule, per
    /// person, app and tool (#406): the next poll is due an interval
    /// after this, or at once when there is no entry yet.
    pub watch_last_polled: HashMap<(String, String, String), DateTime<Utc>>,
    /// When each inbox item was first presented to its person (#367).
    pub items_opened: HashMap<String, DateTime<Utc>>,
    /// When each inbox item was decided, for the feed (#367).
    pub items_resolved_at: HashMap<String, DateTime<Utc>>,
    /// How many times each person has connected each app (by person, then
    /// app): the first connection is the one Magnus opens a conversation
    /// about, never a reconnection.
    pub app_connections: HashMap<(String, String), u32>,
    /// The conversations Magnus opened, by person then task: each starts
    /// once ever, so deleting one does not start it again (review of #274).
    pub openers: std::collections::BTreeSet<(String, String)>,
    /// People turned off at the identity provider or by an admin, until
    /// they sign in again: their apps answer no calls for them.
    pub turned_off: std::collections::BTreeSet<String>,
    /// The latest backup's restore check: file, whether it restored, detail, when.
    pub last_backup: Option<(String, bool, String, DateTime<Utc>)>,
    /// The latest backup's off-host copy: file, whether it held, detail, when.
    pub last_copy: Option<(String, bool, String, DateTime<Utc>)>,
    /// Removed content some backups still hold, until they are rewritten.
    pub scrub_pending: Option<ScrubPending>,
    /// Each person erased with content, by their id then.
    pub erasures: BTreeMap<String, Erasure>,
    /// An appliance update in progress: from, to, and whether it is a rollback.
    pub updating: Option<(String, String, bool)>,
    pub last_update: Option<Update>,
    /// Releases rolled back from: not installed again by themselves.
    pub skipped_releases: std::collections::BTreeSet<String>,
    /// Apps the owner added from the app (config-file apps are not here).
    pub connectors: BTreeMap<String, crate::config::Connector>,
    /// Providers the organisation has a registration with (its client ID
    /// and secret are in the vault).
    pub registrations: std::collections::BTreeSet<String>,
    /// The admin's settings for each app: on or off, and who may use it.
    pub connector_settings: BTreeMap<String, ConnectorSetting>,
    /// When each organisation credential was last set or revoked: an app's
    /// key by the app's name, a provider's client secret as
    /// `registration:<provider>`. Never the value.
    pub credential_changed: BTreeMap<String, (bool, DateTime<Utc>)>,
    /// What was done in the admin console, oldest first: each entry's seq,
    /// when, and the action as written.
    pub audit: Vec<(i64, DateTime<Utc>, Value)>,
    /// Secrets the kill switch emptied from the vault, latest engagement
    /// only: what the owner signs in to or enters again.
    pub revoked: BTreeMap<String, Revoked>,
    /// Secrets the kill switch kept, sealed: only their provider can revoke them.
    pub sealed: Vec<String>,
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
    /// Entries people deleted from their threads, by thread.
    pub hidden: HashMap<String, std::collections::HashSet<String>>,
    /// Each person's reactions to Magnus's messages, by (thread, entry):
    /// every reaction given, oldest first, and when (None: taken back), so
    /// a run sees them as they stood when it started.
    pub reactions: HashMap<String, BTreeMap<(String, String), Vec<(Option<String>, DateTime<Utc>)>>>,
    /// The disagreements Magnus recorded, by id.
    pub disagreements: BTreeMap<String, Disagreement>,
    /// Threads branched from another, by thread.
    pub branches: HashMap<String, Branch>,
    pub schedules: BTreeMap<String, Schedule>,
    /// Each person's grants (#235), revoked ones among them, by id.
    pub grants: BTreeMap<String, Grant>,
    /// Each person's commitment ledger (#236), by id.
    pub commitments: BTreeMap<String, Commitment>,
    /// Cards from chat (schedules and suggestions) the person answered, by
    /// card id: with the cards in their turns' results, the suggestion
    /// ledger (`suggestions::ledger`).
    pub cards: HashMap<String, CardState>,
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
    pub model: Option<(String, String)>,
    pub pins: HashMap<String, Pin>,
    /// Tools whose live definition differs from the pin, with the open item.
    pub changed: HashMap<String, (String, Pin)>,
    pub secrets: HashSet<String>,
    pub memory: Vec<MemoryEntry>,
    /// The company knowledge base; its searchable index is in Postgres.
    pub knowledge: HashMap<String, KnowledgeEntry>,
    /// Files attached to each task, by task id.
    pub attachments: HashMap<String, Vec<Value>>,
    /// Every person's library, by file id.
    pub files: BTreeMap<String, LibFile>,
    pub projects: BTreeMap<String, Project>,
    /// Items the person moved into a project (Some) or out of one (None).
    pub moved: HashMap<String, Option<String>>,
    /// Latest conformance result per adapter id: (version, model, passed, report).
    pub conformance: HashMap<String, (String, String, bool, Value)>,
    pub playbook: BTreeMap<String, PlaybookItem>,
    /// The org context bundle, by path.
    pub bundle: BTreeMap<String, BundleDoc>,
    /// The last version number each path was written with, kept through a
    /// removal: a path written again counts on, so (path, version) names
    /// one version in the log.
    pub bundle_last: HashMap<String, u32>,
    /// Every policy a layer ever had, oldest first.
    pub policy_history: Vec<PolicyVersion>,
    /// Run modules an admin turned on or off for the organisation.
    pub module_settings: BTreeMap<String, bool>,
    /// Model providers an admin disabled.
    pub providers_off: std::collections::BTreeSet<String>,
    /// The desktop channel for everyone but the owner and testers, when an
    /// admin set one.
    pub desktop_channel: Option<String>,
    /// The admin console's session lifetimes, as an admin set them.
    pub console_settings: Option<Value>,
    /// Admins' drafts of policy layers, not in force.
    pub policy_drafts: BTreeMap<String, String>,
    /// Model and harness changes, for version notices (U5): (seq, at, what, value).
    pub version_changes: Vec<(i64, DateTime<Utc>, String, String)>,
    /// Increments with every playbook change (I3).
    pub playbook_version: u64,
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
    /// the person of its run, item or schedule. `runs` holds the runs of
    /// events written in the same batch, before them. The organisation's
    /// own content (knowledge entries, the playbook) belongs to no one
    /// person: None.
    pub fn content_owner(&self, ev: &crate::events::Ev, runs: &HashMap<String, String>) -> Option<String> {
        use crate::events::Ev;
        match ev {
            Ev::KnowledgeAdded { .. } | Ev::Playbook { .. } => return None,
            // An org document is nobody's; a person's own is theirs.
            Ev::BundleWritten { owner_user, .. } => return owner_user.clone(),
            Ev::ItemResolved { id, .. } => return self.items.get(id).map(|i| i.person.clone()),
            Ev::ItemOpened { id } => return self.items.get(id).map(|i| i.person.clone()),
            Ev::ScheduleEnded { id, .. } => return self.schedules.get(id).map(|s| s.person.clone()),
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
        if let Some(p) = self.moved.get(item) {
            return p.clone().filter(|p| self.projects.contains_key(p));
        }
        if let Some(t) = self.tasks.get(item) {
            return t.project.as_ref().map(|p| p.id.clone());
        }
        if let Some(f) = self.files.get(item) {
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
            (!["timer", "update", "kill_switch"].contains(&reason)).then(|| (t.task_id.clone(), format!("its last run is waiting on you ({reason})")))
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

    /// How many of a schedule's latest firings in a row failed (since it was
    /// last resumed): runs that ended failed, and firings that started none.
    pub fn failures_in_a_row(&self, schedule: &str) -> u32 {
        let Some(sc) = self.schedules.get(schedule) else { return 0 };
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

    /// A model call's cost, charged to the grant the run's task runs
    /// under, if any (#280).
    fn charge_grant(&mut self, run: &str, at: DateTime<Utc>, usd: f64) {
        let grant = self.runs.get(run).and_then(|r| self.tasks.get(&r.task)).and_then(|t| t.grant.clone());
        if let Some(g) = grant.and_then(|g| self.grants.get_mut(&g)) {
            g.charges.push((at, usd));
        }
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
            let branch = self.branches.remove(item);
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
        if let Some(f) = self.files.remove(item) {
            return Some((f.name.clone(), None, Held::File(Box::new(f))));
        }
        if let Some(p) = self.projects.remove(item) {
            return Some((p.name.clone(), None, Held::Project(p)));
        }
        if let Some(i) = self.memory.iter().position(|m| m.id == item) {
            // Its title in the trash is its fact, recalled when listed.
            let m = self.memory.remove(i);
            return Some((String::new(), None, Held::Memory(m)));
        }
        let k = self.knowledge.remove(item)?;
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
        let pending = self.scrub_pending.clone();
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
            self.audit.push((e.seq, e.at, serde_json::json!({ "kind": "admin_action", "admin": "kernel", "name": "Magnus", "action": action, "target": target, "source": null, "before": null, "after": after })));
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
                    self.branches.insert(id, b);
                }
            }
            Held::Entry { .. } => {}
            Held::File(f) => {
                self.files.insert(f.id.clone(), *f);
            }
            Held::Project(p) => {
                self.projects.insert(p.id.clone(), p);
            }
            Held::Memory(m) => {
                let at = self.memory.partition_point(|x| x.at <= m.at);
                self.memory.insert(at, m);
            }
            Held::Knowledge(k) => {
                self.knowledge.insert(k.id.clone(), *k);
            }
        }
    }

    /// A person's file, or a delivered file of a task they may see.
    pub fn file_for(&self, person: &str, owner: bool, id: &str) -> Option<&LibFile> {
        self.files.get(id).filter(|f| f.person == person || (owner && f.run.is_some()))
    }

    pub fn apply(&mut self, e: &Event) {
        self.last_seq = e.seq;
        if let Some(r) = e.ev.run().and_then(|id| self.runs.get_mut(id)) {
            r.last_seq = e.seq;
            r.last_at = e.at;
        }
        match &e.ev {
            Ev::KernelBoot { version, instructions, .. } => {
                for (what, v) in [("harness", version), ("instructions", instructions)] {
                    if !v.is_empty() && self.version_changes.iter().rev().find(|c| c.2 == what).is_none_or(|c| &c.3 != v) {
                        self.version_changes.push((e.seq, e.at, what.into(), v.clone()));
                    }
                }
            }
            Ev::SecretSet { name } => {
                self.secrets.insert(name.clone());
                self.revoked.remove(name);
            }
            Ev::PolicySet { layer, cedar } => {
                self.policies.insert(layer.clone(), cedar.clone());
                self.policy_history.push(PolicyVersion { layer: layer.clone(), seq: e.seq, at: e.at, by: e.actor.clone(), cedar: cedar.clone() });
                let n = self.policy_history.iter().filter(|p| &p.layer == layer).count();
                self.version_changes.push((e.seq, e.at, format!("policy:{layer}"), format!("v{n}")));
            }
            Ev::ModuleSet { name, on } => match on {
                Some(on) => {
                    self.module_settings.insert(name.clone(), *on);
                }
                None => {
                    self.module_settings.remove(name);
                }
            },
            Ev::ProviderSet { id, enabled } => {
                if *enabled {
                    self.providers_off.remove(id);
                } else {
                    self.providers_off.insert(id.clone());
                }
            }
            Ev::DesktopChannelSet { channel } => {
                self.desktop_channel = Some(channel.clone());
            }
            Ev::ConsoleSettings { idle_minutes, session_hours, allow } => {
                self.console_settings = Some(serde_json::json!({ "idle_minutes": idle_minutes, "session_hours": session_hours, "allow": allow }));
            }
            Ev::PolicyDrafted { layer, cedar } => match cedar {
                Some(c) => {
                    self.policy_drafts.insert(layer.clone(), c.clone());
                }
                None => {
                    self.policy_drafts.remove(layer);
                }
            },
            Ev::Computer(c) => self.computers.apply(c),
            Ev::Rule(c) => self.rules.apply(c),
            Ev::SubAgent(c) => self.sub_agents.apply(c),
            Ev::LocalComputer(c) => self.local.apply(c),
            Ev::Handback { run, step, image_b64 } => self.add_frame(run, *step, image_b64),
            Ev::KillSwitch { engaged } => {
                self.kill_switch = *engaged;
                self.kill_switch_by = Some((e.actor.clone(), e.at));
            }
            Ev::AdminAction { .. } => {
                if let Ok(v) = serde_json::to_value(&e.ev) {
                    self.audit.push((e.seq, e.at, v));
                }
            }
            Ev::BackupMade { .. } => {}
            Ev::UpdateStarted { from, to, rollback } => self.updating = Some((from.clone(), to.clone(), *rollback)),
            Ev::UpdateFinished { from, to } => {
                self.updating = None;
                self.last_update = Some(Update { what: "updated".into(), from: from.clone(), to: to.clone(), detail: String::new(), at: e.at });
            }
            Ev::UpdateRolledBack { from, to, reason } => {
                self.updating = None;
                self.skipped_releases.insert(from.clone());
                self.last_update = Some(Update { what: "rolled_back".into(), from: from.clone(), to: to.clone(), detail: reason.clone(), at: e.at });
            }
            Ev::UpdateFailed { to, reason } => {
                self.updating = None;
                self.last_update = Some(Update { what: "failed".into(), from: String::new(), to: to.clone(), detail: reason.clone(), at: e.at });
            }
            Ev::BackupChecked { file, ok, detail } => {
                self.last_backup = Some((file.clone(), *ok, detail.clone(), e.at));
            }
            Ev::BackupCopied { file, ok, detail } => {
                self.last_copy = Some((file.clone(), *ok, detail.clone(), e.at));
            }
            Ev::BackupScrubPending { from, through, files, detail } => {
                self.scrub_pending = Some(ScrubPending { from: *from, through: *through, files: files.clone(), detail: detail.clone(), at: e.at });
                self.erasures_pending(e);
            }
            Ev::BackupsScrubbed { through } => {
                if self.scrub_pending.as_ref().is_some_and(|p| p.through <= *through) {
                    self.scrub_pending = None;
                }
                self.erasures_pending(e);
            }
            Ev::ConnectorAdded { connector } => {
                if connector.credential.is_some() && connector.auth.as_deref() != Some(crate::connectors::OAUTH) {
                    self.credential_changed.insert(connector.name.clone(), (true, e.at));
                }
                self.connectors.insert(connector.name.clone(), connector.clone());
            }
            Ev::ConnectorSettings { name, enabled, access_groups, blocked_groups } => {
                self.connector_settings.insert(name.clone(), ConnectorSetting { enabled: *enabled, access_groups: access_groups.clone(), blocked_groups: blocked_groups.clone() });
            }
            // The connection itself is in the vault; these are its record.
            Ev::AppConnected { app, person, .. } => {
                *self.app_connections.entry((person.clone(), app.clone())).or_default() += 1;
            }
            Ev::AppDisconnected { .. } => {}
            Ev::CredentialSet { connector, set } => {
                self.credential_changed.insert(connector.clone(), (*set, e.at));
            }
            Ev::ConnectorRemoved { name } => {
                self.connectors.remove(name);
            }
            Ev::RegistrationSet { provider, set } => {
                self.credential_changed.insert(format!("registration:{provider}"), (*set, e.at));
                if *set {
                    self.registrations.insert(provider.clone());
                } else {
                    self.registrations.remove(provider);
                }
            }
            Ev::ToolClassified { connector, tool, risk } => {
                if let Some(c) = self.connectors.get_mut(connector) {
                    match risk {
                        Some(r) => {
                            // Its reach is what the tool is, whatever its class.
                            let reach = c.tools.get(tool).and_then(|t| t.reach.clone());
                            c.tools.insert(tool.clone(), crate::config::ToolClass { risk: r.clone(), label: None, reach });
                        }
                        None => {
                            c.tools.remove(tool);
                        }
                    }
                }
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
            Ev::VaultSealed { revoking, kept } => {
                self.revoked = revoking.iter().map(|n| (n.clone(), Revoked::default())).collect();
                self.sealed = kept.clone();
                for n in revoking {
                    self.secrets.remove(n);
                }
            }
            Ev::SecretRevoked { name, adapter, at_provider, detail } => {
                self.revoked.insert(name.clone(), Revoked { adapter: Some(adapter.clone()), at_provider: Some(*at_provider), detail: Some(detail.clone()) });
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
            Ev::PersonErased { person } => {
                let runs: HashSet<String> = self.runs.values().filter(|r| &r.person == person).map(|r| r.id.clone()).collect();
                let tasks: HashSet<String> = self.tasks.values().filter(|t| &t.person == person).map(|t| t.task_id.clone()).collect();
                let their_items: HashSet<String> = self.items.values().filter(|i| &i.person == person).map(|i| i.id.clone()).collect();
                self.tasks.retain(|_, t| &t.person != person);
                self.runs.retain(|_, r| &r.person != person);
                self.items.retain(|_, i| &i.person != person && i.run.as_ref().is_none_or(|r| !runs.contains(r)));
                // Their items' opened and resolved marks go with the items (#412).
                self.items_opened.retain(|id, _| !their_items.contains(id));
                self.items_resolved_at.retain(|id, _| !their_items.contains(id));
                self.schedules.retain(|_, x| &x.person != person);
                // Their grants: none covers whoever is next known by their name.
                self.grants.retain(|_, g| &g.person != person);
                // Their commitments, whose words went with theirs (#236).
                self.commitments.retain(|_, c| &c.person != person);
                self.memory.retain(|m| &m.person != person);
                // Their own documents (their relationship memory among
                // them) and their versions, whose content went with theirs
                // (org-context.md, the owner's ruling on erasure). A path
                // counts its versions on, as through any removal.
                self.bundle.retain(|_, d| d.owner_user.as_deref() != Some(person.as_str()));
                self.turned_off.remove(person);
                self.files.retain(|_, f| &f.person != person);
                self.projects.retain(|_, x| &x.person != person);
                self.trash.retain(|_, t| &t.person != person);
                self.preferences.remove(person);
                self.hidden.remove(person);
                // Their reactions and disagreements, whose words went with theirs.
                self.reactions.remove(person);
                self.disagreements.retain(|_, d| &d.person != person);
                // Their answers to cards, a decline's reason among them.
                self.cards.retain(|_, c| c.person.as_ref() != Some(person));
                // What their drafts were written from.
                self.draft_sources.retain(|_, d| &d.person != person);
                // The proactive loop's scheduling for them.
                self.reviews_last.remove(person);
                self.reviews_overnight.remove(person);
                self.interrupts.remove(person);
                self.review_runs.retain(|_, p| p != person);
                self.wake_marks.retain(|k, _| k.split_once(':').is_some_and(|(p, _)| p != person));
                // Their branched threads (the entries as shown then) and
                // what their tasks had attached go too.
                self.branches.retain(|_, b| &b.person != person);
                self.attachments.retain(|t, _| !tasks.contains(t));
                self.subjects.remove(person);
                self.last_sign_in.remove(person);
                self.app_connections.retain(|(p, _), _| p != person);
                self.openers.retain(|(p, _)| p != person);
                // Their watched baselines and home mark: whoever is next
                // known by their name starts fresh (#412).
                self.watched_seen.retain(|(p, _, _), _| p != person);
                self.home_last_open.remove(person);
                // Their poll clocks: whoever is next known by their
                // name starts polling fresh (#406).
                self.watch_last_polled.retain(|(p, _, _), _| p != person);
                // The rights they held go with them: whoever is next known by
                // their name starts from the organisation's config.
                self.roles_set.remove(person);
                self.provider_rights.remove(person);
                // So do the approvals given for their actions: none covers
                // whoever is next known by their name (#191). Those they gave
                // others stay, as given; the gate still asks whether their
                // approver may give a lasting one.
                self.approvals.retain(|a| &a.person != person);
            }
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
            }
            Ev::ChatHidden { thread, entries, .. } => {
                self.hidden.entry(thread.clone()).or_default().extend(entries.iter().cloned());
            }
            Ev::Reacted { person, thread, entry, reaction } => {
                let mine = self.reactions.entry(person.clone()).or_default();
                mine.entry((thread.clone(), entry.clone())).or_default().push((reaction.clone(), e.at));
            }
            Ev::DisagreementRecorded { id, person, position, decision, .. } => {
                let d = Disagreement { id: id.clone(), person: person.clone(), position: position.clone(), decision: decision.clone(), at: e.at, outcome: None, outcome_at: None };
                self.disagreements.insert(id.clone(), d);
            }
            Ev::DisagreementOutcome { id, outcome, .. } => {
                // The first outcome stands.
                if let Some(d) = self.disagreements.get_mut(id).filter(|d| d.outcome.is_none()) {
                    d.outcome = Some(outcome.clone());
                    d.outcome_at = Some(e.at);
                }
            }
            Ev::ThreadBranched { person, thread, from, upto, entries, title } => {
                self.branches
                    .insert(thread.clone(), Branch { person: person.clone(), from: from.clone(), upto: upto.clone(), entries: entries.clone(), title: title.clone(), at: e.at });
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
            Ev::WatchedSeen { person, app, tool, ids, full } => {
                let window = self.watched_seen.entry((person.clone(), app.clone(), tool.clone())).or_default();
                let seen: HashSet<String> = ids.iter().cloned().collect();
                // Tagged wholes push as they come (#415). Untagged history
                // splits by overlap (#423): era-1 deltas are fresh-only,
                // hence always disjoint from the window, and union onto
                // the newest entry they extend; an overlapping untagged
                // event is an era-2 whole answer and pushes. A total-churn
                // era-2 pair is indistinguishable from seed plus delta and
                // unions transiently, healed by later tagged reads.
                let entry = if *full || window.iter().flatten().any(|id| seen.contains(id)) {
                    seen
                } else {
                    let mut union = window.last().cloned().unwrap_or_default();
                    union.extend(seen);
                    union
                };
                window.push(entry);
                if window.len() > 2 {
                    window.remove(0);
                }
            }
            Ev::WatchPolled { person, app, tool } => {
                self.watch_last_polled.insert((person.clone(), app.clone(), tool.clone()), e.at);
            }
            Ev::ItemOpened { id } => {
                // First presentation stands: opened is when it was
                // first seen, and the app reports it on every open.
                self.items_opened.entry(id.clone()).or_insert(e.at);
            }
            Ev::SignedOut { .. } | Ev::DevicePaired { .. } | Ev::DeviceRevoked { .. } => {}
            Ev::PersonTurnedOff { person, .. } => {
                self.turned_off.insert(person.clone());
            }
            Ev::ScheduleSet { id, person, every_minutes, daily_at, budget, card, label, until, max_runs, .. } => {
                self.schedules.insert(
                    id.clone(),
                    Schedule {
                        id: id.clone(),
                        person: person.clone(),
                        every_minutes: *every_minutes,
                        daily_at: daily_at.clone(),
                        created: e.at,
                        last_fired: None,
                        budget: budget.clone(),
                        card: card.clone(),
                        label: label.unwrap_or_default(),
                        seq: e.seq,
                        failures: vec![],
                        until: *until,
                        max_runs: *max_runs,
                        started: 0,
                        skipped: None,
                        paused: None,
                        paused_on: None,
                        paused_by_admin: false,
                        resumed: None,
                        ended: None,
                    },
                );
                if let Some(c) = card {
                    self.cards.insert(c.clone(), CardState { answer: Answer::Confirmed(id.clone()), at: e.at, person: Some(person.clone()) });
                }
            }
            Ev::GrantSet { id, person, apps, allow, period, max_usd, card, cedar, label, .. } => {
                let g = self.grants.entry(id.clone()).or_insert_with(|| Grant {
                    id: id.clone(),
                    person: person.clone(),
                    apps: vec![],
                    allow: vec![],
                    period: String::new(),
                    max_usd: 0.0,
                    cedar: String::new(),
                    card: card.clone(),
                    label: Label::PRINCIPAL,
                    seq: e.seq,
                    at: e.at,
                    by: e.actor.clone(),
                    edited: None,
                    revoked: None,
                    actions: vec![],
                    charges: vec![],
                });
                if g.seq != e.seq {
                    g.edited = Some(e.at);
                }
                g.apps = apps.clone();
                g.allow = allow.clone();
                g.period = period.clone();
                g.max_usd = *max_usd;
                g.cedar = cedar.clone();
                g.label = label.unwrap_or_default();
                g.seq = e.seq;
                // A card accepted sets it; a proposal handled with it is
                // answered by the task it started, which follows.
                if let Some(c) = card.as_ref().filter(|c| !self.cards.contains_key(*c)) {
                    self.cards.insert(c.clone(), CardState { answer: Answer::Granted(id.clone()), at: e.at, person: Some(person.clone()) });
                }
            }
            Ev::GrantRevoked { id, .. } => {
                if let Some(g) = self.grants.get_mut(id).filter(|g| g.revoked.is_none()) {
                    g.revoked = Some(e.at);
                }
            }
            Ev::GrantUsed { grant, task, run, call_id, tool } => {
                if let Some(g) = self.grants.get_mut(grant) {
                    g.actions.push(GrantAction { tool: tool.clone(), task: task.clone(), run: run.clone(), call_id: call_id.clone(), at: e.at });
                }
            }
            Ev::CommitmentRecorded { id, person, who, by, source, label, .. } => {
                let by = by.clone().filter(|b| !b.is_empty());
                self.commitments.entry(id.clone()).or_insert_with(|| Commitment {
                    id: id.clone(),
                    person: person.clone(),
                    who: who.clone(),
                    source: source.clone(),
                    label: *label,
                    at: e.at,
                    by_actor: e.actor.clone(),
                    changed: e.at,
                    seq: e.seq,
                    status: "open".into(),
                    by: by.clone(),
                    steps: vec![CommitmentStep { seq: e.seq, status: "open".into(), by }],
                });
            }
            Ev::CommitmentChanged { id, person, by, status, .. } => {
                if let Some(c) = self.commitments.get_mut(id).filter(|c| &c.person == person) {
                    if let Some(b) = by {
                        c.by = Some(b.clone()).filter(|b| !b.is_empty());
                    }
                    if let Some(s) = status {
                        c.status = s.clone();
                    }
                    c.changed = e.at;
                    c.steps.push(CommitmentStep { seq: e.seq, status: c.status.clone(), by: c.by.clone() });
                }
            }
            Ev::CommitmentRemoved { id, person } => {
                if self.commitments.get(id).is_some_and(|c| &c.person == person) {
                    self.commitments.remove(id);
                }
            }
            Ev::CardDeclined { card, person, reason } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Declined(reason.clone()), at: e.at, person: person.clone() });
            }
            Ev::CardDeferred { card, person } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Later, at: e.at, person: person.clone() });
            }
            Ev::CardAccepted { card, person, task } => {
                self.cards.insert(card.clone(), CardState { answer: Answer::Started(task.clone()), at: e.at, person: Some(person.clone()) });
            }
            Ev::MemoryForgotten { id } => self.memory.retain(|m| &m.id != id),
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
                match self.trash.remove(item).map(|t| t.held) {
                    Some(Held::Entry { thread }) => {
                        self.hidden.entry(thread).or_default().insert(item.clone());
                    }
                    Some(Held::Thread { id, .. }) => {
                        self.removed_threads.insert(id);
                    }
                    _ => {}
                }
            }
            Ev::KnowledgeAdded { id, title, readers, integrity, author, run, .. } => {
                self.knowledge.insert(
                    id.clone(),
                    KnowledgeEntry {
                        id: id.clone(),
                        title: title.clone(),
                        readers: readers.clone(),
                        integrity: integrity.clone(),
                        author: author.clone(),
                        run: run.clone(),
                        endorsed: None,
                        shared: None,
                        at: e.at,
                        version: e.seq,
                        seq: e.seq,
                    },
                );
            }
            Ev::KnowledgeShared { id, readers, by } => {
                if let Some(k) = self.knowledge.get_mut(id) {
                    k.readers = readers.clone();
                    k.shared = Some(serde_json::json!({ "by": by, "at": e.at }));
                    k.version = e.seq;
                }
            }
            Ev::KnowledgeEndorsed { id, by, item, note } => {
                if let Some(k) = self.knowledge.get_mut(id) {
                    k.integrity = "company".into();
                    k.endorsed = Some(serde_json::json!({ "by": by, "item": item, "note": note, "at": e.at }));
                    k.version = e.seq;
                }
            }
            Ev::ScheduleRemoved { id, .. } => {
                self.schedules.remove(id);
            }
            Ev::ScheduleEnded { id, reason, .. } => {
                // It stays, marked with why: its card says it ended, with
                // its latest run. Only a cancelled one is gone (#264). The
                // end is terminal: no pause outlives it.
                if let Some(s) = self.schedules.get_mut(id) {
                    s.ended = Some(reason.clone());
                    s.paused = None;
                    s.paused_on = None;
                    s.paused_by_admin = false;
                }
            }
            Ev::ScheduleSkipped { id, task, reason } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.last_fired = Some(e.at);
                    s.skipped = Some((e.at, task.clone(), reason.clone()));
                }
            }
            Ev::SchedulePaused { id, reason, task, admin, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.paused = Some(reason.clone());
                    s.paused_on = task.clone();
                    s.paused_by_admin = *admin;
                }
            }
            Ev::ScheduleChanged { id, every_minutes, daily_at, until, max_runs, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.every_minutes = *every_minutes;
                    s.daily_at = daily_at.clone();
                    s.until = *until;
                    s.max_runs = *max_runs;
                    // A new end puts an ended one back: edits only move the
                    // end forward, so it runs again (#264).
                    s.ended = None;
                }
            }
            Ev::ScheduleResumed { id, .. } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.paused = None;
                    s.paused_by_admin = false;
                    // Failures count afresh only after a person resumes a
                    // schedule paused for them.
                    if s.paused_on.take().is_none() {
                        s.resumed = Some(e.at);
                    }
                    // The next run is due from now, not from long ago.
                    s.last_fired = Some(e.at);
                }
            }
            // A firing that failed still counts as fired: the next one is due as usual.
            Ev::ScheduleFailed { id, reason } => {
                if let Some(s) = self.schedules.get_mut(id) {
                    s.last_fired = Some(e.at);
                    s.failures.push((e.at, reason.clone()));
                }
            }
            Ev::ScheduleFired { id, task } => {
                // A run its schedule created was counted when it was created
                // (below), so a firing lost after it never lets one more run.
                let counted = self.tasks.get(task).is_some_and(|t| t.schedule.as_deref() == Some(id.as_str()));
                if let Some(s) = self.schedules.get_mut(id) {
                    s.last_fired = Some(e.at);
                    if !counted {
                        s.started += 1;
                    }
                    s.skipped = None;
                }
                if let Some(t) = self.tasks.get_mut(task) {
                    t.schedule = Some(id.clone());
                }
            }
            Ev::ModelSet { adapter, model } => {
                self.model = Some((adapter.clone(), model.clone()));
                self.version_changes.push((e.seq, e.at, "model".into(), format!("{adapter}/{model}")));
            }
            Ev::ToolPinned { tool, hash, description, input_schema, approved } => {
                self.pins.insert(tool.clone(), Pin { hash: hash.clone(), description: description.clone(), input_schema: input_schema.clone(), approved: *approved });
                self.changed.remove(tool);
            }
            Ev::ToolChanged { tool, hash, description, input_schema, item } => {
                self.changed
                    .insert(tool.clone(), (item.clone(), Pin { hash: hash.clone(), description: description.clone(), input_schema: input_schema.clone(), approved: false }));
            }
            Ev::TaskCreated { task, run, person, config, attachments, experiment, chat, label, files, project, grant, .. } => {
                // Playbook items the run starts with carry the taint of the
                // runs that wrote them (I2, M1), and a task from chat that of
                // the turn that started it.
                let pb = self.playbook_at(person, config.playbook).into_iter().fold(label.unwrap_or_default(), |l, i| l.join(i.label));
                // So do the person's open commitments, which its context
                // carries as they stand now (#236): words read in a mailbox
                // do not enter a clean run untainted.
                let pb = self.commitments.values().filter(|c| &c.person == person && c.status == "open").fold(pb, |l, c| l.join(c.label));
                let uploads = !attachments.is_empty() || !files.is_empty();
                // A schedule's run counts against it as it is created.
                if let Some(s) = e.actor.strip_prefix("schedule:").and_then(|id| self.schedules.get_mut(id)) {
                    s.last_fired = Some(e.at);
                    s.started += 1;
                    s.skipped = None;
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
                        actions: 0,
                        usd: 0.0,
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
                    if matches!(&e.ev, Ev::ReviewerReply { .. }) {
                        r.reviewed += 1;
                    }
                    r.model_calls += 1;
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
            Ev::WorkStarted { run } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.work_started = true;
                }
            }
            Ev::ContractFrozen { run, version } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.contract_frozen = Some(*version);
                }
            }
            Ev::Plan { run, version, plan } => {
                if let Some(r) = self.runs.get_mut(run) {
                    r.plan = Some(plan.clone());
                    r.plan_version = *version;
                }
            }
            Ev::Playbook { item, op, person, run, text, addresses, label } => {
                self.playbook_version += 1;
                let v = self.playbook_version;
                self.version_changes.push((e.seq, e.at, "playbook".into(), format!("v{v}")));
                match op.as_str() {
                    "add" => {
                        self.playbook.insert(
                            item.clone(),
                            PlaybookItem {
                                id: item.clone(),
                                person: person.clone(),
                                text: text.clone().unwrap_or_default(),
                                addresses: addresses.clone(),
                                runs: vec![run.clone()],
                                label: *label,
                                added: v,
                                retired: None,
                                added_at: e.at,
                            },
                        );
                    }
                    "edit" => {
                        if let Some(i) = self.playbook.get_mut(item) {
                            if let Some(t) = text {
                                i.text = t.clone();
                            }
                            if addresses.is_some() {
                                i.addresses = addresses.clone();
                            }
                            i.runs.push(run.clone());
                            i.label = i.label.join(*label);
                        }
                    }
                    "retire" => {
                        if let Some(i) = self.playbook.get_mut(item) {
                            i.retired = Some(v);
                        }
                    }
                    _ => {}
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
                        resolution: None,
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
            Ev::Approval { id, item, person, task, tool, input_hash, scope, until, reason, risk } => self.approvals.push(Approval {
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
                    self.files.insert(id.clone(), f);
                }
            }
            Ev::FileUploaded { id, person, name, media_type, data_b64, thread } => {
                let f = LibFile {
                    id: id.clone(),
                    person: person.clone(),
                    name: name.clone(),
                    media_type: media_type.clone(),
                    size: data_b64.len() * 3 / 4,
                    at: e.at,
                    uploaded: true,
                    seq: e.seq,
                    run: None,
                    thread: thread.clone(),
                    label: Label { untrusted: true, private: true },
                };
                self.files.insert(id.clone(), f);
            }
            Ev::ProjectSet { id, person, name, instructions } => {
                let created_at = self.projects.get(id).map_or(e.at, |p| p.created_at);
                self.projects.insert(id.clone(), Project { id: id.clone(), person: person.clone(), name: name.clone(), instructions: instructions.clone(), created_at });
            }
            Ev::Moved { item, project } => {
                self.moved.insert(item.clone(), project.clone());
            }
            Ev::AdapterVerified { adapter_id, version, model, passed, report, .. } => {
                self.conformance.insert(adapter_id.clone(), (version.clone(), model.clone(), *passed, report.clone()));
            }
            Ev::BundleWritten { path, version, doc_kind: kind, scope, owner_user, actor, run, label, sources, writer: named, .. } => {
                let writer = named.clone().or_else(|| run.as_ref().and_then(|r| self.runs.get(r)).map(|r| r.person.clone()));
                let d = self.bundle.entry(path.clone()).or_insert_with(|| BundleDoc {
                    path: path.clone(),
                    kind: kind.clone(),
                    scope: scope.clone(),
                    owner_user: owner_user.clone(),
                    label: *label,
                    sources: vec![],
                    versions: vec![],
                    writer: None,
                    shared: vec![],
                    shared_everyone: false,
                    shared_by: None,
                });
                // An admin's edit (no run, no writer named) keeps the writer
                // whose run read its sources (K2).
                if run.is_some() || named.is_some() {
                    d.writer = writer;
                }
                d.kind = kind.clone();
                d.scope = scope.clone();
                d.owner_user = owner_user.clone();
                d.label = *label;
                d.sources = sources.clone();
                d.versions.push((*version, e.seq, e.at, actor.clone()));
                // A new version is new text: an earlier share does not cover it.
                d.shared.clear();
                d.shared_everyone = false;
                d.shared_by = None;
                let last = self.bundle_last.entry(path.clone()).or_default();
                *last = (*last).max(*version);
            }
            Ev::BundleShared { path, add, everyone, by } => {
                if let Some(d) = self.bundle.get_mut(path) {
                    for p in add {
                        if !d.shared.contains(p) {
                            d.shared.push(p.clone());
                        }
                    }
                    d.shared_everyone |= *everyone;
                    d.shared_by = Some(by.clone());
                }
            }
            Ev::BundleRemoved { path } => {
                self.bundle.remove(path);
            }
            Ev::Memory { id, person, run, label, sources, by, .. } => self.memory.push(MemoryEntry {
                id: id.clone(),
                person: person.clone(),
                run: run.clone(),
                label: *label,
                sources: sources.clone(),
                by: by.clone(),
                at: e.at,
                seq: e.seq,
            }),
        }
    }

    /// The person's playbook items that were live at playbook version `v`.
    pub fn playbook_at(&self, person: &str, v: u64) -> Vec<&PlaybookItem> {
        self.playbook.values().filter(|i| i.person == person && i.added <= v && i.retired.is_none_or(|r| r > v)).collect()
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

    /// A daily time in Los Angeles across both of its clock changes (owner's
    /// CI, 2026-11-01: schedules made in the repeated hour did not fire).
    #[test]
    fn a_daily_schedule_is_due_once_a_day_across_clock_changes() {
        use chrono::{TimeZone, Utc};
        let la = chrono_tz::America::Los_Angeles;
        let utc = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        // Clocks go back at 02:00 PDT on 2026-11-01; 01:00-02:00 comes twice.
        // Made in the first 01:14 (PDT), due at the first 01:16.
        assert_eq!(super::next_daily(utc("2026-11-01T08:14:00Z"), false, 1, 16, &la), Some(utc("2026-11-01T08:16:00Z")));
        // Made in the second 01:14 (PST), due at the second 01:16, not tomorrow.
        assert_eq!(super::next_daily(utc("2026-11-01T09:14:00Z"), false, 1, 16, &la), Some(utc("2026-11-01T09:16:00Z")));
        // Having fired at the first 01:16, not again at the second: tomorrow.
        assert_eq!(super::next_daily(utc("2026-11-01T08:16:00Z"), true, 1, 16, &la), Some(utc("2026-11-02T09:16:00Z")));
        // Fired the day before: the first 01:16.
        assert_eq!(super::next_daily(utc("2026-10-31T08:16:00Z"), true, 1, 16, &la), Some(utc("2026-11-01T08:16:00Z")));
        // Clocks go forward at 02:00 PST on 2027-03-14; 02:30 does not happen.
        // Due as they resume, at 03:30 PDT, and on the next day at 02:30 again.
        assert_eq!(super::next_daily(utc("2027-03-14T09:00:00Z"), false, 2, 30, &la), Some(utc("2027-03-14T10:30:00Z")));
        assert_eq!(super::next_daily(utc("2027-03-14T10:30:00Z"), true, 2, 30, &la), Some(utc("2027-03-15T09:30:00Z")));
        // Whatever order the time zone gives the two in (chrono's `Local`
        // gives the later first; on the owner's build box, the first 01:31
        // came out an hour late).
        let rev = Reversed(la);
        assert_eq!(super::next_daily(utc("2026-11-01T08:29:00Z"), false, 1, 31, &rev), Some(utc("2026-11-01T08:31:00Z")));
        assert_eq!(super::next_daily(utc("2026-11-01T09:29:00Z"), false, 1, 31, &rev), Some(utc("2026-11-01T09:31:00Z")));
        assert_eq!(super::next_daily(utc("2026-11-01T08:31:00Z"), true, 1, 31, &rev), Some(utc("2026-11-02T09:31:00Z")));
        // An ordinary day.
        assert_eq!(super::next_daily(la.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap().with_timezone(&Utc), false, 8, 0, &la), Some(utc("2026-06-02T15:00:00Z")));
    }

    /// A time zone that gives a repeated time's two instants latest first,
    /// as chrono's `Local` does.
    #[derive(Clone, Copy)]
    struct Reversed(chrono_tz::Tz);
    impl chrono::TimeZone for Reversed {
        type Offset = <chrono_tz::Tz as chrono::TimeZone>::Offset;
        fn from_offset(o: &Self::Offset) -> Self {
            Reversed(chrono_tz::Tz::from_offset(o))
        }
        fn offset_from_local_date(&self, d: &chrono::NaiveDate) -> chrono::LocalResult<Self::Offset> {
            self.0.offset_from_local_date(d)
        }
        fn offset_from_local_datetime(&self, d: &chrono::NaiveDateTime) -> chrono::LocalResult<Self::Offset> {
            match self.0.offset_from_local_datetime(d) {
                chrono::LocalResult::Ambiguous(a, b) => chrono::LocalResult::Ambiguous(b, a),
                r => r,
            }
        }
        fn offset_from_utc_date(&self, d: &chrono::NaiveDate) -> Self::Offset {
            self.0.offset_from_utc_date(d)
        }
        fn offset_from_utc_datetime(&self, d: &chrono::NaiveDateTime) -> Self::Offset {
            self.0.offset_from_utc_datetime(d)
        }
    }

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
        assert_eq!(s.schedules["s1"].started, 1);
        assert!(s.schedules["s1"].next_due().is_none(), "a lost firing let the schedule run again");
        // And when the firing is recorded, it is not counted twice.
        s.apply(&ev(3, "kernel", json!({ "kind": "schedule_fired", "id": "s1", "task": "t1" })));
        assert_eq!(s.schedules["s1"].started, 1);
    }

    /// An admin's suspension marks the schedule until a resume clears it;
    /// pauses written before suspensions existed replay unmarked (#262).
    #[test]
    fn an_admin_suspension_marks_the_schedule_until_resumed() {
        let mut s = State::default();
        s.apply(&ev(1, "person:alice", json!({ "kind": "schedule_set", "id": "s1", "person": "alice", "text": "Say hi.", "every_minutes": 60, "max_runs": 1 })));
        s.apply(&ev(2, "kernel", json!({ "kind": "schedule_paused", "id": "s1", "reason": "its last 3 runs failed" })));
        assert!(!s.schedules["s1"].paused_by_admin, "an old pause replayed as an admin's suspension");
        s.apply(&ev(3, "kernel", json!({ "kind": "schedule_resumed", "id": "s1" })));
        s.apply(&ev(4, "admin:root", json!({ "kind": "schedule_paused", "id": "s1", "reason": "Suspended by Root, an admin", "admin": true })));
        assert!(s.schedules["s1"].paused_by_admin, "the suspension left no mark");
        s.apply(&ev(5, "admin:root", json!({ "kind": "schedule_resumed", "id": "s1" })));
        assert!(!s.schedules["s1"].paused_by_admin, "the resume kept the mark");
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
        assert!(!s.branches.contains_key("b-bob"), "bob's branch kept his words");
        assert!(!s.attachments.contains_key("t-bob"), "bob's task kept what it had attached");
        assert!(s.branches.contains_key("b-alice") && s.attachments.contains_key("t-alice"), "alice's went with bob's");
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
        let sc = s.schedules.get("s1").expect("an ended schedule was removed");
        assert_eq!(sc.ended.as_deref(), Some("it ran 1 time, as set"), "the end kept no reason");
        assert!(sc.paused.is_none(), "the end kept the pause");
        assert!(sc.finished(chrono::Utc::now()).is_none(), "an ended schedule finishes again");
        // An edit puts it back: a new end, no mark.
        s.apply(&ev(4, "person:alice", json!({ "kind": "schedule_changed", "id": "s1", "every_minutes": 60, "daily_at": null, "until": null, "max_runs": 2 })));
        assert!(s.schedules["s1"].ended.is_none(), "the edit kept the end mark");
        // Cancelling removes it, as before.
        s.apply(&ev(5, "person:alice", json!({ "kind": "schedule_removed", "id": "s1" })));
        assert!(!s.schedules.contains_key("s1"), "a cancelled schedule stayed");
    }

    /// Pre-#411 history holds fresh-only ids after the seed (#415):
    /// replay unions each onto the newest entry, reconstructing the
    /// answers instead of misreading deltas as wholes.
    #[test]
    fn old_delta_events_replay_as_full_answers() {
        let mut s = State::default();
        let w = |ids: Vec<&str>| json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids });
        s.apply(&ev(1, "person:bob", w(vec!["a"])));
        s.apply(&ev(2, "person:bob", w(vec!["b"])));
        s.apply(&ev(3, "person:bob", w(vec!["c"])));
        let window = &s.watched_seen[&("bob".to_string(), "mail".to_string(), "list_inbox".to_string())];
        assert_eq!(window.len(), 2, "replay kept {window:?}");
        assert_eq!(window[0], std::collections::HashSet::from(["a".to_string(), "b".to_string()]), "the middle answer lost its seed");
        assert_eq!(window[1], std::collections::HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]), "the last answer lost its history");
    }

    /// New events carry whole answers (#415): replay pushes them as they
    /// come, with no unioning.
    #[test]
    fn full_answer_events_replay_whole() {
        let mut s = State::default();
        let w = |ids: Vec<&str>| json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids, "full": true });
        s.apply(&ev(1, "person:bob", w(vec!["a"])));
        s.apply(&ev(2, "person:bob", w(vec!["b"])));
        let window = &s.watched_seen[&("bob".to_string(), "mail".to_string(), "list_inbox".to_string())];
        assert_eq!(window.len(), 2, "replay kept {window:?}");
        assert_eq!(window[0], std::collections::HashSet::from(["a".to_string()]));
        assert_eq!(window[1], std::collections::HashSet::from(["b".to_string()]));
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
        assert!(!s.watched_seen.is_empty() && !s.home_last_open.is_empty() && !s.items_opened.is_empty() && !s.items_resolved_at.is_empty(), "the setup left nothing to erase");
        s.apply(&ev(6, "kernel", json!({ "kind": "person_erased", "person": "bob" })));
        assert!(s.watched_seen.is_empty(), "the erased baseline stayed: {:?}", s.watched_seen.keys().collect::<Vec<_>>());
        assert!(!s.home_last_open.contains_key("bob"), "the erased home mark stayed");
        assert!(!s.items_opened.contains_key("i1"), "the erased opened mark stayed");
        assert!(!s.items_resolved_at.contains_key("i1"), "the erased resolved mark stayed");
    }

    /// Era-2 history (whole answers, untagged, #423): an untagged event
    /// overlapping the window pushes whole instead of unioning.
    #[test]
    fn overlapping_untagged_events_replay_whole() {
        let mut s = State::default();
        let w = |ids: Vec<&str>| json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids });
        s.apply(&ev(1, "person:bob", w(vec!["a"])));
        s.apply(&ev(2, "person:bob", w(vec!["a", "b"])));
        s.apply(&ev(3, "person:bob", w(vec!["b"])));
        let window = &s.watched_seen[&("bob".to_string(), "mail".to_string(), "list_inbox".to_string())];
        assert_eq!(window.len(), 2, "replay kept {window:?}");
        assert_eq!(window[0], std::collections::HashSet::from(["a".to_string(), "b".to_string()]));
        assert_eq!(window[1], std::collections::HashSet::from(["b".to_string()]), "the overlapping answer unioned");
    }

    /// Mixed history across the upgrade (#423): era-1 deltas union,
    /// then an overlapping era-2 whole pushes.
    #[test]
    fn mixed_delta_then_whole_history_replays_each_as_itself() {
        let mut s = State::default();
        let w = |ids: Vec<&str>| json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids });
        s.apply(&ev(1, "person:bob", w(vec!["a"])));
        s.apply(&ev(2, "person:bob", w(vec!["b"])));
        s.apply(&ev(3, "person:bob", w(vec!["b", "c"])));
        let window = &s.watched_seen[&("bob".to_string(), "mail".to_string(), "list_inbox".to_string())];
        assert_eq!(window.len(), 2, "replay kept {window:?}");
        assert_eq!(window[0], std::collections::HashSet::from(["a".to_string(), "b".to_string()]));
        assert_eq!(window[1], std::collections::HashSet::from(["b".to_string(), "c".to_string()]), "the whole answer unioned onto history");
    }

    /// The accepted edge (#423): a total-churn era-2 pair is
    /// indistinguishable from seed plus delta, so it unions. Two
    /// tagged reads heal the window; this pins the transient shape.
    #[test]
    fn disjoint_untagged_era_two_pair_unions_transiently() {
        let mut s = State::default();
        let w = |ids: Vec<&str>| json!({ "kind": "watched_seen", "person": "bob", "app": "mail", "tool": "list_inbox", "ids": ids });
        s.apply(&ev(1, "person:bob", w(vec!["a"])));
        s.apply(&ev(2, "person:bob", w(vec!["b"])));
        let window = &s.watched_seen[&("bob".to_string(), "mail".to_string(), "list_inbox".to_string())];
        assert_eq!(window.len(), 2, "replay kept {window:?}");
        assert_eq!(window[0], std::collections::HashSet::from(["a".to_string()]));
        assert_eq!(window[1], std::collections::HashSet::from(["a".to_string(), "b".to_string()]), "the disjoint pair did not union");
    }
}
