//! Tasks and schedules under control (behavior spec, Tasks and schedules in
//! conversation): list and get tasks and schedules, cancel a task, and
//! pause, resume, edit and cancel a schedule. The same operations serve the
//! app's buttons and Magnus in a conversation, so the two cannot disagree.
//! The layered policy decides who controls what (#266): by default a
//! person controls their own, and the owner and admins control anyone's.

use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{Run, Schedule, State as Projection, Task};
use crate::tools::Label;
use serde_json::{json, Value};

/// Why a control was not done: the person sees "no such" for what is not
/// theirs, so its existence is not told; a policy denial of what they can
/// see names the policy, so a narrowing is answerable.
#[derive(Debug, PartialEq)]
pub enum Refused {
    NotFound(&'static str),
    Denied(String),
    Conflict(String),
    Invalid(String),
    Failed(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotFound(what) => write!(f, "no such {what}"),
            Refused::Denied(m) | Refused::Conflict(m) | Refused::Invalid(m) | Refused::Failed(m) => f.write_str(m),
        }
    }
}

fn may_control(who: &Person, owner: &str) -> bool {
    who.id == owner || who.is_owner() || who.has_role("admin")
}

/// What a control's event records about how it was done (#267): the chat
/// run did it, or the app's button did.
fn sources(turn: Option<&str>) -> (Option<String>, Option<String>) {
    match turn {
        Some(turn) => (Some("conversation".into()), Some(turn.into())),
        None => (Some("button".into()), None),
    }
}

/// Whether `who` may `op` the `kind` (`"Task"`, `"Schedule"`) `id` owned
/// by `owner`: the layered policy decides (#266). A denial of a row they
/// can see is honest; of one they cannot, it reads as not found, as
/// today. A policy that fails to evaluate falls back to today's fixed
/// rule, so one broken layer cannot stop every control in the place.
fn control_check(client: &Kernel, who: &Person, op: &str, kind: &str, id: &str, owner: &str, what: &'static str) -> Result<(), Refused> {
    let policies = client.read(|s| s.policies.clone());
    let t = crate::policy::ControlFacts { op, kind, id, owner };
    match crate::policy::control_allowed(&policies, who, &t) {
        Ok(_) => Ok(()),
        Err(crate::policy::ControlDeny::Forbidden(m)) if may_control(who, owner) => Err(Refused::Denied(m)),
        Err(crate::policy::ControlDeny::Broken(_)) if may_control(who, owner) => Ok(()),
        Err(_) => Err(Refused::NotFound(what)),
    }
}

/// One task's row for lists and get: status, result, what it did.
fn task_row(t: &Task, r: &Run) -> Value {
    json!({ "id": t.task_id, "person": t.person, "status": crate::api::status_str(r.status), "created_at": t.created_at, "schedule": t.schedule, "summary": r.summary, "review": t.review })
}

/// One schedule's row for lists and get, with its latest run.
fn schedule_row(s: &Projection, x: &Schedule) -> Value {
    let latest = s
        .tasks
        .values()
        .filter(|t| t.schedule.as_deref() == Some(&x.id))
        .max_by_key(|t| t.created_at)
        .and_then(|t| s.runs.get(&t.run_id).map(|r| json!({ "task": t.task_id, "status": crate::api::status_str(r.status), "at": t.created_at, "summary": r.summary })));
    json!({
        "id": x.id, "person": x.person, "every_minutes": x.every_minutes, "daily_at": x.daily_at, "until": x.until, "max_runs": x.max_runs,
        "runs": x.started, "paused": x.paused, "next": x.next_due(), "latest": latest,
    })
}

/// One task's row by id, however old, for `who` to control. None when
/// there is no such task or it is not theirs (#265).
fn controlled_task(s: &Projection, who: &Person, id: &str) -> Option<Value> {
    let t = s.tasks.get(id).filter(|t| may_control(who, &t.person))?;
    s.runs.get(&t.run_id).map(|r| task_row(t, r))
}

/// One schedule's row by id for `who` to control (#265).
fn controlled_schedule(s: &Projection, who: &Person, id: &str) -> Option<Value> {
    s.schedules.get(id).filter(|x| may_control(who, &x.person)).map(|x| schedule_row(s, x))
}

impl Kernel {
    /// The tasks `who` controls (their own; everyone's for the owner and
    /// admins), newest first.
    pub fn tasks_for(&self, who: &Person, limit: usize) -> Vec<Value> {
        let mut v: Vec<Value> = self.read(|s| {
            s.tasks
                .values()
                .filter(|t| may_control(who, &t.person) && t.chat.as_ref().is_none_or(|c| !c.turn) && !t.background())
                .filter_map(|t| s.runs.get(&t.run_id).map(|r| task_row(t, r)))
                .collect()
        });
        v.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
        v.truncate(limit);
        v
    }

    /// The schedules `who` controls.
    pub fn schedules_for(&self, who: &Person) -> Vec<Value> {
        self.read(|s| s.schedules.values().filter(|x| may_control(who, &x.person)).map(|x| schedule_row(s, x)).collect())
    }

    /// One task by id, however old: the list's row, and what its run did
    /// so far, split the way a cancel report splits it; under the same
    /// who-may-control rule, no such task when it is not theirs (#340).
    pub fn get_task(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let mut row = self.read(|s| controlled_task(s, who, id)).ok_or(Refused::NotFound("task"))?;
        // The row above passed the control rule; its run is fetched as is.
        let run = self.read(|s| s.tasks.get(id).and_then(|t| s.runs.get(&t.run_id).cloned()));
        let Some(run) = run else { return Err(Refused::NotFound("task")) };
        let (cannot_be_undone, already_done, possibly_done) = self.stopped_run_report(&run);
        if let Some(o) = row.as_object_mut() {
            o.insert("cannot_be_undone".into(), json!(cannot_be_undone));
            o.insert("already_done".into(), json!(already_done));
            o.insert("possibly_done".into(), json!(possibly_done));
        }
        Ok(row)
    }

    /// One schedule by id under the same rule (#265).
    pub fn get_schedule(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        self.read(|s| controlled_schedule(s, who, id)).ok_or(Refused::NotFound("schedule"))
    }

    /// Cancel a task: its run stops, the cancellation is in the log, and
    /// the answer says what it had already done, the changes that cannot be
    /// undone first.
    pub async fn cancel_task(&self, who: &Person, id: &str, turn: Option<&str>) -> Result<Value, Refused> {
        let found = self.read(|s| s.tasks.get(id).and_then(|t| s.runs.get(&t.run_id).map(|r| (t.person.clone(), r.clone()))));
        let Some((owner, run)) = found else { return Err(Refused::NotFound("task")) };
        control_check(self, who, "cancel_task", "Task", id, &owner, "task")?;
        if run.status.terminal() {
            return Err(Refused::Conflict(format!("the task has already ended ({})", crate::api::status_str(run.status))));
        }
        let (source, turn_ev) = sources(turn);
        self.emit(&format!("person:{}", who.id), Ev::Cancelled { run: run.id.clone(), source, turn: turn_ev }).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        // It may have ended on its own as the cancel was recorded: then the
        // cancel changed nothing, and the person is told how it ended.
        let now = self.read(|s| s.runs.get(&run.id).map(|r| r.status));
        if let Some(st) = now.filter(|st| *st != crate::state::Status::Cancelled) {
            return Err(Refused::Conflict(format!("the task has already ended ({})", crate::api::status_str(st))));
        }
        self.on_stop(&run.id, true).await;
        // Reported from the run as it is now: results recorded while it
        // stopped read done, not possibly done.
        let now = self.read(|s| s.runs.get(&run.id).cloned()).unwrap_or(run);
        let (cannot_be_undone, already_done, possibly_done) = self.stopped_run_report(&now);
        Ok(json!({
            "cancelled": id,
            "cannot_be_undone": cannot_be_undone,
            "already_done": already_done,
            "possibly_done": possibly_done,
        }))
    }

    /// What a stopped run had done, for cancel reports: confirmed done
    /// split by reversibility, plus what may have happened — still in
    /// flight or outcome unknown — irreversible first.
    fn stopped_run_report(&self, run: &crate::state::Run) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut done_irr = vec![];
        let mut done_rev = vec![];
        let mut maybe_irr = vec![];
        let mut maybe_rev = vec![];
        for i in run.intents.values() {
            let Some(f) = crate::boundary::facts(self, &i.tool).filter(|f| f.risk.changes_state()) else { continue };
            let name = i.tool.replace("__", ": ").replace('_', " ");
            let irr = matches!(f.risk, crate::tools::Risk::Irreversible | crate::tools::Risk::Financial);
            match report_class(i.result.as_ref()) {
                ReportClass::Done if irr => done_irr.push(name),
                ReportClass::Done => done_rev.push(name),
                ReportClass::Possibly if irr => maybe_irr.push(name),
                ReportClass::Possibly => maybe_rev.push(name),
                ReportClass::Skip => {}
            }
        }
        let mut possibly_done = maybe_irr;
        possibly_done.extend(maybe_rev);
        (done_irr, done_rev, possibly_done)
    }

    fn schedule_owned(&self, who: &Person, id: &str, op: &str) -> Result<crate::state::Schedule, Refused> {
        let sc = self.read(|s| s.schedules.get(id).cloned()).ok_or(Refused::NotFound("schedule"))?;
        control_check(self, who, op, "Schedule", id, &sc.person, "schedule")?;
        Ok(sc)
    }

    /// Pause a schedule: no more runs start until it is resumed; a run
    /// already going goes on. Pausing one held on its waiting run keeps
    /// the person's pause: the hold's end does not resume it. An admin's
    /// suspension is not paused over.
    pub async fn pause_schedule(&self, who: &Person, id: &str, turn: Option<&str>) -> Result<(), Refused> {
        let sc = self.schedule_owned(who, id, "pause_schedule")?;
        if sc.ended.is_some() {
            return Err(Refused::Conflict("the schedule has ended".into()));
        }
        if sc.paused_by_admin {
            return Err(Refused::Conflict("it is suspended by an admin: only an admin lifts it".into()));
        }
        if sc.paused.is_some() && sc.paused_on.is_none() {
            return Err(Refused::Conflict("the schedule is already paused".into()));
        }
        let reason = if who.id == sc.person { "Paused by you".to_string() } else { format!("Paused by {}", who.name) };
        let (source, turn_ev) = sources(turn);
        self.emit(&format!("person:{}", who.id), Ev::SchedulePaused { id: id.into(), reason, task: None, admin: false, source, turn: turn_ev })
            .await
            .map_err(|e| Refused::Failed(format!("{e:#}")))?;
        Ok(())
    }

    /// Resume a schedule its person paused. An admin's suspension is
    /// lifted only by an admin (or the owner), here or in the console.
    pub async fn resume_schedule(&self, who: &Person, id: &str, turn: Option<&str>) -> Result<(), Refused> {
        let sc = self.schedule_owned(who, id, "resume_schedule")?;
        if sc.ended.is_some() {
            return Err(Refused::Conflict("the schedule has ended".into()));
        }
        if sc.paused.is_none() {
            return Err(Refused::Conflict("the schedule is not paused".into()));
        }
        if sc.paused_by_admin && !(who.is_owner() || who.has_role("admin")) {
            return Err(Refused::Conflict("it is suspended by an admin: only an admin lifts it".into()));
        }
        if sc.paused_on.is_some() {
            return Err(Refused::Conflict("its run is waiting on you: answer it, or resume or cancel that task, and the schedule goes on".into()));
        }
        let (source, turn_ev) = sources(turn);
        self.emit(&format!("person:{}", who.id), Ev::ScheduleResumed { id: id.into(), source, turn: turn_ev }).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        Ok(())
    }

    /// Cancel a schedule: no more runs, and a run of it still going stops.
    /// An ended schedule is only deleted: it starts no more runs anyway,
    /// and its last run, which can still be going, goes on (#452).
    pub async fn cancel_schedule(&self, who: &Person, id: &str, turn: Option<&str>) -> Result<Value, Refused> {
        let sc = self.schedule_owned(who, id, "cancel_schedule")?;
        let going: Vec<(String, String)> = if sc.ended.is_some() {
            vec![]
        } else {
            self.read(|s| {
                s.tasks
                    .values()
                    .filter(|t| t.schedule.as_deref() == Some(id))
                    .filter_map(|t| s.runs.get(&t.run_id))
                    .filter(|r| !r.status.terminal())
                    .map(|r| (r.task.clone(), r.id.clone()))
                    .collect()
            })
        };
        let (source, turn_ev) = sources(turn);
        self.emit(&format!("person:{}", who.id), Ev::ScheduleRemoved { id: id.into(), source, turn: turn_ev }).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        let mut runs = vec![];
        let mut stopped = 0;
        for (task, run) in &going {
            let (source, turn_ev) = sources(turn);
            if self.emit(&format!("person:{}", who.id), Ev::Cancelled { run: run.clone(), source, turn: turn_ev }).await.is_ok() {
                self.on_stop(run, true).await;
                stopped += 1;
                if let Some(now) = self.read(|s| s.runs.get(run).cloned()) {
                    let (cannot_be_undone, already_done, possibly_done) = self.stopped_run_report(&now);
                    runs.push(json!({ "task": task, "run": run, "cannot_be_undone": cannot_be_undone, "already_done": already_done, "possibly_done": possibly_done }));
                }
            }
        }
        Ok(json!({ "cancelled": id, "runs_stopped": stopped, "runs": runs }))
    }

    /// Change when a schedule runs and when it ends. What each run does
    /// stays: to change that, cancel it and set up another.
    pub async fn edit_schedule(&self, who: &Person, id: &str, b: &Value, turn: Option<&str>) -> Result<Value, Refused> {
        let sc = self.schedule_owned(who, id, "edit_schedule")?;
        let timing = b.get("every_minutes").is_some_and(|v| !v.is_null()) || b.get("daily_at").is_some_and(|v| !v.is_null());
        let (every_minutes, daily_at) =
            if timing { crate::api::rule_for(b, self.cfg.schedules.min_every_minutes).map_err(Refused::Invalid)? } else { (sc.every_minutes, sc.daily_at.clone()) };
        let (until, max_runs) = crate::api::end_for(b).map_err(Refused::Invalid)?;
        let until = if b.get("until").is_some() { until } else { sc.until };
        let max_runs = if b.get("max_runs").is_some() { max_runs } else { sc.max_runs };
        if max_runs.is_some_and(|m| m <= sc.started) {
            return Err(Refused::Invalid(format!("it has already run {} times", sc.started)));
        }
        let (source, turn_ev) = sources(turn);
        self.emit(&format!("person:{}", who.id), Ev::ScheduleChanged { id: id.into(), every_minutes, daily_at: daily_at.clone(), until, max_runs, source, turn: turn_ev })
            .await
            .map_err(|e| Refused::Failed(format!("{e:#}")))?;
        Ok(json!({ "id": id, "every_minutes": every_minutes, "daily_at": daily_at, "until": until, "max_runs": max_runs }))
    }

    /// A list `who` asked for, with other people's rows in it as the policy
    /// on people's data allows (owner's ruling 14): their words (what each
    /// run said it did) left out while it is private, the rest kept so the
    /// owner and admins still control them; otherwise the read is recorded
    /// and each person told, as a read of their tasks.
    async fn as_policy_allows(&self, who: &Person, mut rows: Vec<Value>) -> Result<Vec<Value>, Refused> {
        let others: std::collections::BTreeSet<String> = rows.iter().filter_map(|r| r["person"].as_str()).filter(|p| *p != who.id).map(String::from).collect();
        if others.is_empty() {
            return Ok(rows);
        }
        if self.read(|s| s.data_private) {
            for r in rows.iter_mut().filter(|r| r["person"] != who.id.as_str()) {
                if let Some(o) = r.as_object_mut() {
                    o.remove("summary");
                }
                if let Some(o) = r["latest"].as_object_mut() {
                    o.remove("summary");
                }
            }
        } else {
            let others: Vec<String> = others.into_iter().collect();
            self.record_data_read(&format!("person:{}", who.id), &who.name, &others, "tasks", None).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        }
        Ok(rows)
    }

    /// What a list or get of tasks or schedules carries (invariant 4): a
    /// summary is the words of the run that wrote it, so the answer is
    /// labelled as each run whose summary it returned, and names the apps
    /// those runs read private data from. A row with no summary says
    /// nothing a run wrote.
    pub fn listed_taint(&self, listed: &Value) -> (Label, Vec<String>) {
        let tasks = listed["tasks"].as_array().into_iter().flatten().chain(std::iter::once(&listed["task"])).filter(|r| r["summary"].is_string()).filter_map(|r| r["id"].as_str());
        let latest = listed["schedules"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| &x["latest"])
            .chain(std::iter::once(&listed["schedule"]["latest"]))
            .filter(|r| r["summary"].is_string())
            .filter_map(|r| r["task"].as_str());
        let ids: Vec<&str> = tasks.chain(latest).collect();
        self.read(|s| {
            let mut label = Label::PRINCIPAL;
            let mut from = std::collections::BTreeSet::new();
            for r in ids.iter().filter_map(|t| s.tasks.get(*t)).filter_map(|t| s.runs.get(&t.run_id)) {
                label = label.join(Label { untrusted: r.untrusted, private: r.private });
                if r.private {
                    from.extend(r.private_from.iter().cloned());
                }
            }
            (label, from.into_iter().collect())
        })
    }

    /// One of the control tools Magnus calls in a conversation, for the
    /// person the conversation is with: what the tool says back, and whether
    /// it is an error. `turn` is the asking chat turn, recorded on the
    /// control's event (#267, #358).
    pub async fn chat_control(&self, person: &str, tool: &str, input: &Value, turn: &str) -> (String, bool, Option<Value>) {
        let Some(who) = self.person(person) else { return ("no such person".into(), true, None) };
        let id = input["id"].as_str().unwrap_or_default();
        let r: Result<Value, Refused> = match tool {
            "list_tasks" => {
                let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(30).clamp(1, 200) as usize;
                self.as_policy_allows(&who, self.tasks_for(&who, limit)).await.map(|v| json!({ "tasks": v }))
            }
            "list_schedules" => self.as_policy_allows(&who, self.schedules_for(&who)).await.map(|v| json!({ "schedules": v })),
            "get_task" => match self.get_task(&who, id) {
                Ok(row) => self.as_policy_allows(&who, vec![row]).await.map(|mut rows| json!({ "task": rows.pop().unwrap_or(Value::Null) })),
                Err(e) => Err(e),
            },
            "get_schedule" => match self.get_schedule(&who, id) {
                Ok(row) => self.as_policy_allows(&who, vec![row]).await.map(|mut rows| json!({ "schedule": rows.pop().unwrap_or(Value::Null) })),
                Err(e) => Err(e),
            },
            "cancel_task" => self.cancel_task(&who, id, Some(turn)).await,
            "pause_schedule" => self.pause_schedule(&who, id, Some(turn)).await.map(|_| json!({ "paused": id })),
            "resume_schedule" => self.resume_schedule(&who, id, Some(turn)).await.map(|_| json!({ "resumed": id })),
            "edit_schedule" => self.edit_schedule(&who, id, input, Some(turn)).await,
            "cancel_schedule" => self.cancel_schedule(&who, id, Some(turn)).await,
            _ => Err(Refused::Invalid(format!("{tool} is not a control"))),
        };
        match r {
            Ok(v) => (v.to_string(), false, Some(v)),
            Err(e) => (e.to_string(), true, None),
        }
    }
}

/// The control tools, as the model calls them.
pub const TOOLS: [&str; 9] = ["list_tasks", "list_schedules", "get_task", "get_schedule", "cancel_task", "pause_schedule", "resume_schedule", "edit_schedule", "cancel_schedule"];

/// How a recorded intent reads in a cancel report.
#[derive(Debug, PartialEq)]
enum ReportClass {
    Done,
    Possibly,
    Skip,
}

/// Done when its result says so; possibly done when it is still in
/// flight or its outcome is unknown, whatever the error flag says;
/// skipped when it failed or was never sent (#263).
fn report_class(result: Option<&crate::state::ResultRec>) -> ReportClass {
    match result {
        None => ReportClass::Possibly,
        Some(r) if r.status == "unknown" => ReportClass::Possibly,
        Some(r) if !r.is_error => ReportClass::Done,
        Some(_) => ReportClass::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::{controlled_schedule, controlled_task, report_class, sources, ReportClass};
    use crate::log::Event;
    use crate::state::ResultRec;
    use crate::state::State as Projection;
    use serde_json::{json, Value};

    fn rec(status: &str, is_error: bool) -> ResultRec {
        ResultRec { seq: 0, text: String::new(), is_error, status: status.into(), data: None }
    }

    /// One task or schedule by id: status, result or latest run, what it
    /// did — the list's row, however old, for its person, the owner and
    /// admins alone (#265).
    #[test]
    /// A control's event names the chat run that did it, or the button.
    #[test]
    fn a_controls_event_names_conversation_or_button() {
        assert_eq!(sources(Some("run_1")), (Some("conversation".into()), Some("run_1".into())));
        assert_eq!(sources(None), (Some("button".into()), None));
    }

    fn get_reaches_any_controlled_task_or_schedule() {
        let mut s = Projection::default();
        let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
        let t0 = chrono::Utc::now();
        let mut seq = 0;
        let mut ev = |at: chrono::DateTime<chrono::Utc>, actor: &str, e: Value| {
            seq += 1;
            s.apply(&Event { seq, at, actor: actor.into(), ev: serde_json::from_value(e).unwrap() });
        };
        for i in 0..31 {
            let at = t0 + chrono::Duration::minutes(i);
            ev(at, "kernel", json!({ "kind": "task_created", "task": format!("t{i:02}"), "run": format!("r{i:02}"), "person": "carol", "text": "Work.", "config": config }));
            ev(at, "kernel", json!({ "kind": "completed", "run": format!("r{i:02}"), "summary": format!("did {i}") }));
        }
        ev(t0, "kernel", json!({ "kind": "task_created", "task": "tb", "run": "rb", "person": "bob", "text": "Work.", "config": config }));
        ev(t0, "kernel", json!({ "kind": "schedule_set", "id": "s1", "person": "carol", "daily_at": "03:00" }));
        ev(t0, "schedule:s1", json!({ "kind": "task_created", "task": "ts", "run": "rs", "person": "carol", "text": "Water.", "config": config }));
        ev(t0, "kernel", json!({ "kind": "completed", "run": "rs", "summary": "watered" }));
        let who = |id: &str, roles: &[&str]| serde_json::from_value::<crate::config::Person>(json!({ "id": id, "roles": roles })).unwrap();
        let (carol, bob, alice) = (who("carol", &[]), who("bob", &[]), who("alice", &["owner"]));

        // The oldest of 31, past the list's 30, with its result.
        let oldest = controlled_task(&s, &carol, "t00").expect("the oldest task");
        assert_eq!((&oldest["status"], &oldest["summary"]), (&json!("completed"), &json!("did 0")), "{oldest}");
        assert!(controlled_task(&s, &carol, "t99").is_none(), "no such task");
        assert!(controlled_task(&s, &carol, "tb").is_none(), "another person's task");
        assert!(controlled_task(&s, &bob, "t00").is_none(), "another person's task");
        assert_eq!(controlled_task(&s, &alice, "t00").as_ref().and_then(|r| r["summary"].as_str()), Some("did 0"), "the owner reads anyone's");

        // The schedule with its latest run.
        let got = controlled_schedule(&s, &carol, "s1").expect("the schedule");
        assert_eq!((&got["daily_at"], &got["latest"]["task"], &got["latest"]["summary"]), (&json!("03:00"), &json!("ts"), &json!("watered")), "{got}");
        assert!(controlled_schedule(&s, &carol, "s9").is_none(), "no such schedule");
        assert!(controlled_schedule(&s, &bob, "s1").is_none(), "another person's schedule");
    }

    /// In flight and outcome-unknown read possibly done; done reads
    /// done; failed reads skipped (#263).
    #[test]
    fn cancel_reports_classify_intents_by_outcome() {
        assert_eq!(report_class(None), ReportClass::Possibly);
        assert_eq!(report_class(Some(&rec("unknown", true))), ReportClass::Possibly);
        assert_eq!(report_class(Some(&rec("unknown", false))), ReportClass::Possibly);
        assert_eq!(report_class(Some(&rec("done", false))), ReportClass::Done);
        assert_eq!(report_class(Some(&rec("failed", true))), ReportClass::Skip);
    }
}
