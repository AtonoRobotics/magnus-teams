//! Magnus asking an outside agent. Sending a task is a boundary action
//! (`outside_agent_send`): the gate judges it like any send, its
//! destination is the endpoint declared at connect, and private data sent
//! after untrusted input asks unless a rule covers it (invariant 4). What
//! comes back, replies, artifacts and status, is the outside agent's words,
//! recorded under its untrusted label, so it taints the run and reaches the
//! model as quoted data. The task is recorded before the send, and a run a
//! crash left mid-send resumes by reading the task's state (GetTask), never
//! by sending it again (invariant 3). Its report that it finished is not
//! evidence: what it changed that Magnus can read is read back (invariant
//! 7). A run that stops cancels its open tasks (CancelTask).

use super::client::RpcError;
use super::ledger::{Change, OutsideAgent};
use super::{CANCELED, CHECK_TOOL, COMPLETED, FAILED};
use crate::boundary::{Action, CallFailure, Issuer, Verdict};
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Status;
use crate::tools::Risk;
use serde_json::{json, Value};
use std::sync::Arc;

/// What a crashed send's task turned out to be, read at resume.
pub(crate) enum Resumed {
    /// It was never recorded as sent: sending it is its first try.
    NotSent,
    /// The outside agent has it: its state, as the send's answer.
    Answer { text: String, data: Option<Value> },
    /// It may have reached the outside agent, which gave no task id: unknown.
    Unknown,
}

/// A task's id, state and what the outside agent said with it, from a
/// SendMessage or GetTask result: a task, or a message standing for a
/// finished one.
fn read_task(result: &Value, fallback_id: &str) -> (String, String, String) {
    let t = result.get("task").unwrap_or(result);
    if let Some(m) = result.get("message") {
        let id = m["messageId"].as_str().unwrap_or(fallback_id).to_string();
        return (id, COMPLETED.into(), super::bounded(&super::parts_text(&m["parts"])));
    }
    let id = t["id"].as_str().unwrap_or(fallback_id).to_string();
    let state = t["status"]["state"].as_str().unwrap_or(super::SUBMITTED).to_string();
    let mut said = vec![super::parts_text(&t["status"]["message"]["parts"])];
    for a in t["artifacts"].as_array().into_iter().flatten() {
        said.push(super::parts_text(&a["parts"]));
    }
    (id, state, super::bounded(&said.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n")))
}

/// The answer the model reads: the task, its state and the agent's words.
fn answer(agent: &OutsideAgent, task: &str, state: &str, said: &str) -> String {
    let words = if said.is_empty() { "It said nothing yet.".to_string() } else { format!("It said:\n{said}") };
    format!("{} has task {task}; its state is {state}. {words}", agent.name)
}

fn stopped(m: impl Into<String>) -> CallFailure {
    CallFailure::NotDone(m.into())
}

impl Kernel {
    /// The agent a run's person connected under `name`.
    fn run_agent(&self, run: &str, name: &str) -> Option<OutsideAgent> {
        self.read(|s| {
            let person = s.runs.get(run)?.person.clone();
            s.outside_agents.named(&person, name).cloned()
        })
    }

    /// Where an outside agent tool's action goes: the endpoint its agent's
    /// card declared at connect, by its host, as a rule names a destination.
    pub(crate) fn outside_destinations(&self, a: &Action) -> Vec<String> {
        let agent = self.run_agent(&a.run, a.input["agent"].as_str().unwrap_or_default());
        agent.and_then(|g| url::Url::parse(&g.endpoint).ok()?.host_str().map(crate::rules::normalized)).into_iter().collect()
    }

    /// Carry out an outside agent tool, past the gate.
    pub(crate) async fn outside_agent_call(self: &Arc<Self>, a: &Action, key: &str) -> Result<(String, bool, Option<Value>), CallFailure> {
        let name = a.input["agent"].as_str().unwrap_or_default();
        let agent = self.run_agent(&a.run, name).ok_or_else(|| stopped(format!("no outside agent called {name} is connected for the person this task is for")))?;
        if a.tool == CHECK_TOOL {
            return self.check_task(a, &agent).await;
        }
        let text = a.input["message"].as_str().map(str::trim).unwrap_or_default();
        if text.is_empty() || text.chars().count() > super::MAX_TEXT_CHARS {
            return Err(stopped(format!("the message must be 1 to {} characters", super::MAX_TEXT_CHARS)));
        }
        // Recorded before the send, with the message id the send carries.
        let message_id = format!("magnus-{key}");
        if self.read(|s| s.outside_agents.outbound(key).is_none()) {
            let opened = Change::TaskOpened { agent: agent.id.clone(), run: a.run.clone(), call_id: a.call_id.clone(), key: key.into(), message_id: message_id.clone() };
            self.emit("kernel", Ev::OutsideAgent(opened)).await.map_err(|e| stopped(format!("{e:#}")))?;
        }
        let params = json!({
            "message": { "messageId": message_id, "role": "ROLE_USER", "parts": [{ "text": text }] },
            "configuration": { "acceptedOutputModes": ["text/plain", "application/json"] }
        });
        match self.outside_rpc(&agent, "SendMessage", params, super::CALL_WAIT).await {
            Ok(result) => {
                let (task, state, said) = read_task(&result, &message_id);
                let started = Change::TaskStarted { key: key.into(), task: task.clone(), state: state.clone() };
                self.emit("kernel", Ev::OutsideAgent(started)).await.map_err(|e| CallFailure::Unknown(format!("{e:#}")))?;
                let mut text = answer(&agent, &task, &state, &said);
                if state == COMPLETED {
                    text = format!("{text}\n\n{}", self.outside_read_back(a, &a.input["read_back"]).await);
                }
                Ok((text, false, Some(json!({ "agent": agent.id, "task": task, "state": state }))))
            }
            Err(RpcError::Unknown(m)) => Err(CallFailure::Unknown(m)),
            Err(e) => {
                let _ = self.emit("kernel", Ev::OutsideAgent(Change::TaskMoved { key: key.into(), state: FAILED.into() })).await;
                Err(stopped(format!("{} {}", agent.name, e.said())))
            }
        }
    }

    /// GetTask on a task this person's runs sent `agent`.
    async fn check_task(self: &Arc<Self>, a: &Action, agent: &OutsideAgent) -> Result<(String, bool, Option<Value>), CallFailure> {
        let task = a.input["task"].as_str().unwrap_or_default();
        let found = self.read(|s| s.outside_agents.sent_as(&agent.id, task));
        let Some((key, before)) = found else { return Err(stopped(format!("{} has no task {task} from this person's work", agent.name))) };
        let result = self.outside_rpc(agent, "GetTask", json!({ "id": task }), super::CALL_WAIT).await.map_err(|e| stopped(format!("{} {}", agent.name, e.said())))?;
        let (_, state, said) = read_task(&result, task);
        let _ = self.emit("kernel", Ev::OutsideAgent(Change::TaskMoved { key: key.clone(), state: state.clone() })).await;
        let mut text = answer(agent, task, &state, &said);
        if state == COMPLETED && !super::terminal(&before.state) {
            let spec = self.read(|s| s.runs.get(&before.run).and_then(|r| r.intents.get(&key)).map(|i| i.input["read_back"].clone())).unwrap_or(Value::Null);
            text = format!("{text}\n\n{}", self.outside_read_back(a, &spec).await);
        }
        Ok((text, false, Some(json!({ "agent": agent.id, "task": task, "state": state }))))
    }

    /// Read back what the outside agent's finished work changed, with the
    /// person's own read tool the send named: the server's read, which may
    /// only read, recorded as the action's read-back. Its report alone is
    /// never evidence. The line for the result says how it went.
    async fn outside_read_back(self: &Arc<Self>, a: &Action, spec: &Value) -> String {
        let record = |read: Option<String>, found: Value, matches: Option<bool>, note: String| {
            Ev::Evidence(crate::evidence::Change::ReadBack { run: a.run.clone(), call_id: a.call_id.clone(), tool: a.tool.clone(), read, found, matches, note })
        };
        let tool = spec["tool"].as_str().unwrap_or_default().to_string();
        let readable = !tool.is_empty() && crate::boundary::facts(self, &tool).is_some_and(|f| f.risk == Risk::Read && f.app != super::APP);
        let (read, found, matches, note) = if !readable {
            let note = if tool.is_empty() {
                "Not read back: the outside agent says it finished, and its report is not evidence; nothing it changed was named to read back.".to_string()
            } else {
                format!("Not read back: {tool} is not a read tool of the person's that can check it, and the outside agent's report is not evidence.")
            };
            (None, Value::Null, None, note)
        } else {
            let check = Action {
                run: a.run.clone(),
                call_id: format!("readback:{}", a.call_id),
                step_id: format!("{}.readback", a.step_id),
                tool: tool.clone(),
                input: spec.get("input").cloned().unwrap_or_else(|| json!({})),
                issuer: Issuer::ReadBack,
            };
            match Box::pin(self.execute(check)).await {
                Ok(Verdict::Done { is_error: false, text, data }) => {
                    let found = data.unwrap_or_else(|| serde_json::from_str(&text).unwrap_or(Value::String(text)));
                    let expect = spec["expect"].as_object().cloned().unwrap_or_default();
                    let off: Vec<String> =
                        expect.iter().filter(|(ptr, want)| !found.pointer(ptr).is_some_and(|got| crate::read_back::holds(want, got))).map(|(p, _)| p.clone()).collect();
                    if expect.is_empty() {
                        (Some(tool.clone()), found, None, format!("Read back with {tool}, with nothing named to compare: the outside agent's report is not evidence."))
                    } else if off.is_empty() {
                        (Some(tool.clone()), found, Some(true), format!("Read back with {tool}: it matches what the outside agent reported."))
                    } else {
                        (Some(tool.clone()), found, Some(false), format!("Read back with {tool}: it does not match ({} differs). Do not report this as done.", off.join(", ")))
                    }
                }
                Ok(Verdict::Done { .. }) | Err(_) => (Some(tool.clone()), Value::Null, None, format!("Not read back: {tool} could not be read.")),
                Ok(Verdict::Suspended) => (Some(tool.clone()), Value::Null, None, format!("Not read back: {tool} was held.")),
            }
        };
        if let Err(e) = self.emit("kernel", record(read, found, matches, note.clone())).await {
            tracing::warn!("an outside agent's read-back: {e:#}");
        }
        note
    }

    /// A send a crash left without its result: read the task's state, by
    /// the id the outside agent gave it, instead of sending it again.
    pub(crate) async fn resume_outside_task(&self, a: &Action, key: &str) -> Resumed {
        let Some(o) = self.read(|s| s.outside_agents.outbound(key).cloned()) else { return Resumed::NotSent };
        let (Some(task), Some(agent)) = (o.task.clone(), self.read(|s| s.outside_agents.agent(&o.agent).cloned())) else { return Resumed::Unknown };
        match self.outside_rpc(&agent, "GetTask", json!({ "id": task }), super::CALL_WAIT).await {
            Ok(result) => {
                let (_, state, said) = read_task(&result, &task);
                let _ = self.emit("kernel", Ev::OutsideAgent(Change::TaskMoved { key: key.into(), state: state.clone() })).await;
                let text = format!(
                    "{} (read after a restart; the task was not sent again.) Its report is not evidence: check what it changed before you report it done.",
                    answer(&agent, &task, &state, &said)
                );
                tracing::info!("{}: resumed outside task {task} with GetTask", a.tool);
                Resumed::Answer { text, data: Some(json!({ "agent": agent.id, "task": task, "state": state })) }
            }
            Err(e) => {
                tracing::warn!("resuming outside task {task}: {}", e.said());
                Resumed::Unknown
            }
        }
    }

    /// CancelTask for each open task sent `agent` that `which` picks; the
    /// number cancelled.
    pub(crate) async fn cancel_sent_tasks(&self, agent: &OutsideAgent, which: impl Fn(&super::Outbound) -> bool) -> usize {
        let open = self.read(|s| s.outside_agents.open_outbound(|o| o.agent == agent.id && which(o)));
        let mut n = 0;
        for (key, o) in open {
            let Some(task) = o.task else { continue };
            match self.outside_rpc(agent, "CancelTask", json!({ "id": task }), super::CANCEL_WAIT).await {
                Ok(result) => {
                    let state = result["status"]["state"].as_str().unwrap_or(CANCELED).to_string();
                    let _ = self.emit("kernel", Ev::OutsideAgent(Change::TaskMoved { key, state })).await;
                    n += 1;
                }
                Err(e) => tracing::warn!("cancelling outside task {task}: {}", e.said()),
            }
        }
        n
    }

    /// A run ended. Stopped or failed, its open outside tasks are cancelled;
    /// a request it was taken up for ends as it did. A finished one's reply
    /// goes through the gate (`reply`).
    pub(crate) async fn outside_run_ended(&self, run: &str) {
        let Some(status) = self.read(|s| s.runs.get(run).map(|r| r.status)) else { return };
        if !matches!(status, Status::Cancelled | Status::Failed) {
            return;
        }
        let agents: Vec<OutsideAgent> =
            self.read(|s| s.outside_agents.open_outbound(|o| o.run == run).into_iter().filter_map(|(_, o)| s.outside_agents.agent(&o.agent).cloned()).collect());
        let mut done = std::collections::BTreeSet::new();
        for agent in agents {
            if done.insert(agent.id.clone()) {
                self.cancel_sent_tasks(&agent, |o| o.run == run).await;
            }
        }
        if let Some(i) = self.read(|s| s.outside_agents.served_by(run).filter(|i| !super::terminal(&i.state)).cloned()) {
            let state = if status == Status::Cancelled { CANCELED } else { FAILED };
            let ended = Change::RequestEnded { task: i.task, state: state.into(), reason: "the work on it stopped".into() };
            if let Err(e) = self.emit("kernel", Ev::OutsideAgent(ended)).await {
                tracing::warn!("ending an outside request: {e:#}");
            }
        }
    }
}
