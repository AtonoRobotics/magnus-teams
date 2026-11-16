//! Run records (contract, plan, completion, failure, questions, proposals,
//! waits, observations and interrupts) that the loop writes to the log
//! itself (K21). Run records grant nothing.

use super::{admission, kind, Flow, CONTRACT_RETIRED};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::Run;
use crate::tools::Label;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

impl Kernel {
    pub(crate) async fn result(&self, run: &str, call_id: &str, text: impl Into<String>, is_error: bool) -> Result<()> {
        self.emit(
            "kernel",
            Ev::ToolResult {
                run: run.into(),
                call_id: call_id.into(),
                key: None,
                status: if is_error { "failed".into() } else { "done".into() },
                text: text.into(),
                is_error,
                label: Label::PRINCIPAL,
                data: None,
                sources: vec![],
                check: false,
            },
        )
        .await?;
        Ok(())
    }

    /// An inbox item for a run record. Its id comes from the call, so a crash
    /// between the item and the call's result does not make a second one.
    /// An inbox item for the run, unless it is already there: its id, and the event to write when it is new.
    fn item_event(&self, run: &Run, kind: &str, summary: String, mut details: Value) -> (String, Option<Ev>) {
        use sha2::Digest;
        let call = details["call_id"].as_str().unwrap_or_default().to_string();
        let id = if call.is_empty() { new_id("item") } else { format!("item_{}", &hex::encode(sha2::Sha256::digest(format!("{}\n{call}\n{kind}", run.id).as_bytes()))[..32]) };
        if self.read(|s| s.inbox.items().contains_key(&id)) {
            return (id, None);
        }
        details["taint"] = json!({ "untrusted": run.untrusted, "private": run.private });
        let ev = Ev::Item { id: id.clone(), item_kind: kind.into(), task: Some(run.task.clone()), run: Some(run.id.clone()), person: run.person.clone(), summary, details };
        (id, Some(ev))
    }

    pub(crate) async fn item(&self, run: &Run, kind: &str, summary: String, details: Value) -> Result<String> {
        let (id, ev) = self.item_event(run, kind, summary, details);
        if let Some(ev) = ev {
            self.emit("kernel", ev).await?;
        }
        Ok(id)
    }

    pub(crate) async fn record_call(self: &Arc<Self>, run: &Run, call_id: &str, _step_id: &str, name: &str, input: &Value) -> Result<Flow> {
        let rid = run.id.as_str();
        // The one rule of what a run may use (`admission`): a chat turn or a
        // team run has no contract, plan or Done, and ends with its answer.
        match self.admits(run, name) {
            Ok(()) => {}
            Err(admission::Refusal::Halted { reason, .. }) => {
                self.suspend(rid, &reason, None).await?;
                return Ok(Flow::Stop);
            }
            Err(admission::Refusal::Denied { why, .. }) => {
                self.result(rid, call_id, why, true).await?;
                return Ok(Flow::Continue);
            }
        }
        if name == "show" {
            return self.show(run, call_id, input).await;
        }
        if let Some(flow) = self.ui_record(run, call_id, name, input).await {
            return flow;
        }
        match name {
            // Retired by spec rev 147: no contract is recorded, and none is needed.
            "task_contract" => self.result(rid, call_id, CONTRACT_RETIRED, false).await?,
            "task_plan" => match validate_plan(input) {
                Ok(()) => {
                    let version = run.plan_version + 1;
                    self.emit("model", Ev::Plan { run: rid.into(), version, plan: input.clone() }).await?;
                    self.result(rid, call_id, format!("Progress recorded (version {version}); it is shown on the task's card."), false).await?;
                }
                Err(why) => self.result(rid, call_id, format!("The plan was not recorded: {why}. A retry with the same plan will not help."), true).await?,
            },
            "task_complete" => return self.complete(run, call_id, input).await,
            "task_fail" => {
                let reason = input["reason"].as_str().unwrap_or("no reason given").to_string();
                self.result(rid, call_id, "Recorded.", false).await?;
                // A failure is never silent (K24): the person's inbox says so, with the evidence, in the same write.
                // Background work's failure is told to the run inspector, not the person (owner, 2026-11-07).
                let mut evs = vec![("model", Ev::Failed { run: rid.into(), reason: reason.clone(), by: "model".into() })];
                // One whose outcome returns to its thread is told there by the agent (outcomes.rs).
                if !self.run_kind(rid).is_background() && !self.read(|s| s.outcome_return(rid)).is_some_and(|o| o.replaces_notice()) {
                    evs.push((
                        "kernel",
                        self.notice_of(
                            run,
                            format!("Failed: {}", self.title_of(run).await),
                            json!({ "reason": "failed", "evidence": { "reason": reason, "last_results": self.last_results(run) } }),
                        ),
                    ));
                }
                self.emit_all(evs).await?;
                self.on_stop(rid, true).await;
                self.after_end(rid);
                return Ok(Flow::Stop);
            }
            "ask_principal" => {
                let q = input["question"].as_str().unwrap_or_default().to_string();
                let (item, ev) = self.item_event(run, "question", q.clone(), json!({ "call_id": call_id, "question": q, "options": input["options"] }));
                self.suspend_on(rid, "question", item, ev).await?;
                return Ok(Flow::Stop);
            }
            "propose_policy" => {
                let layer = input["layer"].as_str().unwrap_or_default().to_string();
                let cedar = input["cedar"].as_str().unwrap_or_default().to_string();
                let layer_ok = crate::policy::layers::Layer::parse(&layer).is_some_and(|l| l.proposable_by(&run.person));
                match (layer_ok, crate::policy::validate(&cedar)) {
                    (true, Ok(())) => {
                        if let Err(why) = crate::policy::layers::tightens_only(&layer, &cedar) {
                            self.result(rid, call_id, format!("{why} A retry with the same text will not help."), true).await?;
                        } else if let Some(denied) = self.loop_deny(run, "suggest", None) {
                            self.result(rid, call_id, format!("{denied}. A retry will not help."), true).await?;
                        } else {
                            let item = self
                                .item(
                                    run,
                                    "policy_proposal",
                                    format!("The agent proposes a policy change for {layer}."),
                                    json!({ "layer": layer, "cedar": cedar, "rationale": input["rationale"], "call_id": call_id }),
                                )
                                .await?;
                            self.result(rid, call_id, format!("Proposal {item} sent to whoever owns the {layer} policy. It changes nothing until they accept it."), false).await?;
                        }
                    }
                    (false, _) => {
                        self.result(rid, call_id, format!("Unknown layer: use org, group/<id> or user/{}. A retry with the same layer will not help.", run.person), true).await?
                    }
                    (_, Err(e)) => self.result(rid, call_id, format!("The policy does not parse: {e}. A retry with the same text will not help."), true).await?,
                }
            }
            "end_schedule" => match self.read(|s| s.task_schedule(&run.task)) {
                Some((id, sc)) => {
                    let reason = input["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).unwrap_or("its run ended it").to_string();
                    let title = self.schedule_title(&sc).await?;
                    let notice = self.notice_to(
                        Some(run.task.clone()),
                        Some(rid.to_string()),
                        sc.person.clone(),
                        format!("A schedule ended itself: {title}"),
                        json!({ "reason": "schedule_ended", "evidence": { "schedule": id, "why": reason } }),
                    );
                    self.emit_all(vec![("model", Ev::ScheduleEnded { id, reason, run: Some(rid.to_string()) }), ("kernel", notice)]).await?;
                    self.result(rid, call_id, "The schedule has ended; no more runs will start. This run goes on.", false).await?;
                }
                None => self.result(rid, call_id, "This task was not started by a schedule that is still set. A retry will not help.", true).await?,
            },
            "wait_until" => {
                let at = input["at"].as_str().unwrap_or_default();
                match chrono::DateTime::parse_from_rfc3339(at) {
                    Ok(t) => {
                        let until = t.with_timezone(&chrono::Utc);
                        self.result(rid, call_id, format!("Waiting until {}.", until.to_rfc3339()), false).await?;
                        self.emit("model", Ev::Wait { run: rid.into(), until }).await?;
                        self.suspend(rid, "timer", None).await?;
                        return Ok(Flow::Stop);
                    }
                    Err(e) => self.result(rid, call_id, format!("Not an RFC 3339 time ({e}). A retry with the same value will not help."), true).await?,
                }
            }
            "notify" => {
                let m = input["message"].as_str().unwrap_or_default().to_string();
                self.item(run, "message", m, json!({ "from": "agent", "call_id": call_id })).await?;
                self.result(rid, call_id, "Sent.", false).await?;
            }
            "observe" => {
                let (what, why, evidence) =
                    (input["what"].as_str().unwrap_or_default().trim(), input["why"].as_str().unwrap_or_default().trim(), input["evidence"].as_str().unwrap_or_default().trim());
                if what.is_empty() || why.is_empty() || evidence.is_empty() {
                    self.result(rid, call_id, "observe needs what, why and evidence. A retry with the same values will not help.", true).await?;
                } else if let Some(denied) = self.loop_deny(run, "suggest", None) {
                    self.result(rid, call_id, format!("{denied}. A retry will not help."), true).await?;
                } else {
                    let at = self.notify_due(&run.person);
                    let item =
                        self.item(run, "observation", what.to_string(), json!({ "why": why, "evidence": evidence, "call_id": call_id, "notify_at": at.to_rfc3339() })).await?;
                    self.result(rid, call_id, format!("Queued as observation {item} for the person's next session."), false).await?;
                }
            }
            "interrupt" => {
                let (what, why) = (input["what"].as_str().unwrap_or_default().trim(), input["why"].as_str().unwrap_or_default().trim());
                if what.is_empty() || why.is_empty() {
                    self.result(rid, call_id, "interrupt needs what and why. A retry with the same values will not help.", true).await?;
                } else {
                    let (said, err) = self.interrupt_for(run, what, why, call_id).await?;
                    self.result(rid, call_id, said, err).await?;
                }
            }
            _ => self.result(rid, call_id, format!("{name} is not a run record."), true).await?,
        }
        Ok(Flow::Continue)
    }

    /// A chat turn shows components below its answer (U8). Their shape is
    /// checked like a deliverable's; a reply of answer text and `show` calls
    /// only is the turn's last, so the text streams first and the components
    /// are drawn after it.
    async fn show(self: &Arc<Self>, run: &Run, call_id: &str, input: &Value) -> Result<Flow> {
        let rid = run.id.as_str();
        if !self.run_kind(rid).is_chat_turn() {
            self.result(rid, call_id, "show is for chat. A task's outcome is the deliverable of task_complete.", true).await?;
            return Ok(Flow::Continue);
        }
        if let Err(why) = crate::compose::validate_shown(&input["components"]) {
            self.result(rid, call_id, format!("Nothing was shown: {why}. Fix it; a retry with the same components will not help."), true).await?;
            return Ok(Flow::Continue);
        }
        self.result(rid, call_id, "Shown below your answer.", false).await?;
        let Some(run) = self.read(|s| s.runs.get(rid).cloned()) else { return Ok(Flow::Stop) };
        let Some(last) = run.replies.last() else { return Ok(Flow::Continue) };
        let answer = last.text();
        if run.pending_calls().is_empty() && !answer.is_empty() && last.calls().iter().all(|(_, n, _)| n == "show") {
            self.end_chat_turn(&run, answer).await?;
            return Ok(Flow::Stop);
        }
        Ok(Flow::Continue)
    }

    /// A chat turn ends with its answer and what it showed: no contract, no
    /// Done check. What it did on the way went through execute().
    pub(crate) async fn end_chat_turn(self: &Arc<Self>, run: &Run, answer: String) -> Result<()> {
        let rid = run.id.as_str();
        let run = self.read(|s| s.runs.get(rid).cloned()).unwrap_or_else(|| run.clone());
        // What the person asked for and did not happen is in the inbox too (K24), in the same write as the turn's end.
        // In an outcome turn they asked nothing: a refused retry is the limit at work, which the answer reports.
        let unfinished = self.unfinished_chat_actions(&run);
        let mut evs = vec![("kernel", Ev::Completed { run: rid.into(), summary: answer, view: run.shown(), spoken: None })];
        if !unfinished.is_empty() && self.run_kind(rid) != kind::RunKind::OutcomeTurn {
            evs.push(("kernel", self.notice_of(&run, format!("Not done: {}", self.title_of(&run).await), json!({ "reason": "chat_action_failed", "evidence": unfinished }))));
        }
        self.emit_all(evs).await?;
        self.on_stop(rid, true).await;
        self.after_end(rid);
        Ok(())
    }

    /// The agent reports the work done (spec rev 147). Nothing is put to a
    /// contract: the result carries what the record proves. The server
    /// attaches the receipts and read-backs from the boundary log; the
    /// agent's sources are checked against the calls it made, and its
    /// deliverable's shape, never its content.
    async fn complete(self: &Arc<Self>, run: &Run, call_id: &str, input: &Value) -> Result<Flow> {
        let rid = run.id.as_str();
        let view = input.get("deliverable").cloned().unwrap_or(Value::Null);
        let spoken = input["spoken"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
        let files: Vec<String> = run.outputs.iter().map(|(_, n, ..)| n.clone()).collect();
        if let Err(why) = crate::compose::validate(&view, &files) {
            self.result(rid, call_id, format!("The deliverable was not accepted: {why}. Fix it; a retry with the same deliverable will not help."), true).await?;
            return Ok(Flow::Continue);
        }
        if spoken.as_ref().is_some_and(|t| t.chars().count() > crate::compose::MAX_SPOKEN) {
            self.result(rid, call_id, format!("The spoken script is longer than {} characters; shorten it.", crate::compose::MAX_SPOKEN), true).await?;
            return Ok(Flow::Continue);
        }
        let sources = match crate::evidence::cited(run, &input["sources"]) {
            Ok(s) => s,
            Err(why) => {
                self.result(rid, call_id, format!("The result was not accepted: {why}. A retry with the same sources will not help."), true).await?;
                return Ok(Flow::Continue);
            }
        };
        let summary = input["summary"].as_str().unwrap_or_default().to_string();
        // A read-back that found something other than what was done is
        // said back once, before the result stands (spec rev 147).
        let differs: Vec<String> = self.read(|s| s.evidence.read_backs(rid).iter().filter(|r| r.matches == Some(false)).map(|r| r.tool.clone()).collect());
        let mut evs = vec![];
        if !sources.is_empty() {
            evs.push(("model", Ev::Evidence(crate::evidence::Change::Sources { run: rid.into(), sources })));
        }
        evs.push(("kernel", Ev::Completed { run: rid.into(), summary, view: Some(view), spoken }));
        let said = if differs.is_empty() {
            "Completed. The result shows the receipts and read-backs from the record.".to_string()
        } else {
            format!("Completed. The result shows, under not done, that the read-back after {} did not match.", differs.join(", "))
        };
        self.result(rid, call_id, said, false).await?;
        self.emit_all(evs).await?;
        self.on_stop(rid, true).await;
        self.after_end(rid);
        Ok(Flow::Stop)
    }
}

/// Shape only, never content (K36).
fn validate_plan(p: &Value) -> std::result::Result<(), String> {
    let units = p["units"].as_array().filter(|u| !u.is_empty()).ok_or("it has no units")?;
    if units.len() > 50 {
        return Err("at most 50 units; group some".into());
    }
    let mut ids = std::collections::HashSet::new();
    for u in units {
        let id = u["id"].as_str().filter(|s| !s.is_empty()).ok_or("a unit has no id")?;
        if !ids.insert(id) {
            return Err(format!("unit id {id} is used twice"));
        }
        if u["goal"].as_str().map(str::trim).unwrap_or_default().is_empty() {
            return Err(format!("unit {id} has no goal"));
        }
        if let Some(st) = u.get("status").filter(|v| !v.is_null()) {
            if !["pending", "active", "done", "dropped"].contains(&st.as_str().unwrap_or_default()) {
                return Err(format!("unit {id}: status is one of pending, active, done, dropped"));
            }
        }
    }
    Ok(())
}
