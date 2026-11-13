//! The developer view (V1-V7): what each run saw and did, experiments,
//! live panels, policy history and the playbook. Raw log access lives here
//! and only here, behind the developer role (K10, K45).

use crate::anonymize::{Lens, Sight};
use crate::api::{developer, err, internal, status_str};
use crate::events::{Ev, ExperimentTag};
use crate::kernel::Kernel;
use crate::state::{Run, State, Status};
use axum::{
    extract::{Path, Query, State as AxState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

type K = AxState<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/dev/runs", get(runs))
        .route("/v1/dev/runs/{id}", get(inspect))
        .route("/v1/dev/runs/{id}/log", get(raw_log))
        .route("/v1/dev/runs/{id}/grades", post(grade))
        .route("/v1/dev/metrics", get(crate::dev_panels::metrics))
        .route("/v1/dev/experiments", get(experiments))
        .route("/v1/dev/experiments/{id}", get(experiment))
        .route("/v1/dev/experiments/{id}/trials", post(add_trial))
        .route("/v1/dev/policy", get(crate::dev_panels::policy))
        .route("/v1/dev/playbook", get(crate::dev_panels::playbook))
        .route("/v1/dev/modules", get(modules))
}

macro_rules! dev {
    ($k:expr, $h:expr) => {
        match developer(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
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
fn verdict(r: &Run) -> (bool, &'static str) {
    if !r.grades.is_empty() {
        let latest: HashMap<&str, bool> = r.grades.iter().map(|g| (g.0.as_str(), g.1)).collect();
        (latest.values().all(|p| *p), "grader")
    } else {
        (r.status == Status::Completed, "kernel")
    }
}

fn config_key(r: &Run) -> String {
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
async fn modules(AxState(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let (a, m) = k.default_model();
    let first = format!("{a}/{m}");
    let mut models = vec![first.clone()];
    models.extend(k.cfg.modules.model.iter().map(|x| x.model.clone()).filter(|x| *x != first));
    let acted = k.read(|s| {
        let mut n: HashMap<&str, usize> = HashMap::new();
        for r in s.runs.values().filter(|r| lens.run_sight(&r.id, &r.person) != Sight::Hidden) {
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
    Json(json!({ "modules": list, "lost_thresholds": k.cfg.modules.lost })).into_response()
}

/// Each removable module on or off for a run, as pinned at its start.
fn modules_of(r: &Run) -> Value {
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

async fn runs(AxState(k): K, h: HeaderMap, Query(q): Query<RunQuery>) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let stall = k.cfg.observability.stall_minutes;
    let ids: Option<BTreeSet<String>> = q.ids.as_ref().map(|i| i.split(',').map(String::from).collect());
    let w = match k.all_words().await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let mut list: Vec<Value> = k.read(|s| {
        let mut rs: Vec<&Run> = s.runs.values().collect();
        rs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        rs.into_iter()
            .filter(|r| q.status.as_deref().is_none_or(|st| status_str(r.status) == st))
            .filter(|r| q.experiment.as_deref().is_none_or(|e| r.experiment.as_ref().is_some_and(|x| x.id == e)))
            .filter(|r| q.failure.as_deref().is_none_or(|f| failure(s, r, stall) == Some(f)))
            .filter(|r| ids.as_ref().is_none_or(|i| i.contains(&r.id)))
            .filter_map(|r| lens.show(lens.run_sight(&r.id, &r.person), summary(s, r, stall, &w)))
            .collect()
    });
    list.truncate(q.limit.unwrap_or(500));
    Json(json!({ "runs": list })).into_response()
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
        "item" => {
            if body["details"]["disagreement"].is_string() {
                body["summary"] = json!(crate::tools::REDACTED_CALL);
                body["details"]["outcome"] = json!(crate::tools::REDACTED_CALL);
            }
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
async fn inspect(AxState(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let req = dev!(k, h);
    let lens = k.lens(&req);
    // A run its person has not let developers see is, to them, no run.
    let Some(run) = k.read(|s| s.runs.get(&id).cloned()).filter(|r| lens.run_sight(&r.id, &r.person) != Sight::Hidden) else { return err(StatusCode::NOT_FOUND, "no such run") };
    // Another developer reads the run's shape, not the words of the calls
    // that are its person's alone (#257).
    let redact = req.id != run.person;
    let events = match k.run_events(&id).await {
        Ok(e) => e,
        Err(e) => return internal(e),
    };
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
                attempts.as_array_mut().unwrap().push(json!({ "attempt": attempt, "seq": e.seq, "at": e.at }));
            }
            Ev::ModelError { step, attempt, class, message, .. } => {
                let st = steps.entry(*step).or_default();
                let attempts = st.entry("attempts").or_insert(json!([]));
                attempts.as_array_mut().unwrap().push(json!({ "attempt": attempt, "seq": e.seq, "error": { "class": class, "message": message } }));
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
                actions[ix]["decisions"].as_array_mut().unwrap().push(json!({ "seq": e.seq, "stage": stage, "outcome": outcome, "reason": reason, "rule": rule }));
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
    let w = match k.all_words().await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let s = k.read(|s| summary(s, &run, k.cfg.observability.stall_minutes, &w));
    let v = json!({ "run": s, "steps": out, "actions": actions, "records": records, "harness": harness, "events": events.len() });
    Json(lens.show(lens.run_sight(&run.id, &run.person), v)).into_response()
}

#[derive(serde::Deserialize, Default)]
struct LogQuery {
    after: Option<i64>,
    limit: Option<usize>,
}

/// The full log of one run (V4), with large payloads shortened for reading.
async fn raw_log(AxState(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<LogQuery>) -> Response {
    let req = dev!(k, h);
    let lens = k.lens(&req);
    let Some(person) = k.read(|s| s.runs.get(&id).map(|r| r.person.clone())).filter(|p| lens.run_sight(&id, p) != Sight::Hidden) else {
        return err(StatusCode::NOT_FOUND, "no such run");
    };
    match k.run_events(&id).await {
        Ok(evs) => {
            let sight = lens.run_sight(&id, &person);
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
    }
}

/// A grader's verdict from the evaluation runner (V2).
async fn grade(AxState(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    if k.read(|s| s.runs.get(&id).is_none_or(|r| lens.run_sight(&r.id, &r.person) == Sight::Hidden)) {
        return err(StatusCode::NOT_FOUND, "no such run");
    }
    let (Some(grader), Some(passed)) = (b["grader"].as_str(), b["passed"].as_bool()) else {
        return err(StatusCode::BAD_REQUEST, "grader and passed are required");
    };
    let ev = Ev::Graded { run: id, grader: grader.into(), passed, score: b["score"].as_f64(), details: b.get("details").cloned().unwrap_or(Value::Null) };
    match k.emit(&format!("person:{}", p.id), ev).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    }
}

/// Start one trial of an experiment case, acting for the developer: a task,
/// or with `"via": "chat"` a chat message (in a new thread, or in `thread`),
/// on the appliance's model or the one in `model`. `modules` switches
/// removable modules on or off for a task trial (their ablation).
async fn add_trial(AxState(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = dev!(k, h);
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    let (Some(case), Some(trial)) = (b["case"].as_str(), b["trial"].as_u64()) else { return err(StatusCode::BAD_REQUEST, "case and trial are required") };
    if text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "the task has no text");
    }
    let model = match crate::api::chosen_model(&k, &b).await {
        Ok(m) => m,
        Err(r) => return r,
    };
    let mut modules = BTreeMap::new();
    for (name, on) in b["modules"].as_object().into_iter().flatten() {
        let Some(on) = on.as_bool().filter(|_| MODULES.iter().any(|m| m.name == name)) else {
            return err(StatusCode::BAD_REQUEST, &format!("no removable module {name}, or not true/false"));
        };
        modules.insert(name.clone(), on);
    }
    if !modules.is_empty() && b["via"] == "chat" {
        return err(StatusCode::BAD_REQUEST, "module switches apply to task trials");
    }
    // The same trial may run once on each model and module setting, to compare them.
    let tag = ExperimentTag { id, case: case.into(), trial: trial as u32 };
    let on = model.clone().unwrap_or_else(|| k.default_model());
    let (mut want, _) = k.modules_for(&on.0, &on.1);
    want.extend(modules.clone());
    let dup = k.read(|s| {
        s.runs.values().any(|r| {
            r.experiment.as_ref() == Some(&tag)
                && (r.config.adapter.as_str(), r.config.model.as_str()) == (on.0.as_str(), on.1.as_str())
                && MODULES.iter().all(|m| r.config.module(m.name) == want[m.name])
        })
    });
    if dup {
        return err(StatusCode::CONFLICT, "this trial already exists");
    }
    if b["via"] == "chat" {
        return match k.chat_say(&p.id, text, b["thread"].as_str().map(String::from), model, Some(tag), vec![], None).await {
            Ok(Some((thread, task))) => (StatusCode::CREATED, Json(json!({ "thread": thread, "task_id": task }))).into_response(),
            Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
            Err(e) => internal(e),
        };
    }
    let attachments = b["attachments"].as_array().cloned().unwrap_or_default();
    match k.create_tagged(&format!("person:{}", p.id), &p.id, text, attachments, Some(tag), model, modules).await {
        Ok((task, run)) => (StatusCode::CREATED, Json(json!({ "task_id": task, "run_id": run }))).into_response(),
        Err(e) => internal(e),
    }
}

fn choose(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (k - i) as f64)
}

/// pass@k: the chance that at least one of k trials passes (unbiased, from n
/// trials with c passes). pass^k: the chance that all k pass.
fn pass_at(n: u64, c: u64, k: u64) -> (f64, f64) {
    if n == 0 || k == 0 || k > n {
        return (f64::NAN, f64::NAN);
    }
    let at = 1.0 - choose(n - c, k) / choose(n, k);
    let hat = choose(c, k) / choose(n, k);
    (at, hat)
}

fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (m, f64::NAN);
    }
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

pub(crate) fn num(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 10000.0).round() / 10000.0)
    } else {
        Value::Null
    }
}

async fn experiments(AxState(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let list: Vec<Value> = k.read(|s| {
        let mut by: BTreeMap<String, Vec<&Run>> = BTreeMap::new();
        for r in s.runs.values().filter(|r| lens.run_sight(&r.id, &r.person) != Sight::Hidden) {
            if let Some(t) = &r.experiment {
                by.entry(t.id.clone()).or_default().push(r);
            }
        }
        by.into_iter()
            .map(|(id, rs)| {
                let cases: BTreeSet<&str> = rs.iter().map(|r| r.experiment.as_ref().unwrap().case.as_str()).collect();
                let configs: BTreeSet<String> = rs.iter().map(|r| config_key(r)).collect();
                let first = rs.iter().map(|r| r.created_at).min();
                let settled = rs.iter().filter(|r| r.status.terminal()).count();
                json!({ "id": id, "trials": rs.len(), "settled": settled, "cases": cases.len(), "configs": configs, "started": first })
            })
            .collect()
    });
    Json(json!({ "experiments": list })).into_response()
}

#[derive(serde::Deserialize, Default)]
struct ExpQuery {
    k: Option<u64>,
    /// The earlier run of the same requests to compare with.
    base: Option<String>,
}

/// One experiment: each configuration across its repeated trials, case by
/// case, with pass@k and pass^k and their spread, a paired comparison, and
/// failures tagged by where they went wrong (V2). Every number lists its runs.
async fn experiment(AxState(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<ExpQuery>) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let stall = k.cfg.observability.stall_minutes;
    let w = match k.all_words().await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let v = k.read(|s| {
        let rs: Vec<&Run> = s.runs.values().filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == id) && lens.run_sight(&r.id, &r.person) != Sight::Hidden).collect();
        if rs.is_empty() {
            return None;
        }
        // config -> case -> runs
        let mut grid: BTreeMap<String, BTreeMap<String, Vec<&Run>>> = BTreeMap::new();
        for r in &rs {
            grid.entry(config_key(r)).or_default().entry(r.experiment.as_ref().unwrap().case.clone()).or_default().push(r);
        }
        let cases: BTreeSet<String> = rs.iter().map(|r| r.experiment.as_ref().unwrap().case.clone()).collect();
        let settled = |r: &&&Run| r.status.terminal() || r.status == Status::Suspended;
        let kk = q.k.unwrap_or_else(|| {
            grid.values().flat_map(|c| c.values()).map(|v| v.iter().filter(settled).count() as u64).filter(|n| *n > 0).min().unwrap_or(1)
        });
        let mut configs = vec![];
        let mut rates: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        for (key, by_case) in &grid {
            let mut rows = vec![];
            let (mut at, mut hat, mut p1) = (vec![], vec![], vec![]);
            let mut tags: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            let mut basis = BTreeSet::new();
            for (case, trials) in by_case {
                let done: Vec<&&Run> = trials.iter().filter(settled).collect();
                let n = done.len() as u64;
                let passed: Vec<String> = done.iter().filter(|r| verdict(r).0).map(|r| r.id.clone()).collect();
                let c = passed.len() as u64;
                for r in &done {
                    basis.insert(verdict(r).1);
                    if !verdict(r).0 {
                        tags.entry(failure(s, r, stall).unwrap_or("grader")).or_default().push(r.id.clone());
                    }
                }
                let (a, hh) = pass_at(n, c, kk);
                if n > 0 {
                    at.push(a);
                    hat.push(hh);
                    p1.push(c as f64 / n as f64);
                    rates.entry(key.clone()).or_default().insert(case.clone(), c as f64 / n as f64);
                }
                rows.push(json!({
                    "case": case, "trials": trials.len(), "settled": n, "passed": c,
                    "pass_at_k": num(a), "pass_hat_k": num(hh),
                    "runs": trials.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), "passed_runs": passed,
                }));
            }
            let (m1, se1) = mean_se(&p1);
            let (ma, sea) = mean_se(&at);
            let (mh, seh) = mean_se(&hat);
            let sample = by_case.values().next().and_then(|v| v.first()).unwrap();
            configs.push(json!({
                "key": key,
                "config": { "adapter": sample.config.adapter, "model": sample.config.model, "harness": sample.config.harness, "instructions": sample.config.instructions, "playbook": sample.config.playbook, "modules": modules_of(sample) },
                "pass_rate": { "mean": num(m1), "se": num(se1) },
                "pass_at_k": { "mean": num(ma), "se": num(sea) },
                "pass_hat_k": { "mean": num(mh), "se": num(seh) },
                "verdict_basis": basis,
                "cases": rows,
                "failures": tags.into_iter().map(|(t, r)| json!({ "where": t, "count": r.len(), "runs": r })).collect::<Vec<_>>(),
            }));
        }
        // Paired, case by case, every configuration against the first.
        let keys: Vec<&String> = rates.keys().collect();
        let mut paired = vec![];
        if let Some(base) = keys.first() {
            for other in keys.iter().skip(1) {
                let (a, b) = (&rates[*base], &rates[*other]);
                let common: Vec<&String> = a.keys().filter(|c| b.contains_key(*c)).collect();
                let diffs: Vec<f64> = common.iter().map(|c| b[*c] - a[*c]).collect();
                let (m, se) = mean_se(&diffs);
                paired.push(json!({
                    "base": base, "other": other, "cases": common.len(),
                    "mean_difference": num(m), "se": num(se),
                    "better": diffs.iter().filter(|d| **d > 0.0).count(),
                    "worse": diffs.iter().filter(|d| **d < 0.0).count(),
                    "same": diffs.iter().filter(|d| **d == 0.0).count(),
                    "by_case": common.iter().zip(&diffs).map(|(c, d)| json!({ "case": c, "difference": num(*d) })).collect::<Vec<_>>(),
                }));
            }
        }
        let requests = request_rows(s, &id, &w, &lens);
        let compared = if requests.is_empty() { Value::Null } else { compare(s, &id, &requests, q.base.as_deref(), &w, &lens) };
        Some(json!({ "id": id, "k": kk, "cases": cases, "configs": configs, "paired": paired, "requests": requests, "compare": compared }))
    });
    match v {
        Some(v) => Json(v).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such experiment"),
    }
}

/// A chat trial and what it led to: what the model chose (answer, a
/// question back, tasks, a schedule), the tasks its message started, every
/// policy stop (ask or deny) on the way, and the cost and time of it all,
/// until each run first ended or stopped (for the person, or to wait), and
/// the person's verdict on the outcome.
fn request_trial(s: &State, r: &Run, w: &crate::recall::Words) -> Value {
    let kids: Vec<&Run> =
        s.tasks.values().filter(|t| t.chat.as_ref().is_some_and(|c| c.from_run.as_deref() == Some(r.id.as_str()))).filter_map(|t| s.runs.get(&t.run_id)).collect();
    let card = r.results.values().any(|res| res.data.as_ref().is_some_and(|d| d["card"].is_string()));
    let answer = r.summary.as_deref().unwrap_or_default().trim();
    let choice = if card {
        "schedule"
    } else if !kids.is_empty() {
        "task"
    } else if r.status != Status::Completed {
        status_str(r.status)
    } else if answer.ends_with('?') {
        "asked"
    } else {
        "answer"
    };
    let all: Vec<&Run> = std::iter::once(r).chain(kids.iter().copied()).collect();
    let stops: Vec<Value> = all
        .iter()
        .flat_map(|x| {
            x.decisions.iter().filter(|d| d.outcome != "allow").map(move |d| json!({ "run": x.id, "tool": d.tool, "outcome": d.outcome, "stage": d.stage, "rule": d.rule }))
        })
        .collect();
    let settled = all.iter().all(|x| x.status.terminal() || x.status == Status::Suspended);
    let end = all.iter().map(|x| x.first_stop.unwrap_or(x.last_at)).max().unwrap_or(r.created_at);
    let tag = r.experiment.as_ref().unwrap();
    let (verdict, note, verdict_from) = request_verdict(s, r, &kids);
    json!({
        "case": tag.case, "trial": tag.trial, "config": config_key(r), "run": r.id, "task": r.task,
        "thread": s.tasks.get(&r.task).and_then(|t| t.chat.as_ref()).map(|c| c.thread.clone()),
        "text": s.tasks.get(&r.task).map(|t| w.text(t.seq).to_string()),
        "choice": choice, "answer": (!answer.is_empty()).then_some(answer),
        "tasks": kids.iter().map(|k| json!({
            "task": k.task, "title": s.tasks.get(&k.task).map(|t| crate::api::title_of(t, Some(k), w)),
            "status": status_str(k.status), "reason": k.reason, "stopped_for": k.suspensions,
        })).collect::<Vec<_>>(),
        "stops": stops,
        "usd": num(all.iter().map(|x| x.usd).sum()),
        "model_calls": all.iter().map(|x| x.model_calls).sum::<u32>(),
        "seconds": (end - r.created_at).num_milliseconds() as f64 / 1000.0,
        "settled": settled,
        "verdict": verdict, "note": note, "verdict_from": verdict_from,
        "model": format!("{}/{}", r.config.adapter, r.config.model),
        "setup": { "harness": r.config.harness, "instructions": r.config.instructions, "playbook": r.config.playbook },
    })
}

/// A request's verdict: the person's on the chat answer; where the message
/// started tasks and the answer has none, theirs once every task has one
/// (right only if all are right, wrong if any is wrong, else partly).
fn request_verdict(s: &State, r: &Run, kids: &[&Run]) -> (Option<String>, Option<String>, &'static str) {
    if let Some(v) = s.tasks.get(&r.task).and_then(|t| t.verdict.as_ref()) {
        return (Some(v.verdict.clone()), v.note.clone(), "answer");
    }
    let vs: Vec<&crate::state::Verdict> = kids.iter().filter_map(|k| s.tasks.get(&k.task).and_then(|t| t.verdict.as_ref())).collect();
    if kids.is_empty() || vs.len() < kids.len() {
        return (None, None, "none");
    }
    let worst = vs.iter().map(|v| rank(&v.verdict)).max().unwrap_or(0);
    let combined = if vs.iter().all(|v| v.verdict == "right") {
        "right"
    } else if worst == rank("wrong") {
        "wrong"
    } else {
        "partly"
    };
    let notes: Vec<String> = vs.iter().filter_map(|v| v.note.clone()).collect();
    (Some(combined.into()), (!notes.is_empty()).then(|| notes.join(" · ")), "tasks")
}

/// Right first: a lower rank is a better verdict.
fn rank(v: &str) -> usize {
    crate::state::VERDICTS.iter().position(|x| *x == v).unwrap_or(crate::state::VERDICTS.len())
}

/// The request trials of one experiment, in case, model and trial order.
fn request_rows(s: &State, id: &str, w: &crate::recall::Words, lens: &Lens) -> Vec<Value> {
    let mut rows: Vec<Value> = s
        .runs
        .values()
        .filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == id) && s.tasks.get(&r.task).is_some_and(|t| t.is_chat_turn()))
        .filter_map(|r| lens.show(lens.run_sight(&r.id, &r.person), request_trial(s, r, w)))
        .collect();
    rows.sort_by_key(|v| (v["case"].as_str().unwrap_or_default().to_string(), v["config"].as_str().unwrap_or_default().to_string(), v["trial"].as_u64()));
    rows
}

/// This run of a request set against an earlier one (`base`, or else the
/// latest earlier experiment sharing a request): each request on each model,
/// side by side, what changed between them, and once every trial here has
/// settled, how many improved, stayed the same or regressed by verdict.
fn compare(s: &State, id: &str, rows: &[Value], base: Option<&str>, w: &crate::recall::Words, lens: &Lens) -> Value {
    let started = |e: &str| s.runs.values().filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == e)).map(|r| r.created_at).min();
    let cases: BTreeSet<&str> = rows.iter().filter_map(|r| r["case"].as_str()).collect();
    let base_id = base.map(String::from).or_else(|| {
        let mine = started(id)?;
        let mut others: BTreeMap<String, chrono::DateTime<chrono::Utc>> = BTreeMap::new();
        for r in s.runs.values() {
            let Some(t) = r.experiment.as_ref() else { continue };
            if t.id != id
                && lens.run_sight(&r.id, &r.person) != Sight::Hidden
                && cases.contains(t.case.as_str())
                && s.tasks.get(&r.task).is_some_and(|t| t.is_chat_turn())
                && r.created_at < mine
            {
                let e = others.entry(t.id.clone()).or_insert(r.created_at);
                *e = (*e).min(r.created_at);
            }
        }
        others.into_iter().max_by_key(|(_, at)| *at).map(|(e, _)| e)
    });
    let Some(base_id) = base_id else { return Value::Null };
    let before = request_rows(s, &base_id, w, lens);
    let key = |r: &Value| (r["case"].as_str().unwrap_or_default().to_string(), r["model"].as_str().unwrap_or_default().to_string(), r["trial"].as_u64());
    let earlier: BTreeMap<_, &Value> = before.iter().map(|r| (key(r), r)).collect();
    let (mut improved, mut unchanged, mut regressed, mut unjudged) = (0, 0, 0, 0);
    let pairs: Vec<Value> = rows
        .iter()
        .filter_map(|now| {
            let then = *earlier.get(&key(now))?;
            let mut changed = vec![];
            for f in ["verdict", "choice"] {
                if then[f] != now[f] {
                    changed.push(f);
                }
            }
            let stops = |r: &Value| -> Vec<String> {
                let mut v: Vec<String> = r["stops"].as_array().into_iter().flatten().map(|x| format!("{} {} {}", x["tool"], x["outcome"], x["rule"])).collect();
                v.sort();
                v
            };
            if stops(then) != stops(now) {
                changed.push("stops");
            }
            let tasks = |r: &Value| -> Vec<String> {
                let mut v: Vec<String> = r["tasks"].as_array().into_iter().flatten().map(|x| x["status"].as_str().unwrap_or_default().to_string()).collect();
                v.sort();
                v
            };
            if tasks(then) != tasks(now) {
                changed.push("tasks");
            }
            if then["setup"] != now["setup"] {
                changed.push("setup");
            }
            let outcome = match (then["verdict"].as_str(), now["verdict"].as_str()) {
                (Some(a), Some(b)) if rank(b) < rank(a) => {
                    improved += 1;
                    "improved"
                }
                (Some(a), Some(b)) if rank(b) > rank(a) => {
                    regressed += 1;
                    "regressed"
                }
                (Some(_), Some(_)) => {
                    unchanged += 1;
                    "unchanged"
                }
                _ => {
                    unjudged += 1;
                    "unjudged"
                }
            };
            let delta = |f: &str| match (then[f].as_f64(), now[f].as_f64()) {
                (Some(a), Some(b)) => num(b - a),
                _ => Value::Null,
            };
            Some(json!({
                "case": now["case"], "model": now["model"], "trial": now["trial"], "text": now["text"],
                "before": then, "after": now, "changed": changed, "outcome": outcome,
                "usd_change": delta("usd"), "seconds_change": delta("seconds"),
            }))
        })
        .collect();
    let settled = rows.iter().filter(|r| r["settled"] == true).count();
    json!({
        "base": base_id, "pairs": pairs, "settled": settled, "trials": rows.len(),
        "complete": settled == rows.len() && !rows.is_empty(),
        "summary": { "improved": improved, "unchanged": unchanged, "regressed": regressed, "unjudged": unjudged },
    })
}
