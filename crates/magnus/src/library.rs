//! The library and projects. The library is every file a person uploaded
//! and every file Magnus delivered to them, each linked to the task or chat
//! it came from; it is derived from the log. A project groups chats, tasks,
//! schedules and files, and holds the person's instructions for everything
//! in it and its reference files. Only the person writes a project (I1).

use crate::api::{err, internal, who};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::{LibFile, State as St};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

type K = State<Arc<Kernel>>;

/// The largest file a person may upload: the same as a delivered file.
pub const MAX_FILE: usize = 25 * 1024 * 1024;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        // Base64 makes a file a third larger; the rest is the request's fields.
        .route("/v1/files", get(list).post(upload).layer(DefaultBodyLimit::max(MAX_FILE / 3 * 4 + 64 * 1024)))
        .route("/v1/files/{id}", get(download))
        .route("/v1/files/{id}/info", get(info))
        .route("/v1/projects", get(projects).post(new_project))
        .route("/v1/projects/{id}", get(project).put(set_project))
        .route("/v1/moves", post(move_item))
}

fn actor(p: &Person) -> String {
    format!("person:{}", p.id)
}

/// Where a file came from: the task that delivered it, the chat it was
/// uploaded in or given to, the task it was given to, or the library.
/// `w` holds the recalled words of the person's tasks.
fn origin(s: &St, f: &LibFile, w: &crate::recall::Words) -> Value {
    let task_ref = |task: &str| {
        let t = s.tasks.get(task)?;
        let r = s.runs.get(&t.run_id)?;
        Some(match t.chat.as_ref() {
            Some(c) if c.turn => json!({ "kind": "chat", "thread": c.thread, "task_id": t.task_id, "title": crate::api::title_of(t, Some(r), w) }),
            c => json!({ "kind": "task", "task_id": t.task_id, "title": crate::api::title_of(t, Some(r), w), "thread": c.map(|c| c.thread.clone()) }),
        })
    };
    if let Some(v) = f.run.as_ref().and_then(|r| s.runs.get(r)).and_then(|r| task_ref(&r.task)) {
        return v;
    }
    if let Some(t) = f.thread.as_ref() {
        let title = s
            .tasks
            .values()
            .filter(|t2| t2.chat.as_ref().is_some_and(|c| &c.thread == t && c.turn))
            .min_by_key(|t2| t2.created_at)
            .map(|t2| w.text(t2.seq).chars().take(80).collect::<String>());
        return json!({ "kind": "chat", "thread": t, "title": title });
    }
    if let Some(v) = s.tasks.values().filter(|t| t.files.contains(&f.id)).min_by_key(|t| t.created_at).and_then(|t| task_ref(&t.task_id)) {
        return v;
    }
    json!({ "kind": "library" })
}

pub(crate) fn project_ref(s: &St, item: &str) -> Value {
    s.project_of(item).and_then(|p| s.projects.get(&p)).map_or(Value::Null, |p| json!({ "id": p.id, "name": p.name }))
}

/// `w` holds the recalled words of the file's person's tasks
/// (`Kernel::person_words`), for where it came from.
pub(crate) fn file_json(s: &St, f: &LibFile, w: &crate::recall::Words) -> Value {
    json!({
        "id": f.id, "name": f.name, "media_type": f.media_type, "size": f.size, "at": f.at,
        "source": if f.run.is_some() { "delivered" } else { "uploaded" },
        "origin": origin(s, f, w),
        "project": project_ref(s, &f.id),
        "taint": { "untrusted": f.label.untrusted, "private": f.label.private },
    })
}

/// The person's library, newest first. `q` matches every word against the
/// name, where it came from, its project and a text file's content;
/// `project` keeps one project's files (`none`: those in no project).
async fn list(State(k): K, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let terms = crate::api::terms(q.get("q").map(String::as_str).unwrap_or_default());
    let texts = match k.file_texts(&p.id, &terms).await {
        Ok(t) => t,
        Err(e) => return internal(e),
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let files = k.read(|s| files_matching(s, &p, &terms, &texts, q.get("project").map(String::as_str), &words));
    Json(json!({ "files": files })).into_response()
}

/// The person's files with every term in their name, origin, project or
/// text, newest first; `want` keeps one project's (`none`: in no project).
/// `texts` holds the search text of their text files, recalled for them
/// (`Kernel::file_texts`).
pub(crate) fn files_matching(s: &St, p: &Person, terms: &[String], texts: &HashMap<String, String>, want: Option<&str>, w: &crate::recall::Words) -> Vec<Value> {
    let mut v: Vec<(&LibFile, Value)> = s
        .files
        .values()
        .filter(|f| f.person == p.id)
        .map(|f| (f, file_json(s, f, w)))
        .filter(|(_, j)| match want {
            None => true,
            Some("none") => j["project"].is_null(),
            Some(id) => j["project"]["id"] == id,
        })
        .filter(|(f, j)| {
            let text = texts.get(&f.id).map(String::as_str).unwrap_or_default();
            let hay = format!(
                "{} {} {} {}",
                f.name.to_lowercase(),
                j["origin"]["title"].as_str().unwrap_or_default().to_lowercase(),
                j["project"]["name"].as_str().unwrap_or_default().to_lowercase(),
                text
            );
            terms.iter().all(|t| hay.contains(t.as_str()))
        })
        .collect();
    v.sort_by(|a, b| b.0.at.cmp(&a.0.at));
    v.into_iter().map(|(_, j)| j).collect()
}

/// Add a file: to the library, into a project of the person's, or in a
/// chat thread of theirs (then it is in that thread's project).
async fn upload(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let thread = b["thread"].as_str().map(String::from);
    if let Some(t) = &thread {
        if !k.has_thread(&p.id, t) {
            return err(StatusCode::NOT_FOUND, "no such thread");
        }
    }
    let project = b["project"].as_str().map(String::from);
    if let Some(id) = &project {
        if !k.read(|s| s.projects.get(id).is_some_and(|x| x.person == p.id)) {
            return err(StatusCode::NOT_FOUND, "no such project");
        }
    }
    match add_file(&k, &p, &b, thread).await {
        Ok(Ok(id)) => {
            if let Some(project) = project {
                if let Err(e) = k.emit(&actor(&p), Ev::Moved { item: id.clone(), project: Some(project) }).await {
                    return internal(e);
                }
            }
            let words = k.person_words(&p.id).await.unwrap_or_default();
            let v = k.read(|s| s.files.get(&id).map(|f| file_json(s, f, &words)));
            (StatusCode::CREATED, Json(v.unwrap_or(json!({ "id": id })))).into_response()
        }
        Ok(Err(r)) => r,
        Err(e) => internal(e),
    }
}

/// Record one uploaded file ({name, media_type, data_b64}).
pub(crate) async fn add_file(k: &Kernel, p: &Person, b: &Value, thread: Option<String>) -> anyhow::Result<Result<String, Response>> {
    let (Some(name), Some(media_type), Some(data)) = (b["name"].as_str().map(str::trim).filter(|n| !n.is_empty()), b["media_type"].as_str(), b["data_b64"].as_str()) else {
        return Ok(Err(err(StatusCode::BAD_REQUEST, "a file needs name, media_type and data_b64")));
    };
    use base64::Engine;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { return Ok(Err(err(StatusCode::BAD_REQUEST, "data_b64 is not base64"))) };
    if bytes.is_empty() {
        return Ok(Err(err(StatusCode::BAD_REQUEST, "the file is empty")));
    }
    if bytes.len() > MAX_FILE {
        return Ok(Err(err(StatusCode::PAYLOAD_TOO_LARGE, format!("files are limited to {} MB each", MAX_FILE / 1024 / 1024))));
    }
    let id = new_id("file");
    k.emit(&actor(p), Ev::FileUploaded { id: id.clone(), person: p.id.clone(), name: name.into(), media_type: media_type.into(), data_b64: data.into(), thread }).await?;
    Ok(Ok(id))
}

/// The person's uploaded files named in a request (`files`), to give to a
/// task or a message.
pub(crate) fn given_files(k: &Kernel, p: &Person, b: &Value) -> Result<Vec<String>, Response> {
    let ids: Vec<String> = b["files"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    for id in &ids {
        match k.read(|s| s.files.get(id).filter(|f| f.person == p.id).map(|f| f.uploaded)) {
            None => return Err(err(StatusCode::NOT_FOUND, format!("no such file {id}"))),
            Some(false) => return Err(err(StatusCode::BAD_REQUEST, "only uploaded files can be given to a task or message; to use a delivered file, put it in a project")),
            Some(true) => {}
        }
    }
    Ok(ids)
}

/// A project of the person's named in a request (`project`), if any.
pub(crate) fn chosen_project(k: &Kernel, p: &Person, b: &Value) -> Result<Option<String>, Response> {
    let Some(id) = b["project"].as_str() else { return Ok(None) };
    if k.read(|s| s.projects.get(id).is_some_and(|x| x.person == p.id)) {
        Ok(Some(id.into()))
    } else {
        Err(err(StatusCode::NOT_FOUND, "no such project"))
    }
}

/// One file's entry, as the library lists it, for its own page.
async fn info(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    match k.read(|s| s.files.get(&id).filter(|f| f.person == p.id).map(|f| file_json(s, f, &words))) {
        Some(mut v) => {
            v["preview"] = json!({ "chars": crate::compose::PREVIEW_CHARS, "rows": crate::compose::MAX_ROWS });
            Json(v).into_response()
        }
        None => err(StatusCode::NOT_FOUND, "no such file"),
    }
}

async fn download(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(f) = k.read(|s| s.files.get(&id).filter(|f| f.person == p.id).cloned()) else { return err(StatusCode::NOT_FOUND, "no such file") };
    // Recalled for the person, through their wall.
    let bytes = match k.file_bytes(&f).await {
        Ok(Some(b)) => b,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such file"),
        Err(e) => return internal(e),
    };
    attachment(&f.name, &f.media_type, bytes)
}

/// A file to save, never to run in the app's origin: whatever its name and
/// stated type hold (control characters, quotes, non-ASCII letters), the
/// answer is well-formed. The name goes plain where it is safe ASCII and in
/// full as RFC 6266 `filename*`; a type that is not a valid header value is
/// sent as `application/octet-stream`; the browser may not sniff another.
pub fn attachment(name: &str, media_type: &str, bytes: Vec<u8>) -> Response {
    let plain: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
    let plain = if plain.trim().is_empty() { "file".to_string() } else { plain };
    let full: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    let disposition = format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{full}");
    let ctype = axum::http::HeaderValue::from_str(media_type).unwrap_or(axum::http::HeaderValue::from_static("application/octet-stream"));
    let mut r = Response::new(axum::body::Body::from(bytes));
    r.headers_mut().insert("content-type", ctype);
    if let Ok(v) = axum::http::HeaderValue::from_str(&disposition) {
        r.headers_mut().insert("content-disposition", v);
    }
    r.headers_mut().insert("x-content-type-options", axum::http::HeaderValue::from_static("nosniff"));
    r
}

fn project_json(s: &St, p: &crate::state::Project) -> Value {
    let inside = |item: &str| s.project_of(item).as_deref() == Some(p.id.as_str());
    json!({
        "id": p.id, "name": p.name, "instructions": p.instructions, "created_at": p.created_at,
        "counts": {
            "tasks": s.tasks.values().filter(|t| t.person == p.person && !t.is_chat_turn() && inside(&t.task_id)).count(),
            "files": s.files.values().filter(|f| f.person == p.person && inside(&f.id)).count(),
            "schedules": s.schedules.values().filter(|x| x.person == p.person && inside(&x.id)).count(),
        },
    })
}

async fn projects(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let v: Vec<Value> = k.read(|s| s.projects.values().filter(|x| x.person == p.id).map(|x| project_json(s, x)).collect());
    Json(json!({ "projects": v })).into_response()
}

fn instructions_of(b: &Value) -> Option<String> {
    b["instructions"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from)
}

async fn new_project(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(name) = b["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) else { return err(StatusCode::BAD_REQUEST, "the project needs a name") };
    let id = new_id("proj");
    match k.emit(&actor(&p), Ev::ProjectSet { id: id.clone(), person: p.id.clone(), name: name.into(), instructions: instructions_of(&b) }).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Err(e) => internal(e),
    }
}

/// A project with everything in it: chats, tasks, schedules and files.
async fn project(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(mut v) = k.read(|s| s.projects.get(&id).filter(|x| x.person == p.id).map(|x| project_json(s, x))) else { return err(StatusCode::NOT_FOUND, "no such project") };
    let threads: Vec<Value> = match k.threads(&p.id).await {
        Ok(t) => t.into_iter().filter(|t| t["project"]["id"] == id.as_str()).collect(),
        Err(e) => return internal(e),
    };
    let words = match k.person_words(&p.id).await {
        Ok(w) => w,
        Err(e) => return internal(e),
    };
    let (tasks, schedules, files) = k.read(|s| {
        let inside = |item: &str| s.project_of(item).as_deref() == Some(id.as_str());
        let tasks: Vec<Value> = s
            .tasks
            .values()
            .filter(|t| t.person == p.id && !t.is_chat_turn() && inside(&t.task_id))
            .filter_map(|t| s.runs.get(&t.run_id).map(|r| crate::api::task_row(&k, s, &p, t, r, &words)))
            .collect();
        let schedules: Vec<Value> = s
            .schedules
            .values()
            .filter(|x| x.person == p.id && inside(&x.id))
            .map(|x| json!({ "id": x.id, "title": words.title(x.seq).unwrap_or_else(|| words.text(x.seq).to_string()), "text": words.text(x.seq), "next": x.next_due() }))
            .collect();
        let mut files: Vec<&LibFile> = s.files.values().filter(|f| f.person == p.id && inside(&f.id)).collect();
        files.sort_by(|a, b| b.at.cmp(&a.at));
        (tasks, schedules, files.into_iter().map(|f| file_json(s, f, &words)).collect::<Vec<_>>())
    });
    v["threads"] = json!(threads);
    v["tasks"] = json!(tasks);
    v["schedules"] = json!(schedules);
    v["files"] = json!(files);
    Json(v).into_response()
}

/// The person renames a project or changes its instructions. Tasks already
/// running keep the instructions they started with.
async fn set_project(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(cur) = k.read(|s| s.projects.get(&id).filter(|x| x.person == p.id).cloned()) else { return err(StatusCode::NOT_FOUND, "no such project") };
    let name = match b.get("name") {
        Some(n) => match n.as_str().map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => return err(StatusCode::BAD_REQUEST, "the project needs a name"),
        },
        None => cur.name,
    };
    let instructions = if b.get("instructions").is_some() { instructions_of(&b) } else { cur.instructions };
    match k.emit(&actor(&p), Ev::ProjectSet { id: id.clone(), person: p.id.clone(), name, instructions }).await {
        Ok(_) => Json(json!({ "id": id })).into_response(),
        Err(e) => internal(e),
    }
}

/// Move a task, chat thread, schedule or file of the person's into one of
/// their projects, or out (`project` null).
async fn move_item(State(k): K, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let item = b["item"].as_str().unwrap_or_default().to_string();
    let project = match b["project"].as_str() {
        Some(id) if !k.read(|s| s.projects.get(id).is_some_and(|x| x.person == p.id)) => return err(StatusCode::NOT_FOUND, "no such project"),
        x => x.map(String::from),
    };
    let mine = k.read(|s| {
        s.tasks.get(&item).is_some_and(|t| t.person == p.id && !t.is_chat_turn())
            || s.schedules.get(&item).is_some_and(|x| x.person == p.id)
            || s.files.get(&item).is_some_and(|f| f.person == p.id)
    }) || k.has_thread(&p.id, &item);
    if !mine {
        return err(StatusCode::NOT_FOUND, "no such task, chat, schedule or file");
    }
    match k.emit(&actor(&p), Ev::Moved { item, project }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}
