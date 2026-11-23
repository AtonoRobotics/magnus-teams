//! The developer view (V1-V7): what each run saw and did, experiments,
//! live panels, policy history and the playbook. Raw log access lives here
//! and only here, behind the developer role (K10, K45).

mod experiments;

pub(crate) use experiments::num;

use crate::anonymize::Sight;
use crate::api::{internal, status_str};
use crate::api_error::{refuse, ApiResult};
use crate::events::Ev;
use crate::extract::{Body, Developer};
use crate::kernel::Kernel;
use crate::state::{Run, State, Status};
use axum::{
    extract::{Path, Query, State as AxState},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

pub(super) type K = AxState<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/dev/runs", get(runs))
        .route("/v1/dev/runs/{id}", get(inspect))
        .route("/v1/dev/runs/{id}/log", get(raw_log))
        .route("/v1/dev/runs/{id}/grades", post(grade))
        .route("/v1/dev/metrics", get(crate::dev_panels::metrics))
        .route("/v1/dev/experiments", get(experiments::experiments))
        .route("/v1/dev/experiments/{id}", get(experiments::experiment))
        .route("/v1/dev/experiments/{id}/trials", post(experiments::add_trial))
        .route("/v1/dev/policy", get(crate::dev_panels::policy))
        .route("/v1/dev/playbook", get(crate::dev_panels::playbook))
        .route("/v1/dev/modules", get(modules))
}

/// Where a run went wrong, from its log (V2). None for a run that succeeded
/// or is still moving normally.
pub fn failure(s: &State, r: &Run, stall_minutes: u32) -> Option<&'static str> {
    match r.status {
        Status::Completed => r.grades.iter().any(|g| !g.1).then_some("grader_disagrees"),
        Status::Failed if r.checks_failed > 0 => Some("verification"),
        Status::Failed if r.decisions.iter().any(|d| d.outcome == "deny") => Some("boundary"),
        Status::Failed if r.model_errors > 0 && r.replies.is_empty() => Some("provider"),
        Status::Failed => Some("agent"),
        Status::Cancelled => Some("cancelled"),
        Status::Suspended => match r.reason.as_deref() {
            Some("budget") | Some("grant_budget") => Some("budget"),
            Some("provider") | Some("usage_limit") | Some("auth") | Some("invalid_request") => Some("provider"),
            Some(r) if crate::halts::is_kill(r) => Some("kill_switch"),
            Some("timer") => None,
            _ => Some("waiting_on_person"),
        },
        Status::Running => {
            let idle = chrono::Utc::now() - r.last_at;
            let _ = s;
            (idle > chrono::Duration::minutes(stall_minutes as i64)).then_some("stalled")
        }
    }
}

/// A trial passes when every grader passed it, each by its latest verdict
/// (a person may change a judgment); with no grader, when the kernel
/// verified it (and the result says which).
pub(super) fn verdict(r: &Run) -> (bool, &'static str) {
    if !r.grades.is_empty() {
        let latest: HashMap<&str, bool> = r.grades.iter().map(|g| (g.0.as_str(), g.1)).collect();
        (latest.values().all(|p| *p), "grader")
    } else {
        (r.status == Status::Completed, "kernel")
    }
}

pub(super) fn config_key(r: &Run) -> String {
    let c = &r.config;
    let off: Vec<&str> = MODULES.iter().filter(|m| !c.module(m.name)).map(|m| m.name).collect();
    let off = if off.is_empty() { String::new() } else { format!(" · without {}", off.join(", ")) };
    format!("{}/{} · {} · {} · playbook v{}{off}", c.adapter, c.model, c.harness, c.instructions, c.playbook)
}

/// A removable module: what model weakness it compensates for, and the
/// experiment that shows whether it still does (its ablation suite: the same
/// cases with it on and off, per model).
pub struct Module {
    pub name: &'static str,
    pub compensates: &'static str,
    pub ablation_suite: &'static str,
}

/// The removable modules: what each compensates for, its ablation suite,
/// whether it is on for the appliance's model and each model set apart, and
/// how often it acted.
async fn modules(AxState(k): K, Developer(p): Developer) -> ApiResult {
    let lens = k.lens(&p);
    let (a, m) = k.default_model();
    let first = format!("{a}/{m}");
    let mut models = vec![first.clone()];
    models.extend(k.cfg.modules.model.iter().map(|x| x.model.clone()).filter(|x| *x != first));
    let acted = k.read(|s| {
        let mut n: HashMap<&str, usize> = HashMap::new();
        for r in s.runs.values().filter(|r| lens.sight(&r.person) != Sight::Hidden) {
            *n.entry("lost_reset").or_default() += r.cuts.len();
            *n.entry("escalation").or_default() += r.escalated.is_some() as usize;
            *n.entry("reviewer").or_default() += r.reviewed as usize;
        }
        n.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<String, usize>>()
    });
    let list: Vec<Value> = MODULES
        .iter()
        .map(|md| {
            let on: Map<String, Value> = models
                .iter()
                .map(|key| {
                    let (a, m) = key.split_once('/').unwrap_or((key, ""));
                    let (on, to) = k.modules_for(a, m);
                    let v = if md.name == "escalation" { json!({ "on": on[md.name], "escalate_to": to }) } else { json!({ "on": on[md.name] }) };
                    (key.clone(), v)
                })
                .collect();
            json!({ "name": md.name, "compensates": md.compensates, "ablation_suite": md.ablation_suite, "models": on, "acted": acted.get(md.name).copied().unwrap_or(0) })
        })
        .collect();
    Ok(Json(json!({ "modules": list, "lost_thresholds": k.cfg.modules.lost })).into_response())
}

/// Each removable module on or off for a run, as pinned at its start.
pub(super) fn modules_of(r: &Run) -> Value {
    MODULES.iter().map(|m| (m.name.to_string(), json!(r.config.module(m.name)))).collect::<Map<String, Value>>().into()
}

pub const MODULES: &[Module] = &[
    Module {
        name: "lost_reset",
        compensates: "models that repeat a failing step instead of changing approach",
        ablation_suite: "tests/scratch/tests/modules.rs: a_looping_run_is_reset_and_recovers_and_without_the_module_runs_to_its_budget",
    },
    Module {
        name: "escalation",
        compensates: "models too weak for a task they keep failing at",
        ablation_suite: "tests/scratch/tests/modules.rs: a_run_lost_again_escalates_to_the_stronger_model",
    },
    Module {
        name: "memory_pass",
        compensates: "models that rarely choose to write down what they learn about the person",
        ablation_suite: "tests/scratch/tests/memory.rs: a_fact_stated_in_one_chat_is_recalled_in_a_new_one",
    },
    Module {
        name: "reviewer",
        compensates: "models that send without checking where it goes",
        ablation_suite: "tests/scratch/tests/modules.rs: the_reviewer_blocks_a_send_to_the_wrong_recipient",
    },
];

/// `w` holds the recalled words of the runs' tasks (`Kernel::all_words`).
fn summary(s: &State, r: &Run, stall: u32, w: &crate::recall::Words) -> Value {
    let title = s.tasks.get(&r.task).map(|t| w.text(t.seq).chars().take(100).collect::<String>()).unwrap_or_default();
    json!({
        "run": r.id, "task": r.task, "person": r.person, "title": title,
        "status": status_str(r.status), "reason": r.reason,
        "created_at": r.created_at, "ended_at": r.ended_at,
        "escalated_to": r.escalated.as_ref().map(|(a, m)| format!("{a}/{m}")),
        "config": {
            "modules": modules_of(r),
            "adapter": r.config.adapter, "model": r.config.model, "harness": r.config.harness,
            "instructions": r.config.instructions, "playbook": r.config.playbook, "key": config_key(r),
        },
        "model_calls": r.model_calls, "actions": r.actions, "usd": r.usd, "tokens": r.tokens,
        "experiment": r.experiment,
        "grades": r.grades.iter().map(|(g, p, sc, d)| json!({ "grader": g, "passed": p, "score": sc, "details": d })).collect::<Vec<_>>(),
        "failure": failure(s, r, stall),
    })
}

#[derive(serde::Deserialize, Default)]
struct RunQuery {
    status: Option<String>,
    experiment: Option<String>,
    failure: Option<String>,
    ids: Option<String>,
    limit: Option<usize>,
}

async fn runs(AxState(k): K, Developer(p): Developer, Query(q): Query<RunQuery>) -> ApiResult {
    let lens = k.lens(&p);
    let stall = k.cfg.observability.stall_minutes;
    let ids: Option<BTreeSet<String>> = q.ids.as_ref().map(|i| i.split(',').map(String::from).collect());
    let w = k.all_words().await?;
    let mut list: Vec<Value> = k.read(|s| {
        let mut rs: Vec<&Run> = s.runs.values().collect();
        rs.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        rs.into_iter()
            .filter(|r| q.status.as_deref().is_none_or(|st| status_str(r.status) == st))
            .filter(|r| q.experiment.as_deref().is_none_or(|e| r.experiment.as_ref().is_some_and(|x| x.id == e)))
            .filter(|r| q.failure.as_deref().is_none_or(|f| failure(s, r, stall) == Some(f)))
            .filter(|r| ids.as_ref().is_none_or(|i| i.contains(&r.id)))
            .filter_map(|r| lens.show(lens.sight(&r.person), summary(s, r, stall, &w)))
            .collect()
    });
    list.truncate(q.limit.unwrap_or(500));
    Ok(Json(json!({ "runs": list })).into_response())
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    format!("{}… [{} characters in the log]", s.chars().take(n).collect::<String>(), s.chars().count())
}

/// Replace large base64 payloads so a log view stays readable; the log keeps them.
fn shrink(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| match (k.as_str(), v) {
                    ("data_b64" | "image_b64", Value::String(s)) if s.len() > 120 => (k.clone(), json!(format!("[{} bytes of base64]", s.len()))),
                    _ => (k.clone(), shrink(v)),
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(shrink).collect()),
        other => other.clone(),
    }
}

/// Which tool each call of these events is, from the intents and replies
/// that name them, so a result (which carries no name) can be judged.
fn call_tools(events: &[crate::log::Event]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for e in events {
        match &e.ev {
            Ev::Intent { call_id, tool, .. } => {
                out.entry(call_id.clone()).or_insert_with(|| tool.clone());
            }
            Ev::ModelReply { blocks, .. } => {
                for b in blocks {
                    if let magnus_adapter_proto::Block::ToolCall { id, name, .. } = b {
                        out.entry(id.clone()).or_insert_with(|| name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Hide one person's relationship notes and disagreement words from another
/// developer reading this run's log (#257): the calls stay listed, with
/// their names and decisions, but their words do not come back.
fn redact_event_body(body: &mut Value, call_tool: &HashMap<String, String>) {
    let only = crate::tools::person_only_tool;
    match body["kind"].as_str().unwrap_or_default() {
        "model_reply" => {
            if let Some(blocks) = body["blocks"].as_array_mut() {
                for b in blocks {
                    if b["type"] == "tool_call" && only(b["name"].as_str().unwrap_or_default()) {
                        b["input"] = json!(crate::tools::REDACTED_CALL);
                    }
                }
            }
        }
        "intent" => {
            if only(body["tool"].as_str().unwrap_or_default()) {
                body["input"] = json!(crate::tools::REDACTED_CALL);
            }
        }
        "tool_result" => {
            let tool = body["call_id"].as_str().and_then(|c| call_tool.get(c));
            if tool.is_some_and(|t| only(t)) {
                body["text"] = json!(crate::tools::REDACTED_CALL);
                body["data"] = Value::Null;
            }
        }
        "disagreement_recorded" => {
            body["position"] = json!(crate::tools::REDACTED_CALL);
            body["decision"] = json!(crate::tools::REDACTED_CALL);
        }
        "disagreement_outcome" => {
            body["outcome"] = json!(crate::tools::REDACTED_CALL);
        }
        "item" if body["details"]["disagreement"].is_string() => {
            body["summary"] = json!(crate::tools::REDACTED_CALL);
            body["details"]["outcome"] = json!(crate::tools::REDACTED_CALL);
        }
        _ => {}
    }
}

fn issuer_of(actor: &str) -> &'static str {
    if actor == "model" {
        "model"
    } else if actor.starts_with("person:") {
        "person"
    } else {
        "code"
    }
}

#[derive(Default)]
struct Sources(BTreeMap<String, usize>);

impl Sources {
    fn add(&mut self, k: &str, n: usize) {
        *self.0.entry(k.to_string()).or_default() += n;
    }
}

/// The run inspector (V1): per step, what the model saw (context by source,
/// with tokens), what it did, and what the harness did, with who issued each
/// action (V7). Built from the run's raw events.
async fn inspect(AxState(k): K, Developer(req): Developer, Path(id): Path<String>) -> ApiResult {
    let lens = k.lens(&req);
    // A run its person has not let developers see is, to them, no run.
    let Some(run) = k.read(|s| s.runs.get(&id).cloned()).filter(|r| lens.sight(&r.person) != Sight::Hidden) else { return refuse(StatusCode::NOT_FOUND, "no such run") };
    // Another developer reads the run's shape, not the words of the calls
    // that are its person's alone (#257).
    let redact = req.id != run.person;
    let events = k.run_events(&id).await?;
    let tools_now = {
        use sha2::Digest;
        let t = k.tools_for(&run).await;
        let bytes = serde_json::to_vec(&t).unwrap_or_default();
        (hex::encode(&sha2::Sha256::digest(&bytes)[..16]), bytes.len())
    };
    // A chat turn runs on the chat instructions, as the kernel chose them.
    let chat_turn = k.read(|s| s.tasks.get(&run.task).and_then(|t| t.chat.as_ref()).is_some_and(|c| c.turn));
    let instructions_now = if chat_turn { crate::run::chat_instructions_version() } else { crate::run::instructions_version() };
    let playbook_chars: usize = k.read(|s| s.playbook_at(&run.person, run.config.playbook).iter().map(|i| i.text.len() + 20).sum());

    let mut seen = Sources::default();
    seen.add("instructions", crate::run::INSTRUCTIONS.len());
    let mut steps: BTreeMap<u32, Map<String, Value>> = BTreeMap::new();
    let mut actions: Vec<Value> = vec![];
    let mut action_ix: HashMap<String, usize> = HashMap::new();
    let call_tool = call_tools(&events);
    let mut harness: Vec<Value> = vec![];
    let mut records: Vec<Value> = vec![];
    for e in &events {
        let who = issuer_of(&e.actor);
        match &e.ev {
            Ev::TaskCreated { text, attachments, .. } => {
                seen.add("task", text.len());
                for a in attachments {
                    seen.add("attachments", a["data_b64"].as_str().map(str::len).unwrap_or(0) + 120);
                }
                if playbook_chars > 0 {
                    seen.add("playbook", playbook_chars);
                }
            }
            Ev::ModelCall { step, attempt, context, .. } => {
                let mut sources: Vec<Value> = seen.0.iter().map(|(k, v)| json!({ "source": k, "chars": v })).collect();
                sources.insert(0, json!({ "source": "tool definitions", "chars": tools_now.1 }));
                let st = steps.entry(*step).or_default();
                st.insert("step".into(), json!(step));
                st.insert("context".into(), json!({
                    "through_seq": context.through_seq,
                    "instructions": context.instructions,
                    // The run's own instructions, whatever of its person the call carried besides.
                    "instructions_match_build": run.config.instructions == instructions_now,
                    "tools_hash": context.tools_hash,
                    "tools_match_now": context.tools_hash == tools_now.0,
                    "sources": sources,
                }));
                let attempts = st.entry("attempts").or_insert(json!([]));
                if let Some(a) = attempts.as_array_mut() {
                    a.push(json!({ "attempt": attempt, "seq": e.seq, "at": e.at }));
                }
            }
            Ev::ModelError { step, attempt, class, message, .. } => {
                let st = steps.entry(*step).or_default();
                let attempts = st.entry("attempts").or_insert(json!([]));
                if let Some(a) = attempts.as_array_mut() {
                    a.push(json!({ "attempt": attempt, "seq": e.seq, "error": { "class": class, "message": message } }));
                }
                harness.push(json!({ "seq": e.seq, "step": step, "kind": "model_error", "issuer": "code", "text": format!("{class:?}: {message}") }));
            }
            Ev::ModelReply { step, blocks, usage, served_model, stop, .. } => {
                let mut text = String::new();
                let mut calls = vec![];
                let mut chars = 0usize;
                for b in blocks {
                    match b {
                        magnus_adapter_proto::Block::Text { text: t } => {
                            text.push_str(t);
                            chars += t.len();
                        }
                        magnus_adapter_proto::Block::ToolCall { id, name, input } => {
                            chars += name.len() + input.to_string().len();
                            let input = if redact && crate::tools::person_only_tool(name) { json!(crate::tools::REDACTED_CALL) } else { input.clone() };
                            calls.push(json!({ "call_id": id, "tool": name, "input": input }));
                        }
                        _ => {}
                    }
                }
                let st = steps.entry(*step).or_default();
                st.insert("reply".into(), json!({ "seq": e.seq, "text": text, "calls": calls, "served_model": served_model, "stop": stop, "usage": usage }));
                seen.add("earlier replies", chars);
            }
            Ev::Notice { step, text, .. } => {
                seen.add("kernel notices", text.len());
                harness.push(json!({ "seq": e.seq, "step": step, "kind": "notice", "issuer": "code", "text": text }));
            }
            Ev::Intent { call_id, step_id, tool, input, issuer, .. } => {
                // The boundary decides before it records the intent, so the
                // action may already be listed.
                let ix = *action_ix.entry(call_id.clone()).or_insert_with(|| {
                    actions.push(json!({ "call_id": call_id, "tool": tool, "decisions": [], "result": null }));
                    actions.len() - 1
                });
                let a = &mut actions[ix];
                a["step_id"] = json!(step_id);
                a["input"] = if redact && crate::tools::person_only_tool(tool) { json!(crate::tools::REDACTED_CALL) } else { input.clone() };
                a["issuer"] = json!(if issuer == "check" { "code (a check the model wrote)" } else { issuer.as_str() });
                a["intent_seq"] = json!(e.seq);
            }
            Ev::Decision { call_id, tool, outcome, stage, reason, rule, .. } => {
                let ix = *action_ix.entry(call_id.clone()).or_insert_with(|| {
                    actions.push(json!({ "call_id": call_id, "tool": tool, "issuer": if call_id.starts_with("check:") { "code (a check the model wrote)" } else if call_id.starts_with("verify:") { "the verifier" } else if call_id.starts_with("preview:") { "the gate (destination preview)" } else if call_id.starts_with("sent:") { "the gate (sent search)" } else { "model" }, "decisions": [], "result": null }));
                    actions.len() - 1
                });
                if let Some(a) = actions[ix]["decisions"].as_array_mut() {
                    a.push(json!({ "seq": e.seq, "stage": stage, "outcome": outcome, "reason": reason, "rule": rule }));
                }
            }
            Ev::ToolResult { call_id, status, text, is_error, label, .. } => {
                let tool = call_tool.get(call_id).cloned().unwrap_or_else(|| "run records".into());
                seen.add(&format!("tool results: {tool}"), text.len().min(crate::run::MAX_RESULT_IN_CONTEXT));
                let shown = if redact && crate::tools::person_only_tool(&tool) { crate::tools::REDACTED_CALL } else { text.as_str() };
                let res = json!({ "seq": e.seq, "status": status, "is_error": is_error, "label": label, "text": clip(shown, 4000), "recorded_by": who });
                match action_ix.get(call_id) {
                    Some(ix) => actions[*ix]["result"] = res,
                    None => records.push(json!({ "seq": e.seq, "kind": "record_result", "call_id": call_id, "tool": tool, "issuer": who, "result": res })),
                }
            }
            Ev::Contract { version, contract, approved_by, .. } => records.push(json!({ "seq": e.seq, "kind": "contract", "version": version, "issuer": who, "body": contract, "approved_by": approved_by })),
            Ev::WorkStarted { .. } => harness.push(json!({ "seq": e.seq, "kind": "work_started", "issuer": who })),
            Ev::ContractFrozen { version, .. } => harness.push(json!({ "seq": e.seq, "kind": "contract_frozen", "issuer": who, "text": format!("version {version}") })),
            Ev::Plan { version, plan, .. } => records.push(json!({ "seq": e.seq, "kind": "plan", "version": version, "issuer": who, "body": plan })),
            Ev::Verification { criterion, passed, evidence, by, .. } => {
                harness.push(json!({ "seq": e.seq, "kind": "verification", "issuer": issuer_of(by), "criterion": criterion, "passed": passed, "evidence": evidence }))
            }
            Ev::Suspended { reason, item, .. } => harness.push(json!({ "seq": e.seq, "kind": "suspended", "issuer": who, "text": reason, "item": item })),
            Ev::Resumed { .. } => harness.push(json!({ "seq": e.seq, "kind": "resumed", "issuer": who })),
            Ev::Tools(crate::tool_gate::Change::Screened { tool, verdict, finding, .. }) => harness.push(json!({ "seq": e.seq, "kind": "tool_screen", "issuer": who, "text": format!("{tool}: {verdict} {finding}") })),
            Ev::Completed { summary, .. } => harness.push(json!({ "seq": e.seq, "kind": "completed", "issuer": who, "text": summary })),
            Ev::Failed { reason, by, .. } => harness.push(json!({ "seq": e.seq, "kind": "failed", "issuer": issuer_of(by), "text": reason })),
            Ev::Cancelled { .. } => harness.push(json!({ "seq": e.seq, "kind": "cancelled", "issuer": who })),
            Ev::Wait { until, .. } => harness.push(json!({ "seq": e.seq, "kind": "wait", "issuer": who, "text": until })),
            Ev::BudgetExtended { budget, .. } => harness.push(json!({ "seq": e.seq, "kind": "budget_extended", "issuer": who, "text": budget })),
            Ev::ApprovalUsed { call_id, .. } => harness.push(json!({ "seq": e.seq, "kind": "approval_used", "issuer": who, "text": call_id })),
            Ev::Item { item_kind, summary, id, details, .. } => harness.push(json!({ "seq": e.seq, "kind": format!("inbox_{item_kind}"), "issuer": who, "text": if redact && details.get("disagreement").is_some_and(|d| d.is_string()) { json!(crate::tools::REDACTED_CALL) } else { json!(summary) }, "item": id, "reasoning": (item_kind == "approval").then(|| crate::asks::reasoning(details)) })),
            Ev::Output { name, media_type, .. } => harness.push(json!({ "seq": e.seq, "kind": "output", "issuer": who, "text": format!("{name} ({media_type})") })),
            Ev::Memory { fact, label, .. } => harness.push(json!({ "seq": e.seq, "kind": "memory_write", "issuer": who, "text": fact, "label": label })),
            Ev::Playbook { op, item, text, .. } => harness.push(json!({ "seq": e.seq, "kind": "playbook", "issuer": who, "text": format!("{op} {item}: {}", text.clone().unwrap_or_default()) })),
            Ev::Graded { grader, passed, score, .. } => harness.push(json!({ "seq": e.seq, "kind": "graded", "issuer": who, "text": format!("{grader}: {}", if *passed { "pass" } else { "fail" }), "score": score })),
            _ => {}
        }
    }
    // Spread each step's reported input tokens over its sources by size.
    let mut out = vec![];
    for (_, mut st) in steps {
        let usage = st.get("reply").map(|r| r["usage"].clone()).unwrap_or(Value::Null);
        let reported = usage["input_tokens"].as_u64().unwrap_or(0) + usage["cache_read_tokens"].as_u64().unwrap_or(0) + usage["cache_write_tokens"].as_u64().unwrap_or(0);
        if let Some(ctx) = st.get_mut("context") {
            let total: usize = ctx["sources"].as_array().map(|a| a.iter().map(|s| s["chars"].as_u64().unwrap_or(0) as usize).sum()).unwrap_or(0);
            ctx["total_chars"] = json!(total);
            ctx["reported_input_tokens"] = json!(reported);
            ctx["token_basis"] = json!(if reported > 0 { "provider-reported total, shared by size" } else { "estimated at 4 characters a token" });
            if let Some(a) = ctx["sources"].as_array_mut() {
                for s in a.iter_mut() {
                    let c = s["chars"].as_u64().unwrap_or(0) as f64;
                    let t = if reported > 0 && total > 0 { reported as f64 * c / total as f64 } else { c / 4.0 };
                    s["tokens"] = json!(t.round() as u64);
                }
            }
        }
        out.push(Value::Object(st));
    }
    let w = k.all_words().await?;
    let s = k.read(|s| summary(s, &run, k.cfg.observability.stall_minutes, &w));
    let v = json!({ "run": s, "steps": out, "actions": actions, "records": records, "harness": harness, "events": events.len() });
    Ok(Json(lens.show(lens.sight(&run.person), v)).into_response())
}

#[derive(serde::Deserialize, Default)]
struct LogQuery {
    after: Option<i64>,
    limit: Option<usize>,
}

/// The full log of one run (V4), with large payloads shortened for reading.
async fn raw_log(AxState(k): K, Developer(req): Developer, Path(id): Path<String>, Query(q): Query<LogQuery>) -> ApiResult {
    let lens = k.lens(&req);
    let Some(person) = k.read(|s| s.runs.get(&id).map(|r| r.person.clone())).filter(|p| lens.sight(p) != Sight::Hidden) else {
        return refuse(StatusCode::NOT_FOUND, "no such run");
    };
    Ok(match k.run_events(&id).await {
        Ok(evs) => {
            let sight = lens.sight(&person);
            let redact = req.id != person;
            let call_tool = call_tools(&evs);
            let after = q.after.unwrap_or(0);
            let limit = q.limit.unwrap_or(1000);
            let rows: Vec<Value> = evs
                .iter()
                .filter(|e| e.seq > after)
                .take(limit)
                .map(|e| {
                    let mut body = serde_json::to_value(&e.ev).unwrap_or_default();
                    if redact {
                        redact_event_body(&mut body, &call_tool);
                    }
                    // Anonymised, the event is walked field by field: its
                    // kind, counts and codes stay, its words go.
                    let row = |body: Value| json!({ "seq": e.seq, "at": e.at, "actor": e.actor, "kind": body["kind"], "event": body });
                    let mut shown = lens.show(sight, row(shrink(&body))).unwrap_or_default();
                    shown["body"] = shown["event"].take();
                    shown.as_object_mut().map(|m| m.remove("event"));
                    shown
                })
                .collect();
            Json(json!({ "events": rows, "total": evs.len() })).into_response()
        }
        Err(e) => internal(e),
    })
}

/// A grader's verdict from the evaluation runner (V2).
async fn grade(AxState(k): K, Developer(p): Developer, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    let lens = k.lens(&p);
    if k.read(|s| s.runs.get(&id).is_none_or(|r| lens.sight(&r.person) == Sight::Hidden)) {
        return refuse(StatusCode::NOT_FOUND, "no such run");
    }
    let (Some(grader), Some(passed)) = (b["grader"].as_str(), b["passed"].as_bool()) else {
        return refuse(StatusCode::BAD_REQUEST, "grader and passed are required");
    };
    let ev = Ev::Graded { run: id, grader: grader.into(), passed, score: b["score"].as_f64(), details: b.get("details").cloned().unwrap_or(Value::Null) };
    Ok(match k.emit(&format!("person:{}", p.id), ev).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    })
}
