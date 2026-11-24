//! The HTTP side of co-editing: sharing a file to co-edit, writing a
//! version of one, its history, a check-out, and a person's open changes
//! (those that conflicted, and their agent's edits waiting in their
//! Airlock), which they land, deciding each conflict, or throw away.

use super::commit::Draft;
use super::merge::Choices;
use super::{Author, Landed};
use crate::api_error::{refuse, ApiResult};
use crate::config::Person;
use crate::kernel::Kernel;
use crate::team::caller::{at, Member};
use crate::team::chat::Chat;
use crate::team::chat_routes::name_of;
use crate::team::fit_name;
use crate::team::routes::{send, K};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// What a share asks for in its `share` query: the file is the project's to co-edit.
pub(in crate::team) const CO_EDIT: &str = "coedit";

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/coedit/{name}", get(history))
        .route("/v1/workspaces/{ws}/coedit/{name}/versions/{n}", get(version))
        .route("/v1/workspaces/{ws}/coedit/{name}/checkout", post(check_out))
        .route("/v1/workspaces/{ws}/coedit/{name}/checkin", post(check_in))
        .route("/v1/workspaces/{ws}/coedit/{name}/revert", post(revert))
        .route("/v1/workspaces/{ws}/coedit-changes", get(changes))
        .route("/v1/workspaces/{ws}/coedit-changes/{id}/land", post(land))
        .route("/v1/workspaces/{ws}/coedit-changes/{id}/discard", post(discard))
}

fn actor(p: &Person) -> String {
    format!("person:{}", p.id)
}

/// What a change came to, as the app reads it: a conflict is a 409 with
/// both versions of each part.
fn answer(landed: Landed) -> Response {
    match landed {
        Landed::Version(n) => Json(json!({ "ok": true, "version": n })).into_response(),
        Landed::Proposed(id) => Json(json!({ "ok": true, "proposed": id })).into_response(),
        Landed::Rejected { id, head, conflicts } => {
            let error = "the file changed since your version, and some parts of yours conflict with it: decide each, and nothing is lost";
            (StatusCode::CONFLICT, Json(json!({ "error": error, "change": id, "head": head, "conflicts": conflicts }))).into_response()
        }
        Landed::Refused(why) => (StatusCode::CONFLICT, Json(json!({ "error": why }))).into_response(),
    }
}

/// A write to the volume (`PUT /files/{name}`) that is co-editing's: a new
/// file shared to co-edit, or a version of a co-edited one, made on the
/// `base` it names. `None` leaves it to a shared copy's write.
pub(in crate::team) async fn write(k: &Kernel, p: &Person, ws: &str, name: &str, q: &HashMap<String, String>, body: &Bytes) -> ApiResult<Option<Response>> {
    let co_edited = k.read(|s| s.team.workspace(ws).is_some_and(|w| w.coedit.files.contains_key(name)));
    if co_edited {
        if !k.read(|s| s.team.workspace(ws).is_some_and(|w| w.sees_file(name, &p.id))) {
            return refuse(StatusCode::NOT_FOUND, "no such file");
        }
        let Some(base) = q.get("base").and_then(|b| b.parse::<u32>().ok()) else {
            return refuse(StatusCode::BAD_REQUEST, "say which version this change was made on (base), so nothing that landed since is lost");
        };
        let author = Author { person: p.id.clone(), run: None };
        let landed = k.coedit_write(&actor(p), Draft { ws, name, base, mine: body, author: &author }).await?;
        return Ok(Some(answer(landed)));
    }
    let taken = |k: &Kernel| k.read(|s| s.team.workspace(ws).is_some_and(|w| w.files.contains_key(name)));
    if q.get("share").map(String::as_str) != Some(CO_EDIT) || taken(k) {
        return Ok(None);
    }
    let evs = match k.coedit_share(ws, &p.id, name, body) {
        Ok(evs) => evs,
        Err(why) => return refuse(StatusCode::BAD_REQUEST, format!("{why}; share a copy instead, or fix the file")),
    };
    // The name is found free and the file shared under it in one step.
    let held = super::share_lock().await;
    if taken(k) {
        return refuse(StatusCode::CONFLICT, format!("{name} was shared by someone else a moment ago; open it, and make your change on its version"));
    }
    let dir = crate::team::agent::volume_dir(k, ws);
    if let Err(e) = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(name), body).await
    }
    .await
    {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"));
    }
    let a = actor(p);
    k.emit_all(evs.into_iter().map(|e| (a.as_str(), e)).collect()).await?;
    drop(held);
    // The sharer keeps their own copy, as it was when shared.
    k.keep_in_library(&a, &p.id, name, body).await?;
    let project = q.get("project").filter(|id| k.read(|s| s.team.workspace(ws).is_some_and(|w| w.projects.get(*id).is_some_and(|d| d.may_read(&p.id))))).cloned();
    if project.is_some() {
        send(k, p, crate::team::Change::Chat { workspace: ws.into(), chat: Chat::FileIn { name: name.into(), project } }).await?;
    }
    Ok(Some(Json(json!({ "ok": true, "version": 1 })).into_response()))
}

/// A co-edited file this person may read, or a 404.
fn readable(k: &Kernel, ws: &str, name: &str, person: &str) -> ApiResult<super::Coedited> {
    let f = k.read(|s| s.team.workspace(ws).filter(|w| fit_name(name) && w.sees_file(name, person)).and_then(|w| w.coedit.files.get(name).cloned()));
    f.map_or_else(|| refuse(StatusCode::NOT_FOUND, "no such co-edited file"), Ok)
}

/// Its history: every version with its base and author, who has it checked
/// out, and for a sheet its values now.
async fn history(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    let f = readable(&k, &ws, &name, &p.id)?;
    let now = chrono::Utc::now();
    let versions: Vec<Value> = f
        .versions
        .values()
        .rev()
        .map(|m| json!({ "number": m.number, "base": m.base, "author": m.author.person, "author_name": name_of(&k, &m.author.person), "by_agent": m.author.run.is_some(), "at": m.at }))
        .collect();
    let head = f.head().map_or(0, |h| h.number);
    let values = match super::merge::values(f.format, &k.coedit_bytes(&ws, &name, head).await?.unwrap_or_default()) {
        Some(v) if f.format == super::Format::Sheet => Value::String(v),
        _ => Value::Null,
    };
    let lease = f.held(now).map(|l| json!({ "person": l.person, "name": name_of(&k, &l.person), "until": l.until, "mine": l.person == p.id }));
    Ok(Json(json!({ "name": name, "format": f.format, "version": head, "versions": versions, "checked_out": lease, "values": values })).into_response())
}

async fn version(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path((ws, name, n)): Path<(String, String, u32)>) -> ApiResult {
    readable(&k, &ws, &name, &p.id)?;
    match k.coedit_bytes(&ws, &name, n).await? {
        Some(bytes) => Ok(bytes.into_response()),
        None => refuse(StatusCode::NOT_FOUND, "no such version"),
    }
}

async fn check_out(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    readable(&k, &ws, &name, &p.id)?;
    match k.coedit_check_out(&actor(&p), &ws, &name, &p.id).await? {
        Ok(until) => Ok(Json(json!({ "ok": true, "until": until })).into_response()),
        Err(why) => refuse(StatusCode::CONFLICT, why),
    }
}

async fn check_in(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, name)): Path<(String, String)>) -> ApiResult {
    readable(&k, &ws, &name, &p.id)?;
    if !k.coedit_check_in(&actor(&p), &ws, &name, &p.id).await? {
        return refuse(StatusCode::CONFLICT, "you do not have it checked out");
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct Revert {
    to: u32,
}

async fn revert(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, name)): Path<(String, String)>, Json(b): Json<Revert>) -> ApiResult {
    readable(&k, &ws, &name, &p.id)?;
    Ok(answer(k.coedit_revert(&actor(&p), &ws, &name, b.to, &p.id).await?))
}

/// This person's open changes here: each that conflicted, with both
/// versions of what did, and each edit of their agent's that waits with its
/// lane's result in their Airlock.
async fn changes(State(k): K, Member { person: p, .. }: Member<at::Guest>, Path(ws): Path<String>) -> ApiResult {
    let open: Vec<Value> = k.read(|s| {
        let Some(w) = s.team.workspace(&ws) else { return vec![] };
        w.coedit.of_person(&p.id).filter(|o| w.sees_file(&o.name, &p.id)).map(|o| json!(o)).collect()
    });
    Ok(Json(json!({ "changes": open })).into_response())
}

#[derive(Deserialize, Default)]
struct Decided {
    #[serde(default)]
    choices: Choices,
}

async fn land(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, id)): Path<(String, String)>, Json(b): Json<Decided>) -> ApiResult {
    Ok(answer(k.coedit_settle(&actor(&p), &ws, &p.id, &id, &b.choices).await?))
}

async fn discard(State(k): K, Member { person: p, .. }: Member<at::Member>, Path((ws, id)): Path<(String, String)>) -> ApiResult {
    if !k.coedit_discard(&actor(&p), &ws, &p.id, &id).await? {
        return refuse(StatusCode::NOT_FOUND, "you have no such open change");
    }
    Ok(Json(json!({ "ok": true })).into_response())
}
