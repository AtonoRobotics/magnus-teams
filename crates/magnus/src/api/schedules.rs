//! Schedule HTTP endpoints (split out of `api/work.rs`).

use super::work::task_row;
use super::{controlled, internal, not_modified, seq_etag, with_etag, K};
use crate::api_error::{refuse, ApiResult};
use crate::events::Ev;
use crate::extract::{Body, Caller};
use crate::kernel::new_id;
use crate::state::Run;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

pub(super) async fn schedules(State(k): K, Caller(p): Caller, h: HeaderMap) -> ApiResult {
    let etag = seq_etag(&k, &p.id);
    if let Some(r) = not_modified(&h, &etag) {
        return Ok(r);
    }
    let words = k.person_words(&p.id).await?;
    let list: Vec<Value> = k.read(|s| {
        s.arrangements
            .schedules_of(&p.id)
            .map(|x| {
                // Its runs, newest first; the last result is the newest that ended.
                let mut runs: Vec<(&crate::state::Task, &Run)> =
                    s.tasks.values().filter(|t| t.schedule.as_deref() == Some(x.id.as_str())).filter_map(|t| s.runs.get(&t.run_id).map(|r| (t, r))).collect();
                runs.sort_by_key(|a| std::cmp::Reverse(a.0.created_at));
                let rows: Vec<Value> = runs.iter().map(|(t, r)| task_row(&k, s, &p, t, r, &words)).collect();
                // Every firing shows: the newest task whatever its state, and firings that could not start one.
                let last = rows.first().cloned();
                let failures: Vec<Value> = x.failures.iter().rev().map(|(at, why)| json!({ "at": at, "error": why })).collect();
                let last_failed = x
                    .failures
                    .last()
                    .filter(|(at, _)| rows.first().and_then(|r| r["created_at"].as_str()).and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok()).is_none_or(|c| c < *at))
                    .map(|(at, why)| json!({ "at": at, "error": why }));
                let title = words.title(x.seq).unwrap_or_else(|| words.text(x.seq).to_string());
                let is_system = x.system || crate::outcomes::is_heartbeat_text(Some(&title), words.text(x.seq));
                json!({
                    "id": x.id, "person": x.person, "text": words.text(x.seq), "title": title, "system": is_system,
                    "every_minutes": x.every_minutes, "daily_at": x.daily_at, "next": x.next_due(), "last_fired": x.last_fired,
                    "until": x.until, "max_runs": x.max_runs, "started": x.started, "paused": x.paused, "paused_on": x.paused_on, "ended": x.ended,
                    // Each run's limit: the server's per-task budget.
                    "budget": crate::task_budget::scheduled(&k.cfg.budget, x.budget.as_ref()),
                    "skipped": x.skipped.as_ref().map(|(at, task, why)| json!({ "at": at, "task": task, "why": why })),
                    "last": last, "last_failed": last_failed, "failures": failures, "runs": rows,
                    "project": s.project_of(&x.id).filter(|_| x.person == p.id).and_then(|id| s.library.projects().get(&id)).map(|p| json!({ "id": p.id, "name": p.name })),
                })
            })
            .collect()
    });
    Ok(with_etag(Json(json!({ "schedules": list })).into_response(), &etag))
}

pub(super) async fn add_schedule(State(k): K, Caller(p): Caller, Body(b): Body<Value>) -> ApiResult {
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "the schedule has no task text");
    }
    let (every_minutes, daily_at) = match crate::cadence::rule_of(&b, k.cfg.schedules.min_every_minutes) {
        Ok(r) => r,
        Err(m) => return refuse(StatusCode::BAD_REQUEST, m),
    };
    let (until, max_runs) = match end_for(&b) {
        Ok(r) => r,
        Err(m) => return refuse(StatusCode::BAD_REQUEST, m),
    };
    let project = crate::library::chosen_project(&k, &p, &b)?;
    let title = b["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
    let system = b["system"].as_bool().unwrap_or(false) || crate::outcomes::is_heartbeat_text(title.as_deref(), &text);
    let id = new_id("sched");
    let actor = format!("person:{}", p.id);
    if let Err(e) = k
        .emit(
            &actor,
            Ev::ScheduleSet {
                id: id.clone(),
                person: p.id.clone(),
                text,
                every_minutes,
                daily_at,
                budget: None,
                card: None,
                label: None,
                title,
                until,
                max_runs,
                scope: None,
                // Set up in settings, outside a conversation: no chat thread.
                thread: None,
                quarantine_schema: None,
                system,
            },
        )
        .await
    {
        return Ok(internal(e));
    }
    if project.is_some() {
        k.emit(&actor, Ev::Moved { item: id.clone(), project }).await?;
    }
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "system": system }))).into_response())
}

/// Start a paused schedule again: failures count afresh, and the next run
/// is due one interval from now.
pub(super) async fn resume_schedule(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(match controlled(k.resume_schedule(&p, &id, None).await.map(|_| Value::Null)) {
        r if r.status() == StatusCode::OK => StatusCode::NO_CONTENT.into_response(),
        r => r,
    })
}

/// Pause: no more runs until it is resumed.
pub(super) async fn pause_schedule(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(match controlled(k.pause_schedule(&p, &id, None).await.map(|_| Value::Null)) {
        r if r.status() == StatusCode::OK => StatusCode::NO_CONTENT.into_response(),
        r => r,
    })
}

/// Change when it runs and when it ends.
pub(super) async fn edit_schedule(State(k): K, Caller(p): Caller, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    Ok(controlled(k.edit_schedule(&p, &id, &b, None).await))
}

/// Stop: no more runs, and one still going stops; the schedule stays, ended.
pub(super) async fn stop_schedule(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    Ok(controlled(k.stop_schedule(&p, &id).await))
}

/// Delete: the schedule is gone, and a run of it still going stops.
pub(super) async fn remove_schedule(State(k): K, Caller(p): Caller, Path(id): Path<String>) -> ApiResult {
    // The report goes back with the status: what each stopped run had
    // done, and how many actually stopped (#263).
    Ok(controlled(k.cancel_schedule(&p, &id, None).await))
}

/// When a schedule ends: `until` (RFC 3339) and `max_runs`, either, both or
/// neither. A model that must fill every field sends "" or 0 for one it
/// does not mean.
pub(crate) fn end_for(b: &Value) -> Result<(Option<chrono::DateTime<chrono::Utc>>, Option<u32>), String> {
    let until = match b["until"].as_str().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(chrono::DateTime::parse_from_rfc3339(u).map_err(|e| format!("until is not an RFC 3339 time ({e})"))?.with_timezone(&chrono::Utc)),
        None => None,
    };
    if until.is_some_and(|u| u <= chrono::Utc::now()) {
        return Err("until is already past".into());
    }
    let max_runs = b["max_runs"].as_u64().filter(|m| *m > 0).map(|m| u32::try_from(m).unwrap_or(u32::MAX));
    Ok((until, max_runs))
}
