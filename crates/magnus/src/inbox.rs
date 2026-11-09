//! The inbox: every human decision (U1). Items reach people through this API
//! only, outside the agent's context (B5); the agent sees only that it waits.

use crate::config::Person;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::{Item, State, Status};
use crate::tools::Label;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub enum Answer {
    Ok,
    Forbidden(&'static str),
    NotFound,
    Conflict(&'static str),
    Invalid(String),
}

/// May `p` set policy for `layer`? (E7: separate from approving.)
pub fn may_set_policy(p: &Person, layer: &str) -> bool {
    if p.is_owner() {
        return true;
    }
    if let Some(u) = layer.strip_prefix("user/") {
        return u == p.id && p.has_role("policy:user");
    }
    if let Some(g) = layer.strip_prefix("group/") {
        return p.has_role(&format!("policy:group:{g}"));
    }
    false
}

pub fn may_respond(s: &State, p: &Person, i: &Item) -> bool {
    match i.kind.as_str() {
        // A workspace rule that names approvers for this resource decides who.
        "approval" if named_approvers(s, i).is_some() => named_approvers(s, i).is_some_and(|named| named.contains(&p.id)),
        "approval" => p.is_owner() || p.has_role(&format!("approver:{}", i.details["risk"].as_str().unwrap_or_default())),
        "policy_proposal" => may_set_policy(p, i.details["layer"].as_str().unwrap_or_default()),
        "tool_change" | "tool_review" | "deletion_request" => p.is_owner(),
        // A message shared with someone is theirs alone, the owner's too.
        "shared" => i.person == p.id,
        // Only someone with the role who may read the entry makes it a company fact.
        "endorsement" => {
            crate::knowledge::may_endorse(p)
                && crate::knowledge::can_read(p, &serde_json::from_value::<Vec<Vec<String>>>(i.details["readers"].clone()).unwrap_or_else(|_| vec![vec![]]))
        }
        // A task's questions, reviews and notices are its person's alone.
        _ => i.person == p.id,
    }
}

fn named_approvers(s: &State, i: &Item) -> Option<Vec<String>> {
    s.team.approvers(i.task.as_deref()?, i.details["tool"].as_str()?)
}

pub fn visible(s: &State, p: &Person, i: &Item) -> bool {
    i.person == p.id || may_respond(s, p, i)
}

/// What an approval's risk means for the person deciding (U6): the
/// consequence of a destructive action is stated on the card, never left to
/// the details.
fn consequence(i: &Item) -> Option<&'static str> {
    if i.kind != "approval" {
        return None;
    }
    if i.details["reason"] == "unknown_outcome" {
        return Some("It may already have happened once. Approving sends it again.");
    }
    match i.details["risk"].as_str().unwrap_or_default() {
        "irreversible" => Some("This cannot be undone once it runs."),
        "financial" => Some("This spends or moves money and cannot be undone."),
        "reversible_write" => Some("This changes data outside the appliance. It can be undone afterwards."),
        _ => None,
    }
}

/// An item blocks when a run is paused waiting on it; the app interrupts
/// only for those and batches the rest (U3).
pub fn blocking(s: &State, i: &Item) -> bool {
    i.resolution.is_none() && i.run.as_ref().and_then(|r| s.runs.get(r)).is_some_and(|r| r.status == Status::Suspended && r.waiting_item.as_deref() == Some(&i.id))
}

pub fn render(s: &State, p: &Person, i: &Item) -> Value {
    let tier = match i.kind.as_str() {
        "approval" => i.details["risk"].as_str().unwrap_or("unknown").to_string(),
        k => k.to_string(),
    };
    // The conversation its task came from, for the person whose it is: the
    // same card shows there as in the inbox.
    let thread = i.task.as_ref().and_then(|t| s.tasks.get(t)).filter(|t| t.person == p.id).and_then(|t| t.chat.as_ref()).map(|c| c.thread.clone());
    json!({
        "id": i.id,
        "kind": i.kind,
        "task_id": i.task,
        "thread": thread,
        "run_id": i.run,
        "person": i.person,
        "summary": i.summary,
        "details": i.details,
        "created_at": i.created_at,
        "opened_at": s.items_opened.get(&i.id),
        "status": if i.resolution.is_some() { "resolved" } else { "open" },
        "resolution": i.resolution.as_ref().map(|(d, s, t, by)| json!({ "decision": d, "scope": s, "text": t, "by": by })),
        "can_respond": i.resolution.is_none() && may_respond(s, p, i),
        "blocking": blocking(s, i),
        "tier": tier,
        "consequence": consequence(i),
    })
}

impl Kernel {
    pub async fn respond(self: &Arc<Self>, p: &Person, item_id: &str, body: &Value) -> Result<Answer> {
        let _decide = self.decide.lock().await;
        let Some(item) = self.read(|s| s.items.get(item_id).cloned()) else { return Ok(Answer::NotFound) };
        if !self.read(|s| visible(s, p, &item)) {
            return Ok(Answer::NotFound);
        }
        if !self.read(|s| may_respond(s, p, &item)) {
            return Ok(Answer::Forbidden("you do not hold the role this decision needs"));
        }
        if item.resolution.is_some() {
            return Ok(Answer::Conflict("already decided"));
        }
        let decision = body["decision"].as_str().unwrap_or_default().to_string();
        let text = body["text"].as_str().map(String::from);
        let actor = format!("person:{}", p.id);
        let run = item.run.clone().unwrap_or_default();
        let call_id = item.details["call_id"].as_str().unwrap_or_default().to_string();
        let resolve = |scope: Option<String>| Ev::ItemResolved { id: item.id.clone(), decision: decision.clone(), scope, text: text.clone() };
        // The decision and everything it causes go to the log in one write
        // (#194): a crash leaves all of it or none, never a decided card
        // whose grant, result or resumption is missing.
        let mut evs: Vec<(&str, Ev)> = Vec::new();
        // The run ends with this decision: its resources go once it is written.
        let mut ended = false;
        // An entry made a company fact is searched as one once it is written.
        let mut endorsed = None;
        // A task an approval started is driven from now.
        let mut started: Option<String> = None;

        match (item.kind.as_str(), decision.as_str()) {
            ("approval", "approve") => {
                let scope = body["scope"].as_str().unwrap_or("once").to_string();
                if !["once", "task", "window", "standing"].contains(&scope.as_str()) {
                    return Ok(Answer::Invalid(format!("unknown scope {scope}")));
                }
                let rule = item.details["rule"].as_str().unwrap_or_default();
                let unknown = item.details["reason"] == "unknown_outcome";
                // Web lookups after reading private, untrusted data may be
                // allowed for the whole task (B5); other taint asks only once.
                let lookups = rule == crate::boundary::WEB_LOOKUPS;
                if lookups && !["once", "task"].contains(&scope.as_str()) {
                    return Ok(Answer::Invalid("web lookups are allowed once or for this task".into()));
                }
                if scope != "once" && !lookups && (rule.starts_with("kernel:") || unknown) {
                    return Ok(Answer::Invalid("this action can only be approved once: it is asked because of what the task has read, or because its outcome is unknown".into()));
                }
                // A standing approval is autonomy; only the owner grants it (invariant 6).
                if scope == "standing" && !p.is_owner() {
                    return Ok(Answer::Forbidden("only the owner can approve standing; propose a policy instead"));
                }
                let until = match scope.as_str() {
                    "window" => Some(chrono::Utc::now() + chrono::Duration::minutes(body["minutes"].as_i64().unwrap_or(60).clamp(1, 24 * 60))),
                    _ => None,
                };
                let reason = if unknown { Some("unknown_outcome".to_string()) } else { item.details["rule"].as_str().map(String::from) };
                evs.push((&actor, resolve(Some(scope.clone()))));
                evs.push((
                    &actor,
                    Ev::Approval {
                        id: new_id("appr"),
                        item: item.id.clone(),
                        person: item.person.clone(),
                        task: item.task.clone().unwrap_or_default(),
                        tool: item.details["tool"].as_str().unwrap_or_default().into(),
                        input_hash: item.details["input_hash"].as_str().map(String::from),
                        scope,
                        until,
                        reason,
                        risk: item.details["risk"].as_str().map(String::from),
                    },
                ));
                evs.extend(self.resume(&run));
            }
            // They would rather not sign in: the agent is told and goes on.
            ("secure_form", "decline") => {
                let steps = self.read(|st| st.runs.get(&run).map_or(0, |r| r.replies.len() as u32));
                evs.push((&actor, resolve(None)));
                evs.push((
                    "kernel",
                    Ev::Notice { run: run.clone(), step: steps, text: "The person chose not to sign in, so that site is not available to you for this task.".to_string() },
                ));
                evs.extend(self.resume(&run));
            }
            ("approval", "reject") => {
                evs.push((&actor, resolve(None)));
                let unknown = item.details["reason"] == "unknown_outcome";
                let msg = if unknown {
                    format!("{} was treated as not done, by the person's decision. {}", item.details["tool"].as_str().unwrap_or_default(), text.clone().unwrap_or_default())
                } else {
                    format!(
                        "The person declined this action{}. A retry of the same action will not help; decide what to do instead.",
                        text.as_ref().map(|t| format!(": {t}")).unwrap_or_default()
                    )
                };
                evs.extend(reply(&run, &call_id, msg, true, Label::PRINCIPAL));
                evs.extend(self.resume(&run));
            }
            ("review", "accept") => {
                evs.push((&actor, resolve(None)));
                for c in item.details["criteria"].as_array().cloned().unwrap_or_default() {
                    evs.push((
                        &actor,
                        Ev::Verification {
                            run: run.clone(),
                            criterion: c["id"].as_str().unwrap_or_default().into(),
                            passed: true,
                            evidence: json!({ "reviewed_by": p.id, "note": text }),
                            by: actor.clone(),
                        },
                    ));
                }
                evs.extend(reply(&run, &call_id, "The person reviewed the work and accepted it.", false, Label::PRINCIPAL));
                evs.push((
                    &actor,
                    Ev::Completed {
                        run: run.clone(),
                        summary: item.details["summary"].as_str().unwrap_or_default().into(),
                        view: item.details.get("view").filter(|v| !v.is_null()).cloned(),
                        spoken: item.details["spoken"].as_str().map(String::from),
                    },
                ));
                ended = true;
            }
            ("review", "reject") => {
                evs.push((&actor, resolve(None)));
                let msg = format!(
                    "The person reviewed the work and did not accept it{}. The task stays open; a retry without changes will not help.",
                    text.as_ref().map(|t| format!(": {t}")).unwrap_or_default()
                );
                evs.extend(reply(&run, &call_id, msg, true, Label::PRINCIPAL));
                evs.extend(self.resume(&run));
            }
            ("contract_change", "approve") => {
                let version = self.read(|s| s.runs.get(&run).map(|r| r.contract_version)).unwrap_or(0) + 1;
                evs.push((&actor, resolve(None)));
                evs.push((&actor, Ev::Contract { run: run.clone(), version, contract: item.details["new"].clone(), approved_by: Some(actor.clone()) }));
                evs.extend(reply(&run, &call_id, format!("The person approved the contract change. Version {version} is in force, and frozen."), false, Label::PRINCIPAL));
                evs.extend(self.resume(&run));
            }
            ("contract_change", "reject") => {
                evs.push((&actor, resolve(None)));
                let msg = format!(
                    "The person kept the contract (version {}){}. Its criteria still decide when the task is done: finish against them, or call task_fail.",
                    item.details["version"],
                    text.as_ref().map(|t| format!(": {t}")).unwrap_or_default()
                );
                evs.extend(reply(&run, &call_id, msg, true, Label::PRINCIPAL));
                evs.extend(self.resume(&run));
            }
            ("question", "answer") => {
                evs.push((&actor, resolve(None)));
                evs.extend(reply(&run, &call_id, format!("The person answered: {}", text.clone().unwrap_or_default()), false, Label::PRINCIPAL));
                evs.extend(self.resume(&run));
            }
            ("policy_proposal", "accept") => {
                let layer = item.details["layer"].as_str().unwrap_or_default().to_string();
                let add = item.details["cedar"].as_str().unwrap_or_default();
                let current = self.read(|s| s.policies.get(&layer).cloned()).unwrap_or_else(|| if layer == "org" { crate::policy::DEFAULT_ORG.to_string() } else { String::new() });
                let next = format!("{current}\n{add}\n");
                if let Err(e) = crate::policy::validate(&next) {
                    return Ok(Answer::Invalid(format!("the combined policy does not parse: {e}")));
                }
                evs.push((&actor, resolve(None)));
                evs.push((&actor, Ev::PolicySet { layer, cedar: next }));
            }
            // A schedule a worker proposed runs within the proposing run's
            // limits and starts with its taint, as a chat card's does: it
            // cannot get more than the run that asked for it had.
            ("schedule_proposal", "accept") => {
                let from = self.read(|s| item.run.as_ref().and_then(|r| s.runs.get(r)).map(|r| (r.config.budget.clone(), Label { untrusted: r.untrusted, private: r.private })));
                evs.push((&actor, resolve(None)));
                evs.push((
                    &actor,
                    Ev::ScheduleSet {
                        id: new_id("sched"),
                        person: item.person.clone(),
                        text: item.details["text"].as_str().unwrap_or_default().into(),
                        every_minutes: item.details["every_minutes"].as_u64().map(|m| m as u32),
                        daily_at: item.details["daily_at"].as_str().map(String::from),
                        // Never more than the appliance gives a run: an
                        // extension the person gave that one run stays its own.
                        budget: from.as_ref().map(|f| f.0.within(&self.cfg.budget)),
                        card: None,
                        label: from.map(|f| f.1),
                        title: None,
                        until: serde_json::from_value(item.details["until"].clone()).ok().flatten(),
                        max_runs: item.details["max_runs"].as_u64().map(|m| m as u32),
                        scope: None,
                    },
                ));
            }
            // A task a review proposed starts as a chat card's does: the
            // card's budget, scope and title, the proposing run's model
            // and taint, and it starts now (#393).
            ("task_proposal", "accept") => {
                if self.read(|s| s.kill_switch) {
                    return Ok(Answer::Conflict("the kill switch is engaged"));
                }
                let from = self.read(|s| {
                    item.run.as_ref().and_then(|r| s.runs.get(r)).map(|r| (r.config.adapter.clone(), r.config.model.clone(), Label { untrusted: r.untrusted, private: r.private }))
                });
                let budget =
                    serde_json::from_value(item.details["budget"].clone()).map(|b: crate::config::Budget| b.within(&self.cfg.budget)).unwrap_or_else(|_| self.cfg.budget.clone());
                let (adapter, model) = from.as_ref().map(|f| (f.0.clone(), f.1.clone())).unwrap_or_else(|| self.default_model());
                let (modules, escalate_to) = self.modules_for(&adapter, &model);
                let m = &self.cfg.model;
                let fresh = new_id("run");
                evs.push((&actor, resolve(None)));
                evs.push((
                    &actor,
                    Ev::TaskCreated {
                        task: new_id("task"),
                        run: fresh.clone(),
                        person: item.person.clone(),
                        text: item.details["text"].as_str().unwrap_or_default().into(),
                        attachments: vec![],
                        config: crate::events::RunConfig {
                            adapter,
                            model,
                            max_retries: m.max_retries,
                            max_tokens: m.max_tokens,
                            budget,
                            verifier: modules.get("verifier").copied().unwrap_or(false),
                            modules,
                            escalate_to,
                            lost: self.cfg.modules.lost.clone(),
                            verifier_model: self.cfg.verifier.model.clone(),
                            instructions: crate::run::instructions_version(),
                            harness: crate::kernel::HARNESS_VERSION.into(),
                            playbook: self.read(|s| s.playbook_version),
                        },
                        experiment: None,
                        chat: None,
                        label: from.map(|f| f.2),
                        title: item.details["title"].as_str().map(String::from),
                        files: vec![],
                        project: None,
                        scope: item.details["scope"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from),
                        grant: None,
                    },
                ));
                started = Some(fresh);
            }
            // An entry becomes a company fact; its source stays attached, and
            // who endorsed it, from which card, is in the log.
            ("endorsement", "endorse") => {
                let entry = item.details["entry"].as_str().unwrap_or_default().to_string();
                if self.read(|s| s.knowledge.get(&entry).map(|e| e.integrity.clone())).is_none() {
                    return Ok(Answer::Invalid("the entry no longer exists".into()));
                }
                evs.push((&actor, resolve(None)));
                evs.push((&actor, Ev::KnowledgeEndorsed { id: entry.clone(), by: p.id.clone(), item: Some(item.id.clone()), note: text.clone() }));
                endorsed = Some(entry);
            }
            ("endorsement", "decline") => {
                evs.push((&actor, resolve(None)));
            }
            // The owner says what became of a request to delete someone's
            // data; the note tells the person.
            ("deletion_request", "done") | ("deletion_request", "decline") => {
                evs.push((&actor, resolve(None)));
            }
            ("schedule_proposal", "reject")
            | ("task_proposal", "reject")
            | ("policy_proposal", "reject")
            | ("notice", "dismiss")
            | ("message", "dismiss")
            | ("shared", "dismiss")
            | ("tool_change", "reject")
            | ("tool_review", "reject")
            | ("observation", "dismiss")
            | ("interrupt", "dismiss") => {
                evs.push((&actor, resolve(None)));
            }
            // Only the kernel's own notices (which name a reason) can continue a
            // run; a message the agent wrote cannot.
            // A schedule's own notices (paused, ended) stand for no run to go
            // on: the schedule's Resume does that, and nothing ends twice.
            ("notice", "extend") | ("notice", "resume")
                if item.details["reason"].is_string()
                    && !item.details["reason"].as_str().unwrap_or_default().starts_with("schedule_")
                    && item.run.is_some()
                    && (decision != "extend" || item.details["reason"] == "budget") =>
            {
                // The kill switch's notice is the person's own Continue for
                // the run it stopped, as the task's Resume is (K53: nothing
                // else resumes it), and never while the switch is on again.
                let stopped = item.details["reason"] == "kill_switch"
                    && self.read(|s| s.runs.get(&run).is_some_and(|r| r.status == Status::Suspended && r.reason.as_deref() == Some("kill_switch")));
                if stopped && self.read(|s| s.kill_switch) {
                    return Ok(Answer::Conflict("the kill switch is engaged"));
                }
                evs.push((&actor, resolve(None)));
                if stopped {
                    evs.push((&actor, Ev::Resumed { run: run.clone() }));
                } else {
                    if decision == "extend" {
                        if let Some(mut b) = self.read(|s| s.runs.get(&run).map(|r| r.config.budget.clone())) {
                            b.max_model_calls *= 2;
                            b.max_actions *= 2;
                            b.max_usd *= 2.0;
                            evs.push((&actor, Ev::BudgetExtended { run: run.clone(), budget: b }));
                        }
                    }
                    evs.extend(self.resume(&run));
                }
            }
            ("tool_change", "approve") => {
                evs.push((&actor, resolve(None)));
                evs.push((
                    &actor,
                    Ev::ToolPinned {
                        tool: item.details["tool"].as_str().unwrap_or_default().into(),
                        hash: item.details["hash"].as_str().unwrap_or_default().into(),
                        description: item.details["description"].as_str().unwrap_or_default().into(),
                        input_schema: item.details["input_schema"].clone(),
                        approved: true,
                    },
                ));
            }
            ("tool_review", "approve") => {
                evs.push((&actor, resolve(None)));
                for t in item.details["tools"].as_array().cloned().unwrap_or_default() {
                    let name = t["tool"].as_str().unwrap_or_default().to_string();
                    // Approve only definitions that are still the pinned ones.
                    let current = self.read(|s| s.pins.get(&name).map(|p| p.hash.clone()));
                    if current.as_deref() == t["hash"].as_str() {
                        evs.push((
                            &actor,
                            Ev::ToolPinned {
                                tool: name,
                                hash: t["hash"].as_str().unwrap_or_default().into(),
                                description: t["description"].as_str().unwrap_or_default().into(),
                                input_schema: t["input_schema"].clone(),
                                approved: true,
                            },
                        ));
                    }
                }
            }
            (kind, d) => return Ok(Answer::Invalid(format!("{d:?} is not a decision for a {kind} item"))),
        }
        // A run resumed here is driven from now. One whose driver a crash
        // stopped is running in the log, and the kernel's sweep drives it.
        let resumed = evs.iter().any(|(_, e)| matches!(e, Ev::Resumed { .. }));
        self.emit_all(evs).await?;
        if resumed {
            self.wake(&run);
        }
        if let Some(id) = started {
            self.wake(&id);
        }
        if ended {
            self.on_stop(&run, true).await;
        }
        if let Some(entry) = endorsed {
            self.index_id(&entry).await?;
        }
        Ok(Answer::Ok)
    }

    /// A run waiting on a person goes on, unless the kill switch holds it.
    pub(crate) fn resume(&self, run: &str) -> Option<(&'static str, Ev)> {
        if run.is_empty() {
            return None;
        }
        let (suspended, reason) = self.read(|s| s.runs.get(run).map(|r| (r.status == Status::Suspended, r.reason.clone()))).unwrap_or((false, None));
        (suspended && reason.as_deref() != Some("kill_switch") && !self.read(|s| s.kill_switch)).then(|| ("kernel", Ev::Resumed { run: run.into() }))
    }
}

/// What the tool call waiting on the card is told of the decision.
pub(crate) fn reply(run: &str, call_id: &str, text: impl Into<String>, is_error: bool, label: Label) -> Option<(&'static str, Ev)> {
    if run.is_empty() || call_id.is_empty() {
        return None;
    }
    Some((
        "kernel",
        Ev::ToolResult {
            run: run.into(),
            call_id: call_id.into(),
            key: None,
            status: if is_error { "failed".into() } else { "done".into() },
            text: text.into(),
            is_error,
            label,
            data: None,
            sources: vec![],
            check: false,
        },
    ))
}
