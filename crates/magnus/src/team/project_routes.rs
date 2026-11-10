//! Making things by typing their name, the task board, project files, the
//! right panel and invites. Nothing here is a step-by-step form.
use super::chat::{Chat, Invite, Origin, Status, Task};
use super::chat_routes::name_of;
use super::knowledge::Project;
use super::routes::{member, new_id, send, Reply, K};
use super::{Change, Role, Workspace};
use crate::api::{err, who};
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{bail, Result};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_NAME_CHARS: usize = 80;
const MAX_TASK_CHARS: usize = 500;
const INVITE_LIFETIME_DAYS: i64 = 7;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/start", post(start))
        .route("/v1/workspaces/{ws}/tasks", post(add_task))
        .route("/v1/workspaces/{ws}/tasks/{id}", put(change_task).delete(remove_task))
        .route("/v1/workspaces/{ws}/files/{name}/project", post(file_project))
        .route("/v1/workspaces/{ws}/panel", axum::routing::get(panel))
        .route("/v1/workspaces/{ws}/invites", post(invite))
        .route("/v1/invites/{code}/join", post(join))
}

/// A channel name from whatever was typed: "Launch site" is `launch-site`.
pub fn channel_name(typed: &str) -> String {
    let words: Vec<String> = typed.trim().trim_start_matches('#').split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    words.join("-")
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Channel,
}

impl Kernel {
    /// Makes a project, with its own channel, or a channel, from its name
    /// alone. The project's owner is the member it was made for.
    pub(super) async fn start_space(&self, actor: &str, ws: &str, by: &str, kind: Kind, name: &str) -> Result<Value> {
        let name = name.trim();
        let slug = channel_name(name);
        if slug.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            bail!("it needs a short name");
        }
        let channel = new_id();
        let emit = |c: Change| self.emit(actor, Ev::Team(c));
        emit(Change::Channel { workspace: ws.into(), channel: channel.clone(), name: slug }).await?;
        if kind == Kind::Channel {
            return Ok(json!({ "channel": channel }));
        }
        let id = new_id();
        let def = Project { id: id.clone(), name: name.into(), owner: by.into(), members: vec![by.into()], ..Default::default() };
        emit(Change::Project { workspace: ws.into(), id: id.clone(), def }).await?;
        emit(Change::ChannelProject { workspace: ws.into(), channel: channel.clone(), project: Some(id.clone()) }).await?;
        Ok(json!({ "project": id, "channel": channel }))
    }
}

#[derive(Deserialize)]
struct Start {
    kind: Kind,
    name: String,
}

async fn start(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<Start>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    match k.start_space(&format!("person:{}", p.id), &ws, &p.id, b.kind, &b.name).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

fn workspace(k: &Kernel, ws: &str) -> Option<Workspace> {
    k.read(|s| s.team.workspace(ws).cloned())
}

#[derive(Deserialize)]
struct NewTask {
    title: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    status: Status,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    from: Option<Origin>,
}

/// Whether a task's fields fit this workspace for this person.
fn task_fits(w: &Workspace, person: &str, t: &Task) -> Result<(), &'static str> {
    if t.title.trim().is_empty() || t.title.chars().count() > MAX_TASK_CHARS {
        return Err("a task needs a short title");
    }
    if !w.in_project(t.project.as_deref(), person) || t.project.as_ref().is_some_and(|p| !w.projects.contains_key(p)) {
        return Err("no such project");
    }
    if t.assignee.as_ref().is_some_and(|a| !w.members.contains_key(a) && a != super::conversation::AGENT_ID) {
        return Err("a task is assigned to a member or the team agent");
    }
    if t.from.as_ref().is_some_and(|o| !w.sees(&o.channel, person)) {
        return Err("no such channel");
    }
    Ok(())
}

async fn add_task(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewTask>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let task = Task { id: new_id(), project: b.project, title: b.title, status: b.status, assignee: b.assignee, from: b.from, by: p.id.clone(), at: Some(chrono::Utc::now()) };
    if let Err(why) = task_fits(&w, &p.id, &task) {
        return err(StatusCode::BAD_REQUEST, why);
    }
    let id = task.id.clone();
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Task { task } }).await);
    Json(json!({ "id": id })).into_response()
}

#[derive(Deserialize)]
struct TaskChange {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<Status>,
    /// `""` leaves it with nobody.
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

async fn change_task(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>, Json(b): Json<TaskChange>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let Some(mut task) = w.chat.tasks.get(&id).filter(|t| w.in_project(t.project.as_deref(), &p.id)).cloned() else { return err(StatusCode::NOT_FOUND, "no such task") };
    task.title = b.title.unwrap_or(task.title);
    task.status = b.status.unwrap_or(task.status);
    if let Some(a) = b.assignee {
        task.assignee = (!a.is_empty()).then_some(a);
    }
    if let Some(project) = b.project {
        task.project = (!project.is_empty()).then_some(project);
    }
    if let Err(why) = task_fits(&w, &p.id, &task) {
        return err(StatusCode::BAD_REQUEST, why);
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Task { task } }).await);
    Json(json!({ "ok": true })).into_response()
}

async fn remove_task(State(k): K, h: HeaderMap, Path((ws, id)): Path<(String, String)>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let known = k.read(|s| s.team.workspace(&ws).is_some_and(|w| w.chat.tasks.get(&id).is_some_and(|t| w.in_project(t.project.as_deref(), &p.id))));
    if !known {
        return err(StatusCode::NOT_FOUND, "no such task");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::TaskRemoved { id } }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct FileProject {
    project: Option<String>,
}

async fn file_project(State(k): K, h: HeaderMap, Path((ws, name)): Path<(String, String)>, Json(b): Json<FileProject>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Member).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    if !w.files.contains_key(&name) {
        return err(StatusCode::NOT_FOUND, "no such file");
    }
    if b.project.as_ref().is_some_and(|id| w.projects.get(id).is_none_or(|d| !d.may_read(&p.id))) {
        return err(StatusCode::NOT_FOUND, "no such project");
    }
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::FileIn { name, project: b.project } }).await);
    Json(json!({ "ok": true })).into_response()
}

/// The right panel: a project's files, tasks, members, channels and the
/// agent's current work there, or the whole workspace's with no project.
async fn panel(State(k): K, h: HeaderMap, Path(ws): Path<String>, Query(q): Query<HashMap<String, String>>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let Some(w) = workspace(&k, &ws) else { return err(StatusCode::NOT_FOUND, "no such workspace") };
    let project = q.get("project").filter(|s| !s.is_empty()).cloned();
    let def = match &project {
        Some(id) => match w.projects.get(id).filter(|d| d.may_read(&p.id)) {
            Some(d) => Some(d.clone()),
            None => return err(StatusCode::NOT_FOUND, "no such project"),
        },
        None => None,
    };
    let here = |pr: Option<&String>| project.is_none() || pr == project.as_ref();
    let channels: Vec<&String> = w.channels.iter().filter(|(id, c)| w.sees(id, &p.id) && here(c.project.as_ref())).map(|(id, _)| id).collect();
    let files: Vec<Value> = w
        .files
        .values()
        .filter(|f| here(w.chat.file_projects.get(&f.name)))
        .map(|f| json!({ "name": f.name, "size": f.size, "by": f.by, "by_name": name_of(&k, &f.by), "project": w.chat.file_projects.get(&f.name) }))
        .collect();
    let tasks: Vec<&Task> = w.chat.tasks.values().filter(|t| w.in_project(t.project.as_deref(), &p.id) && here(t.project.as_ref())).collect();
    let work: Vec<Value> = k.read(|s| {
        s.team
            .runs_of(&ws)
            .into_iter()
            .filter(|r| !r.sub && channels.contains(&&r.channel))
            .filter_map(|r| s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).filter(|run| !run.status.terminal()).map(|run| (r, run)))
            .map(|(r, run)| json!({ "task": r.task, "channel": r.channel, "requester": r.requester, "requester_name": name_of(&k, &r.requester), "waiting": run.status == crate::state::Status::Suspended, "steps": run.replies.len() }))
            .collect()
    });
    let people: Vec<Value> = match &def {
        Some(d) => std::iter::once(&d.owner)
            .chain(d.members.iter())
            .chain(d.curators.iter())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|id| json!({ "id": id, "name": name_of(&k, id) }))
            .collect(),
        None => w.members.keys().map(|id| json!({ "id": id, "name": name_of(&k, id) })).collect(),
    };
    let knowledge = w.entries(project.as_deref()).len();
    let progress = project.as_deref().map(|id| w.progress(id));
    Json(json!({
        "project": def, "channels": channels, "files": files, "tasks": tasks, "work": work, "members": people,
        "knowledge": knowledge, "progress": progress.map(|(d, t)| json!({ "done": d, "total": t })), "computer": k.team_computer(&ws),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct NewInvite {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    role: Option<Role>,
}

/// By email: someone the server already knows joins at once; anyone else,
/// and anyone with a link, joins with the code.
async fn invite(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<NewInvite>) -> Reply {
    let (p, mine) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let role = b.role.unwrap_or(Role::Member);
    if role > mine {
        return err(StatusCode::FORBIDDEN, "you cannot give a role above your own");
    }
    let email = b.email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    if let Some(e) = &email {
        if !e.contains('@') {
            return err(StatusCode::BAD_REQUEST, "that is not an email address");
        }
        let known = k.accounts.local_people().into_iter().find(|(_, lp)| lp.email.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(e)));
        if let Some((person, _)) = known {
            ok_or_return!(send(&k, &p, Change::Member { workspace: ws, person: person.clone(), role }).await);
            return Json(json!({ "added": person })).into_response();
        }
    }
    let invite = Invite { code: new_id(), role: Some(role), email, by: p.id.clone(), expires: Some(chrono::Utc::now() + chrono::Duration::days(INVITE_LIFETIME_DAYS)) };
    let code = invite.code.clone();
    ok_or_return!(send(&k, &p, Change::Chat { workspace: ws, chat: Chat::Invite { invite } }).await);
    Json(json!({ "code": code })).into_response()
}

/// Whoever is signed in and holds a live code joins. A code for one
/// address is used once; a link works until it expires.
async fn join(State(k): K, h: HeaderMap, Path(code): Path<String>) -> Reply {
    let p = ok_or_return!(who(&k, &h).await.map_err(Box::new));
    let now = chrono::Utc::now();
    let found = k.read(|s| s.team.all().find_map(|w| w.invite(&code, now).map(|i| (w.id.clone(), i.clone(), w.members.get(&p.id).copied()))));
    let Some((ws, inv, already)) = found else { return err(StatusCode::NOT_FOUND, "that invite has ended") };
    if already.is_none() {
        ok_or_return!(send(&k, &p, Change::Member { workspace: ws.clone(), person: p.id.clone(), role: inv.role.unwrap_or(Role::Member) }).await);
    }
    if inv.email.is_some() {
        ok_or_return!(send(&k, &p, Change::Chat { workspace: ws.clone(), chat: Chat::InviteEnded { code } }).await);
    }
    Json(json!({ "workspace": ws })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_is_named_from_what_was_typed() {
        assert_eq!(channel_name("Launch site"), "launch-site");
        assert_eq!(channel_name("#Alpha  testers!"), "alpha-testers");
        assert_eq!(channel_name("  ?? "), "");
    }

    #[test]
    fn a_task_belongs_to_a_project_its_maker_is_in() {
        let mut w = Workspace::default();
        w.members.insert("ann".into(), Role::Member);
        w.members.insert("bo".into(), Role::Member);
        w.projects.insert("p".into(), Project { id: "p".into(), owner: "ann".into(), ..Default::default() });
        let t = Task { title: "Fix it".into(), project: Some("p".into()), ..Default::default() };
        assert!(task_fits(&w, "ann", &t).is_ok());
        assert!(task_fits(&w, "bo", &t).is_err(), "bo is not in the project");
        let to_agent = Task { assignee: Some("agent".into()), project: None, ..t.clone() };
        assert!(task_fits(&w, "bo", &to_agent).is_ok());
        assert!(task_fits(&w, "ann", &Task { title: " ".into(), ..t }).is_err());
    }
}
