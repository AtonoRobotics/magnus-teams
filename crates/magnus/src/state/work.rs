//! The work: tasks and their runs, the runs a person stopped, drafts'
//! sources and tasks' attachments, and the conversations Magnus opened.
//!
//! Relocated, not encapsulated: the fields stay on the state, read in
//! place across the kernel (decisions/2026-11-14-state-folds-per-domain.md).
//! The folds here write only what they are handed; `State::apply` passes
//! in what another domain decides (a task's starting taint) and keeps the
//! cross-domain order (a schedule's count, an inbox item's run, a file
//! delivered into the library).

use super::{LibFile, Schedule, State};
use crate::events::{Ev, RunConfig};
use crate::log::Event;
use crate::tools::Label;
use chrono::{DateTime, Utc};
use magnus_adapter_proto::Block;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

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
    /// Someone else made it for this person (a teammate's mission lane, the
    /// team agent's delegation): its words are theirs, never what this
    /// person asked or the addresses this person named.
    pub by_other: bool,
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

/// What a scoped drafting step wrote from: the threads (none for a new
/// draft) and their participants' addresses (#178).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftSources {
    pub person: String,
    pub threads: Vec<String>,
    pub participants: Vec<String>,
}

/// A run's own record of what happened in it: every kind that names a run
/// and writes nothing else. A run's end stands once: the first of
/// Completed, Failed and Cancelled, so a cancel that loses the race to the
/// run's own end changes nothing, nor does the run's end after a cancel.
pub(super) fn fold_run(runs: &mut HashMap<String, Run>, e: &Event) {
    let Some(r) = e.ev.run().and_then(|id| runs.get_mut(id)) else { return };
    match &e.ev {
        Ev::ContextReset { from, to, note, .. } => {
            r.cuts.push(Cut { from: *from, to: *to, note: note.clone() });
            r.lost_since = *to;
        }
        Ev::Escalated { adapter, model, .. } => {
            r.escalated = Some((adapter.clone(), model.clone()));
            r.lost_since = r.replies.len() as u32;
        }
        Ev::ModelCall { context, .. } => {
            r.model_calls += 1;
            r.personal |= context.carried_person();
        }
        Ev::ModelReply { step, blocks, state, usage, .. } => {
            r.usd += usage.cost_usd;
            r.tokens.add(usage);
            r.replies.push(Reply { step: *step, seq: e.seq, blocks: blocks.clone(), state: state.clone() });
        }
        Ev::VerifierReply { usage, .. }
        | Ev::ReviewerReply { usage, .. }
        | Ev::MemoryPass { usage, .. }
        | Ev::SpokenWritten { usage, .. }
        | Ev::KnowledgeExtraction { usage, .. }
        | Ev::DraftScoped { usage, .. } => {
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
            r.tokens.add(usage);
        }
        Ev::ModelError { usage, .. } => {
            r.model_errors += 1;
            if let Some(usage) = usage {
                r.usd += usage.cost_usd;
                r.tokens.add(usage);
            }
        }
        Ev::Notice { step, text, .. } => r.notices.push((*step, e.seq, text.clone())),
        Ev::Intent { call_id, key, tool, input, issuer, .. } => {
            r.actions += 1;
            r.intents.insert(key.clone(), Intent { call_id: call_id.clone(), tool: tool.clone(), at: e.at, input: input.clone(), issuer: issuer.clone(), result: None });
        }
        Ev::Decision { call_id, tool, outcome, stage, rule, .. } => {
            r.decisions.push(DecisionRec { seq: e.seq, call_id: call_id.clone(), tool: tool.clone(), stage: stage.clone(), outcome: outcome.clone(), rule: rule.clone() });
        }
        Ev::ToolResult { call_id, key, status, text, is_error, label, data, sources, check, .. } => {
            let rec = ResultRec { seq: e.seq, text: text.clone(), is_error: *is_error, status: status.clone(), data: data.clone() };
            if let Some(i) = key.as_ref().and_then(|k| r.intents.get_mut(k)) {
                i.result = Some(rec.clone());
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
        Ev::Contract { version, contract, approved_by, .. } => {
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
        // Retired with contracts (spec rev 147): read in old logs only.
        Ev::WorkStarted { .. } => r.work_started = true,
        Ev::ContractFrozen { version, .. } => r.contract_frozen = Some(*version),
        Ev::Plan { version, plan, .. } => {
            r.plan = Some(plan.clone());
            r.plan_version = *version;
        }
        Ev::Graded { grader, passed, score, details, .. } => r.grades.push((grader.clone(), *passed, *score, details.clone())),
        Ev::Verification { criterion, passed, evidence, by, .. } => {
            if !passed {
                r.checks_failed += 1;
            }
            r.verified.insert(criterion.clone(), (*passed, evidence.clone(), by.clone()));
        }
        Ev::Suspended { reason, item, .. } => {
            r.suspensions.push(reason.clone());
            r.first_stop.get_or_insert(e.at);
            if !r.status.terminal() {
                r.status = Status::Suspended;
                r.reason = Some(reason.clone());
                r.waiting_item = item.clone();
            }
        }
        Ev::Resumed { .. } => {
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
        Ev::Wait { until, .. } => r.wait_until = Some(*until),
        Ev::BudgetExtended { budget, .. } => r.config.budget = budget.clone(),
        Ev::Completed { summary, view, spoken, .. } if !r.status.terminal() => {
            r.status = Status::Completed;
            r.summary = Some(summary.clone());
            r.outcome_view = view.clone();
            r.spoken = spoken.clone();
            r.end(e.at);
        }
        Ev::Failed { reason, .. } if !r.status.terminal() => {
            r.status = Status::Failed;
            r.summary = Some(reason.clone());
            r.end(e.at);
        }
        Ev::Cancelled { .. } if !r.status.terminal() => {
            r.status = Status::Cancelled;
            r.end(e.at);
        }
        _ => {}
    }
}

/// A task and its first run, created together. `start` is the taint the
/// run starts with, from the playbook and commitments it carries.
pub(super) fn created(tasks: &mut BTreeMap<String, Task>, runs: &mut HashMap<String, Run>, e: &Event, start: Label) {
    let Ev::TaskCreated { task, run, person, config, attachments, experiment, chat, files, project, grant, .. } = &e.ev else { return };
    let uploads = !attachments.is_empty() || !files.is_empty();
    tasks.insert(
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
            by_other: match e.actor.split_once(':') {
                Some(("person", by)) => by != person,
                Some(("run", by)) => runs.get(by).is_some_and(|r| r.person != *person),
                _ => false,
            },
        },
    );
    runs.insert(
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
            untrusted: uploads || start.untrusted,
            private: uploads || start.private,
            outside: false,
            // Privacy carried in from where the run started (a
            // thread, a schedule's source) has no app to name:
            // the unnamed source, as for a result without one,
            // so readers and the audience rule still see it.
            private_from: if uploads {
                ["library".to_string()].into()
            } else if start.private {
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

/// A person's verdict on their task; anyone's on a trial's.
pub(super) fn verdict(tasks: &mut BTreeMap<String, Task>, runs: &HashMap<String, Run>, e: &Event) {
    let Ev::Verdict { task, person, verdict, note } = &e.ev else { return };
    let trial = tasks.get(task).and_then(|t| runs.get(&t.run_id)).is_some_and(|r| r.experiment.is_some());
    if let Some(t) = tasks.get_mut(task).filter(|t| &t.person == person || trial) {
        t.verdict = Some(Verdict { verdict: verdict.clone(), note: note.clone(), at: e.at });
    }
}

/// A file a run delivered: on the run, and the library's record of it,
/// with the run's taint, for the router to put in the person's library.
pub(super) fn output(runs: &mut HashMap<String, Run>, e: &Event) -> Option<LibFile> {
    let Ev::Output { id, run, name, media_type, data_b64 } = &e.ev else { return None };
    let r = runs.get_mut(run)?;
    r.outputs.push((id.clone(), name.clone(), media_type.clone(), data_b64.len() * 3 / 4));
    Some(LibFile {
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
    })
}

impl Run {
    /// The run ended now; its first stop is now if it never stopped before.
    fn end(&mut self, at: DateTime<Utc>) {
        self.reason = None;
        self.ended_at = Some(at);
        self.first_stop.get_or_insert(at);
    }
}

impl Tokens {
    fn add(&mut self, u: &magnus_adapter_proto::Usage) {
        self.input += u.input_tokens;
        self.output += u.output_tokens;
        self.cache_read += u.cache_read_tokens;
        self.cache_write += u.cache_write_tokens;
    }
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
            Ev::ItemResolved { id, .. } | Ev::ItemOpened { id } => return self.inbox.items().get(id).map(|i| i.person.clone()),
            Ev::Team(c) => return self.team.content_owner(c),
            Ev::TeamMission(m) => return self.missions.content_owner(m),
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
    pub(super) fn charge_grant(&mut self, run: &str, at: DateTime<Utc>, usd: f64) {
        self.charge(run, at, usd);
    }
}
