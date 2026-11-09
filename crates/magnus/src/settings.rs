//! What a person sets about themselves in Settings, and what the owner sets
//! about people: signed-in devices, roles, notifications and privacy
//! choices, and requests to delete their data.

use crate::api::{credential, err, internal, owner, who};
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

/// A person's own choices and the defaults they start from: notified when
/// something needs them now and when the agent changes, not for what can
/// wait; problem reports leave the appliance only with consent.
pub fn preferences(k: &Kernel, person: &str) -> Value {
    let mut p = json!({ "notify_blocking": true, "notify_batched": false, "notify_agent_changes": true, "feedback_consent": false, "timezone": "UTC", "working_hours": { "start": "09:00", "end": "17:00" }, "quiet_hours": { "start": "22:00", "end": "07:00" }, "review_every_minutes": k.cfg.proactive.review_every_minutes, "interruptions_per_day": k.cfg.proactive.interruptions_per_day });
    if let Some(set) = k.read(|s| s.preferences.get(person).cloned()) {
        for (key, v) in set.as_object().into_iter().flatten() {
            p[key] = v.clone();
        }
    }
    p
}

/// Whether a proactive loop setting holds what the loop can use (#237).
fn proactive_ok(key: &str, v: &Value) -> bool {
    let hhmm = |s: &str| s.split_once(':').is_some_and(|(h, m)| h.parse::<u32>().is_ok_and(|h| h < 24) && m.parse::<u32>().is_ok_and(|m| m < 60));
    match key {
        "timezone" => v.as_str().is_some_and(|z| z.parse::<chrono_tz::Tz>().is_ok()),
        "working_hours" | "quiet_hours" => ["start", "end"].iter().all(|k| v.get(k).and_then(Value::as_str).is_some_and(hhmm)),
        "review_every_minutes" | "interruptions_per_day" => v.as_u64().is_some_and(|n| n > 0),
        _ => false,
    }
}

/// The days the owner has to act on a request to delete a person's data
/// (GDPR Art. 12(3): one month).
pub const DELETION_DAYS: i64 = 30;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/sessions", get(sessions))
        .route("/v1/sessions/{id}", delete(end_session))
        .route("/v1/people", get(people))
        .route("/v1/people/{id}/roles", put(set_roles))
        .route("/v1/people/{id}/data/{what}", get(person_data))
        .route("/v1/data-access", get(get_data_access).put(put_data_access))
        .route("/v1/people/{id}/account", delete(clear_account))
        .route("/v1/preferences", get(get_preferences).put(put_preferences))
        .route("/v1/deletion-requests", get(deletion_requests).post(request_deletion))
}

/// The devices signed in as the person, newest first.
async fn sessions(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let token = credential(&h).unwrap_or_default();
    let list: Vec<Value> = k
        .accounts
        .sessions_of(&p.id, token)
        .await
        .into_iter()
        .map(|(s, current)| json!({ "id": s.id, "client": s.client, "created": s.created, "expires": s.expires, "current": current }))
        .collect();
    Json(json!({ "sessions": list })).into_response()
}

/// Sign one of the person's devices out.
async fn end_session(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.accounts.end(&p.id, &id).await {
        Ok(Some(s)) => match k.emit(&format!("person:{}", p.id), Ev::SignedOut { person: p.id.clone(), session: s.id }).await {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => internal(e),
        },
        Ok(None) => err(StatusCode::NOT_FOUND, "no such session"),
        Err(e) => internal(e),
    }
}

/// Everyone on the appliance with their rights and where the rights come from.
async fn people(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = owner(&k, &h).await {
        return r;
    }
    let list: Vec<Value> = k
        .cfg
        .people
        .iter()
        .filter_map(|c| {
            let p = k.person(&c.id)?;
            let from = k.read(|s| {
                if s.provider_rights.get(&c.id).is_some_and(|(_, r)| r.is_some()) {
                    "provider"
                } else if s.roles_set.contains_key(&c.id) {
                    "settings"
                } else {
                    "config"
                }
            });
            let bound = k.read(|s| s.subjects.contains_key(&c.id));
            Some(json!({ "id": p.id, "name": p.name, "groups": p.groups, "roles": p.roles, "roles_from": from, "account_bound": bound }))
        })
        .collect();
    Json(json!({ "people": list })).into_response()
}

/// The owner sets a person's roles. Roles the identity provider names win,
/// so those cannot be set here; and someone must stay owner.
async fn set_roles(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let me = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(roles) = b["roles"].as_array().map(|a| a.iter().filter_map(|r| r.as_str()).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect::<Vec<_>>()) else {
        return err(StatusCode::BAD_REQUEST, "roles must be a list of role names");
    };
    if k.cfg.person(&id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such person");
    }
    if k.read(|s| s.provider_rights.get(&id).is_some_and(|(_, r)| r.is_some())) {
        return err(StatusCode::CONFLICT, "the identity provider names this person's roles; change them there");
    }
    // Checked and recorded under the decision lock: two owners taking the
    // role from each other at once must leave one.
    let _decide = k.decide.lock().await;
    let still_owned = k.cfg.people.iter().filter(|c| c.id != id).filter_map(|c| k.person(&c.id)).any(|p| p.is_owner()) || roles.iter().any(|r| r == "owner");
    if !still_owned {
        return err(StatusCode::CONFLICT, "someone must keep the owner role");
    }
    match k.emit(&format!("person:{}", me.id), Ev::RolesSet { person: id, roles }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// Whether people's data is private from the admin: by default a person's
/// data is the organisation's, and the owner can make it private (owner's
/// ruling 14, 2026-11-01).
async fn get_data_access(State(k): K, h: HeaderMap) -> Response {
    if let Err(r) = who(&k, &h).await {
        return r;
    }
    Json(json!({ "private": k.read(|s| s.data_private) })).into_response()
}

async fn put_data_access(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let me = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(private) = b["private"].as_bool() else { return err(StatusCode::BAD_REQUEST, "private is true or false") };
    match k.emit(&format!("person:{}", me.id), Ev::DataAccessSet { private }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// The owner lets a person's account at the identity provider change: the
/// person is no longer bound to the account that first signed in as them,
/// and the next account to sign in as them becomes theirs (for an account
/// made again, or a person moved to a new one).
async fn clear_account(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let me = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if k.cfg.person(&id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such person");
    }
    let _decide = k.decide.lock().await;
    if !k.read(|s| s.subjects.contains_key(&id)) {
        return StatusCode::NO_CONTENT.into_response();
    }
    match k.emit(&format!("person:{}", me.id), Ev::SubjectBound { person: id, subject: None }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// The admin reads a person's tasks, conversations, files or memory, unless
/// the organisation's policy makes people's data private. Each read is in
/// the log and in the person's inbox.
async fn person_data(State(k): K, h: HeaderMap, Path((id, what)): Path<(String, String)>) -> Response {
    let me = match owner(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(target) = k.person(&id) else { return err(StatusCode::NOT_FOUND, "no such person") };
    match k.read_person_data(&format!("person:{}", me.id), &me.name, &target, &what, None).await {
        Ok(v) => Json(v).into_response(),
        Err((st, m)) => err(st, m),
    }
}

async fn get_preferences(State(k): K, h: HeaderMap) -> Response {
    match who(&k, &h).await {
        Ok(p) => Json(preferences(&k, &p.id)).into_response(),
        Err(r) => r,
    }
}

/// Change some of the person's own settings; each must be a known one,
/// a yes or no or, for the proactive loop's, what the loop can use.
async fn put_preferences(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let known = preferences(&k, &p.id);
    let Some(set) = b.as_object().filter(|o| !o.is_empty()) else { return err(StatusCode::BAD_REQUEST, "give the settings to change") };
    for (key, v) in set {
        if known.get(key).is_none() {
            return err(StatusCode::BAD_REQUEST, format!("no setting {key}"));
        }
        if !v.is_boolean() && !proactive_ok(key, v) {
            return err(StatusCode::BAD_REQUEST, format!("{key} holds the wrong shape"));
        }
    }
    match k.emit(&format!("person:{}", p.id), Ev::PreferencesSet { person: p.id.clone(), prefs: b }).await {
        Ok(_) => Json(preferences(&k, &p.id)).into_response(),
        Err(e) => internal(e),
    }
}

/// A person asks the owner to delete their data. The request is an inbox
/// item for the owner, with the date it must be acted on by.
async fn request_deletion(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let what = b["what"].as_str().map(str::trim).filter(|w| !w.is_empty()).unwrap_or("all of my data");
    let due = chrono::Utc::now() + chrono::Duration::days(DELETION_DAYS);
    let id = crate::kernel::new_id("item");
    let ev = Ev::Item {
        id: id.clone(),
        item_kind: "deletion_request".into(),
        task: None,
        run: None,
        person: p.id.clone(),
        summary: format!("{} asks for their data to be deleted: {what}", p.name),
        details: json!({ "requested_by": p.id, "what": what, "due": due }),
    };
    match k.emit(&format!("person:{}", p.id), ev).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({ "id": id, "due": due }))).into_response(),
        Err(e) => internal(e),
    }
}

/// The person's own deletion requests and what became of them; the owner
/// sees everyone's.
async fn deletion_requests(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let list: Vec<Value> = k.read(|s| {
        let mut v: Vec<&crate::state::Item> = s.items.values().filter(|i| i.kind == "deletion_request" && (i.person == p.id || p.is_owner())).collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v.into_iter()
            .map(|i| {
                json!({ "id": i.id, "person": i.person, "what": i.details["what"], "due": i.details["due"], "created_at": i.created_at,
                    "decision": i.resolution.as_ref().map(|r| r.0.clone()), "note": i.resolution.as_ref().and_then(|r| r.2.clone()) })
            })
            .collect()
    });
    Json(json!({ "requests": list })).into_response()
}
