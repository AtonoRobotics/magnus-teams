use super::{controlled, err, internal, not_modified, seq_etag, who, with_etag, K};
use crate::api_error::{refuse, ApiResult};
use crate::config::Person;
use crate::extract::{Body, Caller};
use crate::inbox::{self, Answer};
use crate::kernel::Kernel;
use crate::state::{Run, Status};
use crate::{api_bodies as req, events::Ev, people::rights};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

pub(crate) fn status_str(s: Status) -> &'static str {
    match s {
        Status::Running => "running",
        Status::Suspended => "suspended",
        Status::Completed => "completed",
        Status::Failed => "failed",
        Status::Cancelled => "cancelled",
    }
}

/// The short title the model gave the task (when starting it, in its plan,
/// or in a contract recorded before rev 147), else the start of what the person wrote; from its words,
/// recalled for its person.
pub(crate) fn title_of(task: &crate::state::Task, run: Option<&Run>, words: &crate::recall::Words) -> String {
    words
        .title(task.seq)
        .or_else(|| run.and_then(|r| r.plan.as_ref().or(r.contract.as_ref())).and_then(|c| c["title"].as_str()).map(str::trim).filter(|t| !t.is_empty()).map(String::from))
        .unwrap_or_else(|| words.text(task.seq).chars().take(80).collect())
}

/// Whether the task waits on an inbox item this person can answer.
fn needs(s: &crate::state::State, p: &Person, run: &Run) -> bool {
    run.status == Status::Suspended && s.inbox.items().values().any(|i| i.run.as_deref() == Some(run.id.as_str()) && i.resolution.is_none() && inbox::may_respond(s, p, i))
}

/// A task in a list: what it is, where it stands, whether it needs this
/// person, and the request it came from (a chat message or a schedule).
/// `words` holds the recalled words of the tasks and schedules it names.
pub(crate) fn task_row(k: &Kernel, s: &crate::state::State, p: &Person, t: &crate::state::Task, r: &Run, words: &crate::recall::Words) -> Value {
    let mut v = task_json(k, t, r, false, words);
    let mine = t.person == p.id;
    let sub_agents_active = s.sub_agents.below(&r.id).into_iter().filter(|(_, c)| s.runs.get(&c.child).is_some_and(|cr| !cr.status.terminal())).count();
    let receipts_count = crate::evidence::receipts_count_in_state(s, r);
    v["needs_you"] = json!(needs(s, p, r));
    v["sub_agents_active"] = json!(sub_agents_active);
    v["receipts_count"] = json!(receipts_count);
    v["verified"] = json!(receipts_count > 0);
    v["actions"] = json!(r.actions);
    v["recent_steps"] = json!(crate::evidence::recent_steps(r, crate::evidence::RECENT_STEPS_COUNT));
    let is_system = crate::outcomes::is_system_task(s, t, r) || (t.schedule.is_some() && crate::outcomes::is_heartbeat_text(words.title(t.seq).as_deref(), words.text(t.seq)));
    let waits_on_timer = r.reason.as_deref() == Some("timer");
    let needs_person = r.status == Status::Failed || (r.status == Status::Suspended && !waits_on_timer);
    v["system"] = json!(is_system);
    v["routine"] = json!(crate::outcomes::routine(s, t, r) || (is_system && !needs_person));
    // Another person's projects and conversations are theirs alone: the
    // owner sees that a task came from a chat, not what was said in it.
    v["project"] = if mine { crate::library::project_ref(s, &t.task_id) } else { Value::Null };
    v["waiting_item"] = json!(r.waiting_item);
    v["ended_at"] = json!(r.ended_at);
    // A scheduled run's sub-agents are grouped under its schedule.
    v["group"] = if let Some(sc) = s.authority(&t.run_id).and_then(|a| a.schedule.as_ref()).and_then(|id| s.arrangements.schedules().get(id)) {
        json!({ "kind": "schedule", "id": sc.id, "title": words.title(sc.seq).unwrap_or_else(|| words.text(sc.seq).to_string()) })
    } else if let Some((c, turn)) =
        t.chat.as_ref().and_then(|c| c.from_run.as_ref().and_then(|f| s.runs.get(f)).and_then(|r| s.tasks.get(&r.task)).map(|turn| (c, crate::outcomes::person_turn(s, turn))))
    {
        if mine {
            json!({ "kind": "chat", "id": turn.task_id, "title": words.text(turn.seq), "thread": c.thread })
        } else {
            json!({ "kind": "chat", "id": turn.task_id, "title": format!("A conversation of {}", t.person) })
        }
    } else {
        Value::Null
    };
    v
}

fn task_json(k: &Kernel, task: &crate::state::Task, run: &Run, full: bool, words: &crate::recall::Words) -> Value {
    let mut v = json!({
        "task_id": task.task_id,
        "run_id": run.id,
        "person": task.person,
        "text": words.text(task.seq),
        "title": title_of(task, Some(run), words),
        "created_at": task.created_at,
        "status": status_str(run.status),
        "waiting_on": run.reason,
        "status_line": crate::evidence::status_line(run),
        "summary": run.summary,
        "has_deliverable": run.outcome_view.as_ref().and_then(Value::as_array).is_some_and(|a| !a.is_empty()),
        "plan": run.plan,
        "plan_version": run.plan_version,
        "thread": task.chat.as_ref().map(|c| c.thread.clone()),
        "verdict": task.verdict,
        "review": task.review,
        "poll": task.poll,
    });
    // The apps this task asked the person to connect, each as they stand
    // with it now (connectors spec, Inside work).
    if full {
        let mut offered: Vec<(i64, Value)> =
            run.results.values().filter_map(|r| r.data.as_ref().filter(|d| d["connector"].is_string()).map(|d| (r.seq, k.offered_card(&task.person, d)))).collect();
        offered.sort_by_key(|(seq, _)| *seq);
        v["app_offers"] = json!(offered.into_iter().map(|(_, c)| c).collect::<Vec<_>>());
    }
    if full {
        // Evidence from the record (spec rev 147): receipts with their
        // read-backs, what was not done, the sources the result cites.
        v["evidence"] = k.evidence_of(run);
        // A task from before rev 147 keeps the contract it recorded, as data.
        v["contract"] = run.contract.clone().unwrap_or(Value::Null);
        v["model"] = json!({ "adapter": run.config.adapter, "name": run.config.model });
        v["usage"] = json!({ "model_calls": run.model_calls, "actions": run.actions, "cost_usd": run.usd });
        // What the run may spend: for a task from a proposal, the estimate the person approved (#254).
        v["budget"] = json!(run.config.budget);
        v["taint"] = json!({ "untrusted": run.untrusted, "private": run.private });
        v["outputs"] = json!(run.outputs.iter().map(|(id, name, mt, size)| json!({ "id": id, "name": name, "media_type": mt, "size": size })).collect::<Vec<_>>());
        v["outcome_view"] = run.outcome_view.clone().unwrap_or(Value::Null);
        // Only here, never in a list's rows: those are built under the state lock.
        v["files"] = json!(k.read(|s| task.files.iter().filter_map(|id| s.library.files().get(id)).map(|f| crate::library::file_json(s, f, words)).collect::<Vec<_>>()));
        v["project"] = k.read(|s| crate::library::project_ref(s, &task.task_id));
        v["preview"] = json!({ "chars": crate::compose::PREVIEW_CHARS, "rows": crate::compose::MAX_ROWS });
        v["spoken"] = json!(run.spoken);
        v["ended_at"] = json!(run.ended_at);
        v["waiting_item"] = json!(run.waiting_item);
        v["config"] = json!({ "harness": run.config.harness, "instructions": run.config.instructions, "playbook": run.config.playbook });
        v["open_items"] = json!(k.read(|s| s.inbox.open_for_run(&run.id).map(|i| i.id.clone()).collect::<Vec<_>>()));
        // Its sub-agents are shown here, inside it, not listed as tasks.
        v["sub_agents"] = json!(k.read(|s| crate::chat_thread::sub_agents_of(s, run, words)));
    }
    v
}

pub(super) async fn submit(State(k): K, Caller(p): Caller, Body(b): Body<Value>) -> ApiResult {
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "the task has no text");
    }
    let mut files = crate::library::given_files(&k, &p, &b)?;
    let project = crate::library::chosen_project(&k, &p, &b)?;
    let model = chosen_model(&k, &b).await?;
    let reasoning = chosen_reasoning(&b)?;
    // Files sent with the task are uploads like any other: they go to the
    // person's library, linked to this task.
    for a in b["attachments"].as_array().into_iter().flatten() {
        match crate::library::add_file(&k, &p, a, None).await {
            Ok(Ok(id)) => files.push(id),
            Ok(Err(r)) => return Ok(r),
            Err(e) => return Ok(internal(e)),
        }
    }
    let o = crate::kernel::NewRun { model, reasoning, files, project, ..Default::default() };
    Ok(match k.create_run(&format!("person:{}", p.id), &p.id, text, crate::attachment::from_request(&b), o).await {
        Ok((task, run)) => (StatusCode::CREATED, Json(json!({ "task_id": task, "run_id": run }))).into_response(),
        Err(e) => internal(e),
    })
}

/// Optional per-request reasoning effort (`off`, `low`, `medium`, `high`).
pub(crate) fn chosen_reasoning(b: &Value) -> ApiResult<Option<String>> {
    let Some(raw) = b["model"]["reasoning"].as_str().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if matches!(raw, "off" | "low" | "medium" | "high") {
        Ok(Some(raw.to_string()))
    } else {
        refuse(StatusCode::BAD_REQUEST, "reasoning must be one of: off, low, medium, high")
    }
}

/// The model a request chose (`"model": {"adapter", "model"}`), if any,
/// once its adapter confirms it offers it.
pub(crate) async fn chosen_model(k: &Kernel, b: &Value) -> ApiResult<Option<(String, String)>> {
    if b["model"].is_null() {
        return Ok(None);
    }
    let (Some(adapter), Some(model)) = (b["model"]["adapter"].as_str(), b["model"]["model"].as_str()) else {
        return refuse(StatusCode::BAD_REQUEST, "model needs adapter and model");
    };
    match k.check_model(adapter, model).await {
        Ok(()) => Ok(Some((adapter.into(), model.into()))),
        Err(m) => refuse(StatusCode::BAD_REQUEST, m),
    }
}

/// Each person's tasks are their own, the owner's too (owner, 2026-10-30:
/// "Individual users should have their own accounts with their own tasks,
/// conversations, schedules, memory").
fn visible_task(p: &Person, t: &crate::state::Task) -> bool {
    t.person == p.id
}

pub(super) async fn list_tasks(State(k): K, Caller(p): Caller, h: HeaderMap) -> ApiResult {
    let etag = seq_etag(&k);
    if let Some(r) = not_modified(&h, &etag) {
        return Ok(r);
    }
    let words = k.person_words(&p.id).await?;
    let tasks: Vec<Value> = k.read(|s| {
        s.tasks
            .values()
            // Polls fire dozens a day: they stay out of the person's
            // lists, auditable by id and in the log (#406).
            .filter(|t| visible_task(&p, t) && s.listed(t) && !t.poll)
            .filter_map(|t| s.runs.get(&t.run_id).map(|r| task_row(&k, s, &p, t, r, &words)))
            .collect()
    });
    Ok(with_etag(Json(json!({ "tasks": tasks })).into_response(), &etag))
}

/// A search's words, lowercased; a match has every one.
pub(crate) fn terms(q: &str) -> Vec<String> {
    q.to_lowercase().split_whitespace().map(String::from).collect()
}

/// One search across the person's conversations (their titles and what was
/// said in them), tasks (title, request, summary) and files (as the library
/// matches them). A conversation comes with the first thing said in it that
/// matches.
pub(super) async fn search(State(k): K, Caller(p): Caller, Query(q): Query<std::collections::HashMap<String, String>>) -> ApiResult {
    let terms = terms(q.get("q").map(String::as_str).unwrap_or_default());
    if terms.is_empty() {
        return Ok(Json(json!({ "conversations": [], "tasks": [], "files": [] })).into_response());
    }
    let all = |hay: &str| terms.iter().all(|t| hay.contains(t.as_str()));
    let mut conversations = vec![];
    let threads = k.threads(&p.id).await?;
    for t in threads {
        let id = t["thread"].as_str().unwrap_or_default();
        let said: Vec<String> = match k.thread_entries(&p.id, id).await {
            Ok(e) => e.unwrap_or_default(),
            Err(e) => return Ok(internal(e)),
        }
        .iter()
        .filter_map(|e| e.said_text().map(String::from))
        .collect();
        let title = t["title"].as_str().unwrap_or_default().to_lowercase();
        if !all(&format!("{title} {}", said.join(" ").to_lowercase())) {
            continue;
        }
        let snippet = said.iter().find(|s| terms.iter().any(|w| s.to_lowercase().contains(w.as_str()))).map(|s| s.chars().take(160).collect::<String>());
        conversations.push(json!({ "thread": id, "title": t["title"], "updated_at": t["updated_at"], "project": t["project"], "snippet": snippet }));
    }
    let texts = k.file_texts(&p.id, &terms).await?;
    let words = k.person_words(&p.id).await?;
    let (tasks, files) = k.read(|s| {
        let tasks: Vec<Value> = s
            .tasks
            .values()
            .filter(|t| visible_task(&p, t) && s.listed(t) && !t.poll)
            .filter_map(|t| s.runs.get(&t.run_id).map(|r| task_row(&k, s, &p, t, r, &words)))
            .filter(|v| {
                all(&format!("{} {} {}", v["title"].as_str().unwrap_or_default(), v["text"].as_str().unwrap_or_default(), v["summary"].as_str().unwrap_or_default()).to_lowercase())
            })
            .collect();
        (tasks, crate::library::files_matching(s, &p, &terms, &texts, None, &words))
    });
    Ok(Json(json!({ "conversations": conversations, "tasks": tasks, "files": files })).into_response())
}

/// The person writes in chat: a new thread, or one of theirs. What they say
/// has the shape a shared conversation's message has (`say::Say`).
pub(super) async fn chat_post(State(k): K, Caller(p): Caller, Body(b): Body<Value>) -> ApiResult {
    let say = crate::say::Say::of(&b)?;
    let text = say.text.trim().to_string();
    if text.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "the message has no text");
    }
    let said = say.check(&k, &p).await?;
    let project = crate::library::chosen_project(&k, &p, &b)?;
    let (model, reasoning) = (said.pick.model, said.pick.reasoning);
    Ok(match k.chat_say_with_reasoning(&p.id, text.clone(), b["thread"].as_str().map(String::from), model, reasoning, None, said.files, project).await {
        Ok(Some((thread, task))) => (StatusCode::CREATED, Json(json!({ "thread": thread, "task_id": task, "hidden": crate::typed_secrets::mask(&text).hidden }))).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
        Err(e) => internal(e),
    })
}

pub(super) async fn chat_threads(State(k): K, h: HeaderMap) -> ApiResult {
    Ok(match who(&k, &h).await {
        Ok(p) => {
            let etag = seq_etag(&k);
            if let Some(r) = not_modified(&h, &etag) {
                return Ok(r);
            }
            match k.threads(&p.id).await {
                Ok(t) => with_etag(Json(json!({ "threads": t })).into_response(), &etag),
                Err(e) => internal(e),
            }
        }
        Err(r) => *r,
    })
}

/// A thread is its starter's alone.
pub(super) async fn chat_thread(State(k): K, Caller(p): Caller, Path(thread): Path<String>) -> ApiResult {
    Ok(match k.thread_entries(&p.id, &thread).await {
        // Their reactions shown on Magnus's messages: to them alone, not in
        // the thread as anyone else may read it.
        Ok(Some(entries)) => Json(crate::conversation::personal::view(&thread, k.read(|s| crate::reactions::marked(s, &p.id, &thread, entries)))).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
        Err(e) => internal(e),
    })
}

/// Confirm or decline a schedule card; only the person it was proposed to.
pub(super) async fn chat_card(State(k): K, h: HeaderMap, Path(card): Path<String>, Body(b): Body<Value>) -> ApiResult {
    use crate::chat::CardRefusal;
    let p = who(&k, &h).await?;
    Ok(match k.decide_card(&p.id, &card, b["decision"].as_str().unwrap_or_default(), b["reason"].as_str(), &b).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(CardRefusal::NotFound)) => err(StatusCode::NOT_FOUND, "no such card"),
        Ok(Err(CardRefusal::Decided)) => err(StatusCode::CONFLICT, "this card has been decided"),
        Ok(Err(CardRefusal::Invalid(m))) => err(StatusCode::BAD_REQUEST, &m),
        Err(e) => internal(e),
    })
}

fn find(k: &Kernel, p: &Person, id: &str) -> Option<(crate::state::Task, Run)> {
    k.read(|s| {
        let t = s.tasks.get(id)?;
        if !visible_task(p, t) {
            return None;
        }
        Some((t.clone(), s.runs.get(&t.run_id)?.clone()))
    })
}

pub(super) async fn task(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(match find(&k, &p, &id) {
        Some((t, r)) => {
            let words = k.person_words(&t.person).await?;
            // `find` gives only the person's own.
            Json(task_json(&k, &t, &r, true, &words)).into_response()
        }
        None => err(StatusCode::NOT_FOUND, "no such task"),
    })
}

/// The task view's evidence: each step's tool calls, boundary decisions and
/// results, as derived from the log (K45).
pub(super) async fn timeline(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    let Some(view) = k.read(|s| {
        let t = s.tasks.get(&id).filter(|t| visible_task(&p, t))?;
        Some(timeline_of(s.runs.get(&t.run_id)?, false))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no such task");
    };
    Ok(Json(view).into_response())
}

/// A run's steps, each with its tool calls, the boundary's decisions (their
/// stage and outcome: rules and reasons are the developer view's) and
/// their results; its notices and verifications. With `redact_private`, the
/// calls that are the person's alone keep their name but lose their words.
pub(crate) fn timeline_of(run: &Run, redact_private: bool) -> Value {
    let mut steps = vec![];
    for reply in &run.replies {
        let mut calls = vec![];
        let mut said = String::new();
        for b in &reply.blocks {
            match b {
                magnus_adapter_proto::Block::Text { text } => said.push_str(text),
                magnus_adapter_proto::Block::ToolCall { id, name, input } => {
                    let r = run.results.get(id);
                    let decisions: Vec<Value> = run.decisions.iter().filter(|d| &d.call_id == id).map(|d| json!({ "stage": d.stage, "outcome": d.outcome })).collect();
                    let hidden = redact_private && crate::tools::person_only_tool(name);
                    calls.push(json!({
                        "call_id": id, "tool": name, "input": if hidden { json!(crate::tools::REDACTED_CALL) } else { input.clone() },
                        "issuer": run.intents.get(id).map(|i| i.issuer.clone()).unwrap_or_else(|| "model".into()),
                        "decisions": decisions,
                        "result": r.map(|r| if hidden { crate::tools::REDACTED_CALL.to_string() } else { r.text.chars().take(4000).collect::<String>() }),
                        "is_error": r.map(|r| r.is_error), "status": r.map(|r| r.status.clone()),
                    }));
                }
                _ => {}
            }
        }
        steps.push(json!({ "step": reply.step, "text": said, "calls": calls }));
    }
    let notices: Vec<Value> = run.notices.iter().map(|(step, seq, text)| json!({ "step": step, "seq": seq, "text": text })).collect();
    let frames: Vec<Value> = if redact_private { vec![] } else { run.frames.iter().map(|(step, image_b64)| json!({ "step": step, "image_b64": image_b64.as_ref() })).collect() };
    json!({
        "steps": steps,
        "frames": frames,
        "notices": notices,
        "verifications": run.verified.iter().map(|(c, v)| json!({ "criterion": c, "passed": v.0, "evidence": v.1, "by": v.2 })).collect::<Vec<_>>()
    })
}

pub(super) async fn resume(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    // Under the lock cards are answered under: an answer to the card the
    // run stopped on sees it resumed, and resumes or extends nothing more.
    let _decide = k.decide.lock().await;
    let Some((run_id, status)) = k.read(|s| {
        let t = s.tasks.get(&id).filter(|t| visible_task(&p, t))?;
        s.runs.get(&t.run_id).map(|r| (r.id.clone(), r.status))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no such task");
    };
    if status != Status::Suspended {
        return refuse(StatusCode::CONFLICT, "the task is not paused");
    }
    if let Some((_, why)) = k.halt_of(&run_id, None) {
        return refuse(StatusCode::CONFLICT, why.replace("kill switch engaged", "the kill switch is engaged"));
    }
    k.emit(&format!("person:{}", p.id), Ev::Resumed { run: run_id.clone() }).await?;
    k.wake(&run_id);
    Ok(Json(json!({ "ok": true })).into_response())
}

pub(super) async fn cancel(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(controlled(k.cancel_task(&p, &id, None).await.map(|v| {
        let mut v = v;
        v["ok"] = json!(true);
        v
    })))
}

/// Self-service emergency stop for a person's own work: cancels every active or
/// paused task of theirs and pauses all of their active schedules.
pub(super) async fn stop_all(State(k): K, Caller(p): Caller) -> ApiResult {
    let (tasks, schedules): (Vec<String>, Vec<String>) = k.read(|s| {
        let tasks = s
            .tasks
            .values()
            .filter(|t| t.person == p.id && s.team.run_of(&t.task_id).is_none())
            .filter(|t| s.runs.get(&t.run_id).is_some_and(|r| !r.status.terminal()))
            .map(|t| t.task_id.clone())
            .collect();
        let schedules = s.arrangements.schedules_of(&p.id).filter(|sc| sc.ended.is_none() && sc.paused.is_none()).map(|sc| sc.id.clone()).collect();
        (tasks, schedules)
    });
    let mut cancelled = 0usize;
    for id in &tasks {
        if k.cancel_task(&p, id, None).await.is_ok() {
            cancelled += 1;
        }
    }
    let mut paused = 0usize;
    for id in &schedules {
        if k.pause_schedule(&p, id, None).await.is_ok() {
            paused += 1;
        }
    }
    Ok(Json(json!({ "ok": true, "tasks_cancelled": cancelled, "schedules_paused": paused, "cancelled_tasks": cancelled, "paused_schedules": paused })).into_response())
}

/// Steer an active or paused task in flight: appends a steering note from
/// the task's person so the agent reads it on its next step without
/// cancelling or restarting the run.
pub(super) async fn steer(State(k): K, Caller(p): Caller, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    let Some((_, run)) = find(&k, &p, &id) else { return refuse(StatusCode::NOT_FOUND, "no such task") };
    if run.status.terminal() {
        return refuse(StatusCode::CONFLICT, "the task has already ended");
    }
    let text = b["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() || text.chars().count() > crate::compose::MAX_TEXT {
        return refuse(StatusCode::BAD_REQUEST, "steer needs a short instruction");
    }
    let step = run.replies.len() as u32;
    let actor = format!("person:{}", p.id);
    k.emit(&actor, Ev::Notice { run: run.id.clone(), step, text: format!("Steering instruction from the person while this task is running: {text}") }).await?;
    if run.status == crate::state::Status::Running {
        k.wake(&run.id);
    }
    Ok(Json(json!({ "ok": true, "step": step })).into_response())
}

/// The person's verdict on an outcome that has ended: right, partly or
/// wrong, with an optional note. A later verdict replaces it.
pub(super) async fn verdict(State(k): K, Caller(p): Caller, Path(id): Path<String>, Body(b): Body<req::Verdict>) -> ApiResult {
    // An experiment's trials run as the eval people, who have no sign-in of
    // their own: the owner judges them, though they see no one else's tasks.
    let judge = rights(&p).judges_experiments();
    let trial = || k.read(|s| s.tasks.get(&id).and_then(|t| s.runs.get(&t.run_id).filter(|r| r.experiment.is_some()).map(|r| (t.clone(), r.clone()))));
    let Some((task, run)) = find(&k, &p, &id).or_else(|| judge.then(trial).flatten()) else { return refuse(StatusCode::NOT_FOUND, "no such task") };
    if task.person != p.id && !(judge && run.experiment.is_some()) {
        return refuse(StatusCode::FORBIDDEN, "only the person it is for gives a verdict");
    }
    let Some(v) = b.verdict.as_deref().filter(|v| crate::state::VERDICTS.contains(v)) else {
        return refuse(StatusCode::BAD_REQUEST, "verdict is right, partly or wrong");
    };
    let note = b.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(String::from);
    if note.as_ref().is_some_and(|n| n.chars().count() > crate::compose::MAX_TEXT) {
        return refuse(StatusCode::BAD_REQUEST, format!("the note is longer than {} characters", crate::compose::MAX_TEXT));
    }
    if !run.status.terminal() {
        return refuse(StatusCode::CONFLICT, "the task has not ended");
    }
    let ev = Ev::Verdict { task: task.task_id.clone(), person: p.id.clone(), verdict: v.into(), note };
    Ok(match k.emit(&format!("person:{}", p.id), ev).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    })
}

/// Opening the app's home: what changed since the last open — ended
/// tasks, decided cards and new arrivals — with the queued
/// observations, newest first; the open itself is recorded so the next
/// one starts here (#367). A first open looks back a day.
pub(super) async fn home_opened(State(k): K, Caller(p): Caller) -> ApiResult {
    let words = k.person_words(&p.id).await?;
    let now = chrono::Utc::now();
    let (since, mut ended, mut arrived, mut decided, mut observations) = k.read(|s| {
        let since = s.identity.home_last_open(&p.id).unwrap_or(now - chrono::Duration::hours(24));
        let mut ended: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut arrived: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut decided: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        let mut observations: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];
        for t in s.tasks.values().filter(|t| visible_task(&p, t) && s.listed(t) && !t.review && !t.poll) {
            if let Some(r) = s.runs.get(&t.run_id) {
                // A schedule's routine run is not news on coming back.
                if matches!(r.status, Status::Completed | Status::Failed) && !crate::outcomes::routine(s, t, r) {
                    if let Some(ended_at) = r.ended_at.filter(|e| *e > since) {
                        ended.push((ended_at, task_row(&k, s, &p, t, r, &words)));
                    }
                }
            }
        }
        for i in s.inbox.items().values().filter(|i| inbox::visible(s, &p, i)) {
            let row = inbox::render(s, &p, i);
            if i.created_at > since {
                arrived.push((i.created_at, row.clone()));
            }
            if i.resolution.is_some() && s.inbox.items_resolved_at().get(&i.id).is_some_and(|a| *a > since) {
                decided.push((s.inbox.items_resolved_at()[&i.id], row.clone()));
            }
            if i.resolution.is_none() && (i.kind == "observation" || i.kind == "interrupt") {
                observations.push((i.created_at, row));
            }
        }
        (since, ended, arrived, decided, observations)
    });
    for feed in [&mut ended, &mut arrived, &mut decided, &mut observations] {
        feed.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    }
    let feed = Json(json!({
        "since": since,
        "ended": ended.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "arrived": arrived.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "decided": decided.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "observations": observations.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    }));
    Ok(match k.emit(&format!("person:{}", p.id), Ev::HomeOpened { person: p.id.clone() }).await {
        // The feed was built before the open landed: if the open did
        // not land, the next open shows it all again.
        Ok(_) => feed.into_response(),
        Err(e) => internal(e),
    })
}

/// The app presented inbox items to its person: record each visible
/// one opened, first presentation standing (#367). Unknown ids, and
/// another person's, are ignored, so a stale app never fails the open.
pub(super) async fn inbox_opened(State(k): K, Caller(p): Caller, Body(b): Body<req::Opened>) -> ApiResult {
    let (marked, fresh): (Vec<String>, Vec<String>) = k.read(|s| {
        let mut marked = vec![];
        let mut fresh = vec![];
        for id in b.ids {
            if s.inbox.items().get(&id).is_some_and(|i| inbox::visible(s, &p, i)) {
                marked.push(id.clone());
                if !s.inbox.items_opened().contains_key(&id) {
                    fresh.push(id);
                }
            }
        }
        (marked, fresh)
    });
    if !fresh.is_empty() {
        let actor = format!("person:{}", p.id);
        let evs: Vec<(&str, Ev)> = fresh.iter().map(|id| (actor.as_str(), Ev::ItemOpened { id: id.clone() })).collect();
        k.emit_all(evs).await?;
    }
    Ok(Json(json!({ "opened": marked })).into_response())
}

pub(super) async fn inbox_list(State(k): K, Caller(p): Caller, h: HeaderMap) -> ApiResult {
    let etag = seq_etag(&k);
    if let Some(r) = not_modified(&h, &etag) {
        return Ok(r);
    }
    let mut items: Vec<Value> = k.read(|s| s.inbox.items().values().filter(|i| inbox::visible(s, &p, i)).map(|i| inbox::render(s, &p, i)).collect());
    // An approval for an app's action carries the app's card, as the person
    // whose task it is stands with it (connectors spec: the same card in the
    // Library, in chat, in a task and in approvals).
    let apps: Vec<String> = k.connectors().into_iter().map(|c| c.name).collect();
    for i in items.iter_mut().filter(|i| i["kind"] == "approval") {
        let app = i["details"]["tool"].as_str().and_then(|t| t.split_once("__")).map(|(a, _)| a.to_string());
        if let Some(app) = app.filter(|a| apps.contains(a)) {
            i["app"] = k.offered_card(i["person"].as_str().unwrap_or_default(), &json!({ "connector": app }));
        }
    }
    Ok(with_etag(Json(json!({ "items": items })).into_response(), &etag))
}

pub(super) async fn respond(State(k): K, Caller(p): Caller, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    Ok(match k.respond(&p, &id, &b).await {
        Ok(Answer::Ok) => Json(json!({ "ok": true })).into_response(),
        Ok(Answer::Forbidden(m)) => err(StatusCode::FORBIDDEN, m),
        Ok(Answer::NotFound) => err(StatusCode::NOT_FOUND, "no such item"),
        Ok(Answer::Conflict(m)) => err(StatusCode::CONFLICT, m),
        Ok(Answer::Invalid(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(e) => internal(e),
    })
}

/// Model text for a task as it streams (server-sent events), before the
/// turn is appended to the log (K6).
/// The stream asks again, this often, whether its session is still signed
/// in: one ended (sign-out, revoked, disabled) stops getting the run's text.
const LIVE_RECHECK: std::time::Duration = std::time::Duration::from_secs(15);

pub(super) async fn live(State(k): K, Caller(p): Caller, h: HeaderMap, Path(id): Path<String>) -> ApiResult {
    let Some(run_id) = k.read(|s| s.tasks.get(&id).filter(|t| visible_task(&p, t)).map(|t| t.run_id.clone())) else {
        return refuse(StatusCode::NOT_FOUND, "no such task");
    };
    let (so_far, mut rx) = k.live_text.subscribe(&run_id);
    let stream = async_stream(move |tx| async move {
        if !so_far.is_empty() && tx.send(format!("data: {}\n\n", json!({ "text": so_far }))).await.is_err() {
            return;
        }
        // Ends when the client goes, the session ends or the run ends: a
        // stream left open on a finished run would wake for every run's text.
        let mut tick = tokio::time::interval(LIVE_RECHECK);
        loop {
            tokio::select! {
                () = tx.closed() => return,
                _ = tick.tick() => {
                    let ended = k.read(|s| s.runs.get(&run_id).is_none_or(|r| r.status.terminal()));
                    if ended || !who(&k, &h).await.is_ok_and(|q| q.id == p.id) {
                        return;
                    }
                }
                got = rx.recv() => match got {
                    Ok((r, text)) if r == run_id => {
                        if tx.send(format!("data: {}\n\n", json!({ "text": text }))).await.is_err() {
                            return;
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return,
                },
            }
        }
    });
    let mut r = axum::body::Body::from_stream(stream).into_response();
    r.headers_mut().insert("content-type", HeaderValue::from_static("text/event-stream"));
    r.headers_mut().insert("cache-control", HeaderValue::from_static("no-cache"));
    Ok(r)
}

fn async_stream<F, Fut>(f: F) -> impl futures::Stream<Item = Result<String, std::convert::Infallible>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<String>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(f(tx));
    futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|s| (Ok(s), rx)) })
}

pub(crate) use super::schedules::end_for;

/// Download a delivered file. Read from the log, the source of truth.
pub(super) async fn output(State(k): K, Caller(p): Caller, Path((id, out)): Path<(String, String)>) -> ApiResult {
    let Some((run_id, has_out)) = k.read(|s| {
        let t = s.tasks.get(&id).filter(|t| visible_task(&p, t))?;
        s.runs.get(&t.run_id).map(|r| (r.id.clone(), r.outputs.iter().any(|(o, ..)| o == &out)))
    }) else {
        return refuse(StatusCode::NOT_FOUND, "no such task");
    };
    if !has_out {
        return refuse(StatusCode::NOT_FOUND, "no such file");
    }
    Ok(match k.output_bytes(&run_id, &out).await {
        Ok(Some((name, mt, bytes))) => crate::library::attachment(&name, &mt, bytes),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such file"),
        Err(e) => internal(e),
    })
}
