//! Projects and their knowledge bases, over HTTP. A base is open to its
//! people; correcting it takes its curators.
use super::knowledge::{Entry, Flag, Project, Relation, Retired, Weight};
use super::routes::{member, new_id, send, Reply, K};
use super::{Change, Role, Workspace};
use crate::api::err;
use crate::config::Person;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_NAME_CHARS: usize = 80;
const MAX_STATEMENT_CHARS: usize = 4000;
const DEFAULT_KIND: &str = "fact";

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/projects", post(add_project))
        .route("/v1/workspaces/{ws}/projects/{id}", get(project).put(set_project))
        .route("/v1/workspaces/{ws}/channels/{channel}/project", post(link_channel))
        .route("/v1/workspaces/{ws}/knowledge", get(list).post(add))
        .route("/v1/workspaces/{ws}/knowledge/export", get(export))
        .route("/v1/workspaces/{ws}/knowledge/{id}", delete(remove))
        .route("/v1/workspaces/{ws}/knowledge/{id}/flag", post(flag))
        .route("/v1/workspaces/{ws}/knowledge/{id}/correct", post(correct))
        .route("/v1/workspaces/{ws}/knowledge/{id}/retire", post(retire))
}

fn workspace(k: &Kernel, ws: &str) -> Option<Workspace> {
    k.read(|s| s.team.workspace(ws).cloned())
}

#[derive(Deserialize)]
struct NewProject {
    name: String,
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    curators: Vec<String>,
}

fn all_members(w: &Workspace, people: &[String]) -> bool {
    people.iter().all(|p| w.members.contains_key(p))
}

async fn add_project(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewProject>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    if b.name.trim().is_empty() || b.name.chars().count() > MAX_NAME_CHARS {
        return err(StatusCode::BAD_REQUEST, "a project needs a short name");
    }
    if !all_members(&w, &b.members) || !all_members(&w, &b.curators) {
        return err(StatusCode::BAD_REQUEST, "a project's people are members of the workspace");
    }
    let id = new_id();
    let def = Project { id: id.clone(), name: b.name, owner: p.id.clone(), members: b.members, curators: b.curators, ..Default::default() };
    ok_or_return!(send(&k, &p, Change::Project { workspace: ws, id: id.clone(), def }).await);
    Json(json!({ "id": id })).into_response()
}

async fn project(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let found = k.read(|s| s.team.workspace(&ws).and_then(|w| w.projects.get(&id).cloned()));
    match found {
        Some(def) if def.may_read(&p.id) => Json(def).into_response(),
        _ => err(StatusCode::NOT_FOUND, "no such project"),
    }
}

#[derive(Deserialize)]
struct ProjectChange {
    #[serde(default)]
    members: Option<Vec<String>>,
    #[serde(default)]
    curators: Option<Vec<String>>,
    #[serde(default)]
    kinds: Option<Vec<String>>,
    #[serde(default)]
    relations: Option<Vec<String>>,
}

/// The project's owner names its people and its own types.
async fn set_project(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>, Json(b): Json<ProjectChange>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let Some(mut def) = w.projects.get(&id).filter(|d| d.may_read(&p.id)).cloned() else { return err(StatusCode::NOT_FOUND, "no such project") };
    if def.owner != p.id {
        return err(StatusCode::FORBIDDEN, "only the project's owner changes its people and types");
    }
    if !all_members(&w, b.members.as_deref().unwrap_or_default()) || !all_members(&w, b.curators.as_deref().unwrap_or_default()) {
        return err(StatusCode::BAD_REQUEST, "a project's people are members of the workspace");
    }
    def.members = b.members.unwrap_or(def.members);
    def.curators = b.curators.unwrap_or(def.curators);
    def.kinds = b.kinds.unwrap_or(def.kinds);
    def.relations = b.relations.unwrap_or(def.relations);
    ok_or_return!(send(&k, &p, Change::Project { workspace: ws, id, def }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Link {
    project: Option<String>,
}

/// An admin of the workspace, or the owner of the project, ties a channel to it.
async fn link_channel(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>, Json(b): Json<Link>) -> Reply {
    let (p, role) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    if !w.channels.contains_key(&channel) {
        return err(StatusCode::NOT_FOUND, "no such channel");
    }
    if let Some(id) = &b.project {
        let Some(def) = w.projects.get(id).filter(|d| d.may_read(&p.id)) else { return err(StatusCode::NOT_FOUND, "no such project") };
        if def.owner != p.id && role < Role::Admin {
            return err(StatusCode::FORBIDDEN, "the project's owner or an admin ties a channel to it");
        }
    } else if role < Role::Admin {
        return err(StatusCode::FORBIDDEN, "an admin unties a channel");
    }
    ok_or_return!(send(&k, &p, Change::ChannelProject { workspace: ws, channel, project: b.project }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Scope {
    #[serde(default)]
    project: Option<String>,
}

/// A base someone cannot read looks as if it does not exist.
async fn list(State(k): K, h: HeaderMap, Path(ws): Path<String>, Query(q): Query<Scope>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    if !w.may_read(q.project.as_deref(), &p.id) {
        return err(StatusCode::NOT_FOUND, "no such project");
    }
    Json(w.entries(q.project.as_deref())).into_response()
}

#[derive(Deserialize)]
struct NewEntry {
    /// `statement` is the name now; `text` is how a fact was first posted.
    #[serde(default, alias = "text")]
    statement: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    relations: Vec<Relation>,
}

fn fits(statement: &str) -> bool {
    !statement.trim().is_empty() && statement.chars().count() <= MAX_STATEMENT_CHARS
}

async fn add(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewEntry>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    if !fits(&b.statement) {
        return err(StatusCode::BAD_REQUEST, "an entry needs a statement, and not too long a one");
    }
    if !w.may_read(b.project.as_deref(), &p.id) {
        return err(StatusCode::NOT_FOUND, "no such project");
    }
    let kind = b.kind.unwrap_or_else(|| DEFAULT_KIND.to_string());
    if !w.allows_kind(b.project.as_deref(), &kind) || !b.relations.iter().all(|r| w.allows_relation(b.project.as_deref(), &r.kind)) {
        return err(StatusCode::BAD_REQUEST, "this project does not use that type or relation");
    }
    let id = new_id();
    let entry = Entry {
        id: id.clone(),
        project: b.project,
        kind,
        statement: b.statement,
        source: b.source.unwrap_or_else(|| format!("written by {}", p.id)),
        weight: Weight::Member,
        author: p.id.clone(),
        owner: p.id.clone(),
        thread: None,
        reviewed_at: chrono::Utc::now(),
        relations: b.relations,
        flags: vec![],
        retired: None,
    };
    ok_or_return!(send(&k, &p, Change::Entry { workspace: ws, id: id.clone(), entry }).await);
    Json(json!({ "id": id })).into_response()
}

/// The entry and its base, if this person may read it.
fn readable(k: &Kernel, ws: &str, id: &str, p: &Person) -> Option<(Workspace, Entry)> {
    let w = workspace(k, ws)?;
    let e = w.knowledge.get(id).cloned()?;
    w.may_read(e.project.as_deref(), &p.id).then_some((w, e))
}

macro_rules! curated {
    ($k:expr, $ws:expr, $id:expr, $p:expr) => {{
        let Some((w, e)) = readable(&$k, &$ws, &$id, &$p) else { return err(StatusCode::NOT_FOUND, "no such entry") };
        if !w.may_curate(e.project.as_deref(), &$p.id) {
            return err(StatusCode::FORBIDDEN, "the project's owner and curators keep its base");
        }
        (w, e)
    }};
}

#[derive(Deserialize)]
struct NewFlag {
    why: String,
}

/// Any member who can read an entry can say it looks wrong.
async fn flag(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>, Json(b): Json<NewFlag>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some((_, mut e)) = readable(&k, &ws, &id, &p) else { return err(StatusCode::NOT_FOUND, "no such entry") };
    if !fits(&b.why) {
        return err(StatusCode::BAD_REQUEST, "say why it looks wrong, briefly");
    }
    e.flags.push(Flag { by: p.id.clone(), why: b.why });
    ok_or_return!(send(&k, &p, Change::Entry { workspace: ws, id, entry: e }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Correction {
    #[serde(default)]
    statement: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    relations: Option<Vec<Relation>>,
}

/// Corrected and looked at just now, so its flags are answered.
async fn correct(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>, Json(b): Json<Correction>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let (w, mut e) = curated!(k, ws, id, p);
    if b.statement.as_deref().is_some_and(|t| !fits(t)) {
        return err(StatusCode::BAD_REQUEST, "that is not a statement this base can hold");
    }
    let project = e.project.clone();
    if b.kind.as_deref().is_some_and(|t| !w.allows_kind(project.as_deref(), t)) || b.relations.iter().flatten().any(|r| !w.allows_relation(project.as_deref(), &r.kind)) {
        return err(StatusCode::BAD_REQUEST, "this project does not use that type or relation");
    }
    e.statement = b.statement.unwrap_or(e.statement);
    e.kind = b.kind.unwrap_or(e.kind);
    e.relations = b.relations.unwrap_or(e.relations);
    e.flags.clear();
    e.reviewed_at = chrono::Utc::now();
    ok_or_return!(send(&k, &p, Change::Entry { workspace: ws, id, entry: e }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Retirement {
    #[serde(default)]
    superseded_by: Option<String>,
}

/// An entry superseded keeps its history, and the agent stops reading it.
async fn retire(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>, Json(b): Json<Retirement>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let (w, mut e) = curated!(k, ws, id, p);
    if let Some(new) = &b.superseded_by {
        let Some(mut by) = w.knowledge.get(new).filter(|n| n.project == e.project && n.id != id).cloned() else {
            return err(StatusCode::NOT_FOUND, "no such entry to supersede it");
        };
        by.relations.push(Relation { kind: "supersedes".into(), to: id.clone() });
        ok_or_return!(send(&k, &p, Change::Entry { workspace: ws.clone(), id: new.clone(), entry: by }).await);
    }
    e.retired = Some(Retired { by: p.id.clone(), at: chrono::Utc::now(), superseded_by: b.superseded_by });
    ok_or_return!(send(&k, &p, Change::Entry { workspace: ws, id, entry: e }).await);
    Json(json!({ "ok": true })).into_response()
}

/// Gone from the live base at once, and from the log and backups with it.
async fn remove(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    curated!(k, ws, id, p);
    ok_or_return!(send(&k, &p, Change::EntryRemoved { workspace: ws.clone(), id: id.clone() }).await);
    if let Err(e) = k.erase_entry(&format!("person:{}", p.id), &id).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"));
    }
    Json(json!({ "ok": true })).into_response()
}

/// Each base the person can read, as an ordinary file.
async fn export(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let mut open = w.clone();
    open.projects.retain(|_, d| d.may_read(&p.id));
    open.knowledge.retain(|_, e| e.project.as_ref().is_none_or(|id| open.projects.contains_key(id)));
    let files: Vec<Value> = open.export().into_iter().map(|(name, content)| json!({ "name": name, "content": content })).collect();
    Json(json!({ "files": files })).into_response()
}
