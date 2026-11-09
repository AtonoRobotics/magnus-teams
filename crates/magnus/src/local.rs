//! The person's own computer. It connects from their machine and takes jobs
//! over a long poll, so with no connection there is nothing to run on. The
//! server refuses what it can tell is out of bounds; the machine refuses the
//! rest, because only the machine can see its symlinks.
use crate::api::{err, who};
use crate::boundary::{Action, Facts};
use crate::config::Person;
use crate::kernel::Kernel;
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Component, Path as FilePath};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Notify};

const NEXT_WAIT: Duration = Duration::from_secs(20);
/// A job is given up on after this; the machine limits a command to less.
const JOB_WAIT: Duration = Duration::from_secs(150);
/// The gap between two polls, within which a computer still counts as online.
const ONLINE_GRACE: Duration = Duration::from_secs(5);
const MAX_RESULT_BYTES: usize = 512 * 1024;
const MAX_NAME_CHARS: usize = 80;
const MAX_FOLDERS: usize = 32;
/// Set on an ask that only a person can answer, one action at a time.
const ASK_EVERY_TIME: &str = "kernel:local_ask";
const CHANGE_NEEDS_A_RULE: &str = "kernel:local_change";
const BOTH: Label = Label { untrusted: true, private: true };

pub const BUILTINS: &[Builtin] = &[
    Builtin { name: "local_read_file", app: "local", risk: Risk::Read, label: BOTH, audience: Audience::Private },
    Builtin { name: "local_write_file", app: "local", risk: Risk::ReversibleWrite, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "local_edit_file", app: "local", risk: Risk::Irreversible, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "local_delete", app: "local", risk: Risk::Irreversible, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "local_exec", app: "local", risk: Risk::Irreversible, label: BOTH, audience: Audience::Private },
];

/// Stricter comes first, so the lower of two modes is the one that holds.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Never,
    Ask,
    Allow,
}

/// `[local]`: the admin's ceiling on what any person's computer may be set to.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "no_ceiling")]
    pub ceiling: Mode,
}

fn no_ceiling() -> Mode {
    Mode::Allow
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { ceiling: no_ceiling() }
    }
}

/// A connected computer as the log has it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Computer {
    pub id: String,
    pub person: String,
    pub name: String,
    pub folders: Vec<String>,
    pub mode: Mode,
    #[serde(default)]
    pub removed: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    computers: BTreeMap<String, Computer>,
}

impl Ledger {
    pub fn apply(&mut self, c: &Computer) {
        if c.removed {
            self.computers.remove(&c.id);
        } else {
            self.computers.insert(c.id.clone(), c.clone());
        }
    }

    fn of(&self, person: &str) -> Vec<Computer> {
        self.computers.values().filter(|c| c.person == person).cloned().collect()
    }
}

#[derive(Default)]
struct Line {
    jobs: VecDeque<Value>,
    waiting: BTreeMap<String, oneshot::Sender<(bool, String)>>,
    polls: usize,
    seen: Option<Instant>,
    wake: Arc<Notify>,
}

static LINES: LazyLock<Mutex<BTreeMap<String, Line>>> = LazyLock::new(Default::default);

fn line<T>(id: &str, f: impl FnOnce(&mut Line) -> T) -> T {
    let mut all = LINES.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.entry(id.to_string()).or_default())
}

fn online(id: &str) -> bool {
    line(id, |l| l.polls > 0 || !l.waiting.is_empty() || l.seen.is_some_and(|t| t.elapsed() < ONLINE_GRACE))
}

/// Counts a poll in progress. Dropped when the machine's connection closes.
struct Polling(String);

impl Drop for Polling {
    fn drop(&mut self) {
        line(&self.0, |l| {
            l.polls = l.polls.saturating_sub(1);
            l.seen = Some(Instant::now());
        });
    }
}

/// Whether `path` is under a granted folder, written plainly. The machine
/// makes the check that matters.
fn within(folders: &[String], path: &str) -> bool {
    let p = FilePath::new(path);
    p.is_absolute() && !p.components().any(|c| matches!(c, Component::ParentDir)) && folders.iter().any(|f| p.starts_with(f))
}

fn chosen(mine: &[Computer], input: &Value) -> std::result::Result<Computer, String> {
    match (input["computer"].as_str(), mine) {
        (_, []) => Err("no computer is connected, so nothing can run on one".to_string()),
        (Some(name), _) => mine.iter().find(|c| c.name == name).cloned().ok_or_else(|| format!("no connected computer is called {name}")),
        (None, [one]) => Ok(one.clone()),
        (None, _) => Err(format!("name a computer: {}", mine.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Whether an action could carry data away. A local write stays on the
/// person's machine, and a team write stays with the requester if it read
/// their private data.
pub(crate) fn may_send(a: &Action, f: &Facts) -> bool {
    f.app != "local" && a.tool != "team_write_file" && !crate::boundary::is_chat_tool(f)
}

/// The folder an action touches, when it names one.
fn touched(a: &Action) -> Option<&str> {
    a.input["path"].as_str().or_else(|| a.input["cwd"].as_str())
}

impl Kernel {
    /// What the person's computer says about this action: `None` for no
    /// objection, an error to refuse it, or a reason to ask a person.
    pub(crate) fn local_gate(&self, a: &Action, f: &Facts, person: &Person, policy_asked: bool) -> Option<std::result::Result<(String, Option<String>), String>> {
        if f.app != "local" || f.connector.is_some() {
            return None;
        }
        let mine = self.read(|s| s.local.of(&person.id));
        let computer = match chosen(&mine, &a.input) {
            Ok(c) => c,
            Err(why) => return Some(Err(why)),
        };
        if !online(&computer.id) {
            return Some(Err(format!("{} is not online, so nothing can run on it now", computer.name)));
        }
        if touched(a).is_some_and(|p| !within(&computer.folders, p)) {
            return Some(Err(format!("that path is outside the folders {} granted", computer.name)));
        }
        match computer.mode.min(self.cfg.local.ceiling) {
            Mode::Never => Some(Err(format!("{} is set to never", computer.name))),
            Mode::Ask => Some(Ok((format!("{} asks before every action", computer.name), Some(ASK_EVERY_TIME.to_string())))),
            Mode::Allow if policy_asked => Some(Ok((format!("this changes something already on {}, and no rule allows it", computer.name), Some(CHANGE_NEEDS_A_RULE.to_string())))),
            Mode::Allow => None,
        }
    }

    /// The tools that reach past this server: the person's computer, the team.
    pub(crate) async fn far_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        if a.tool.starts_with("team_") {
            return self.team_tool(a).await;
        }
        self.local_tool(a).await
    }

    async fn local_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let person = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone())).context("no such run")?;
        let mine = self.read(|s| s.local.of(&person));
        let computer = match chosen(&mine, &a.input) {
            Ok(c) => c,
            Err(why) => return Ok((why, true, None)),
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut job = a.input.clone();
        job["id"] = json!(id);
        job["op"] = json!(a.tool.trim_start_matches("local_").trim_end_matches("_file"));
        let (done, answer) = oneshot::channel();
        let wake = line(&computer.id, |l| {
            l.waiting.insert(id.clone(), done);
            l.jobs.push_back(job);
            l.wake.clone()
        });
        wake.notify_one();
        let result = tokio::time::timeout(JOB_WAIT, answer).await;
        line(&computer.id, |l| l.waiting.remove(&id));
        Ok(match result {
            Ok(Ok((ok, text))) => (text, !ok, None),
            _ => (format!("{} did not answer, so this may or may not have happened", computer.name), true, None),
        })
    }

    pub(crate) fn local_specs(&self, person: &str) -> Vec<ToolSpec> {
        if self.read(|s| s.local.of(person)).is_empty() {
            return vec![];
        }
        let tool = |name: &str, description: &str, props: Value, needs: &[&str]| ToolSpec { name: name.into(), description: description.into(), input_schema: obj(props, needs) };
        let path = json!({ "type": "string", "description": "an absolute path inside a granted folder" });
        let text = json!({ "type": "string" });
        let on = json!({ "type": "string", "description": "which computer, when there is more than one" });
        vec![
            tool("local_read_file", "Read a file on the person's own computer, inside a folder they granted.", json!({ "path": path, "computer": on }), &["path"]),
            tool(
                "local_write_file",
                "Create a new file on the person's own computer. It never overwrites.",
                json!({ "path": path, "content": text, "computer": on }),
                &["path", "content"],
            ),
            tool(
                "local_edit_file",
                "Replace a file on the person's own computer. The person is asked unless a rule allows it.",
                json!({ "path": path, "content": text, "computer": on }),
                &["path", "content"],
            ),
            tool(
                "local_delete",
                "Delete a file or empty folder on the person's own computer. The person is asked unless a rule allows it.",
                json!({ "path": path, "computer": on }),
                &["path"],
            ),
            tool(
                "local_exec",
                "Run a command on the person's own computer. It sees the granted folders and has no network. The person is asked unless a rule allows it.",
                json!({ "command": text, "cwd": path, "computer": on }),
                &["command"],
            ),
        ]
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/local/computers", get(list).post(connect))
        .route("/v1/local/computers/{id}", delete(disconnect))
        .route("/v1/local/computers/{id}/next", get(next))
        .route("/v1/local/computers/{id}/result", post(result))
}

type K = State<Arc<Kernel>>;

#[derive(Deserialize)]
struct Connect {
    name: String,
    folders: Vec<String>,
    mode: Mode,
}

fn mine(k: &Kernel, person: &str, id: &str) -> Option<Computer> {
    k.read(|s| s.local.of(person)).into_iter().find(|c| c.id == id)
}

async fn list(State(k): K, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let view: Vec<Value> = k.read(|s| s.local.of(&p.id)).into_iter().map(|c| json!({ "online": online(&c.id), "computer": c })).collect();
    Json(view).into_response()
}

async fn connect(State(k): K, h: HeaderMap, Json(b): Json<Connect>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if b.name.trim().is_empty() || b.name.chars().count() > MAX_NAME_CHARS {
        return err(StatusCode::BAD_REQUEST, "a computer needs a short name");
    }
    if b.folders.is_empty() || b.folders.len() > MAX_FOLDERS || b.folders.iter().any(|f| !FilePath::new(f).is_absolute()) {
        return err(StatusCode::BAD_REQUEST, "grant between one and 32 absolute folders");
    }
    let known = k.read(|s| s.local.of(&p.id)).into_iter().find(|c| c.name == b.name).map(|c| c.id);
    let id = known.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let c = Computer { id: id.clone(), person: p.id.clone(), name: b.name, folders: b.folders, mode: b.mode, removed: false };
    match k.emit(&format!("person:{}", p.id), crate::events::Ev::LocalComputer(c)).await {
        Ok(_) => Json(json!({ "id": id })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

async fn disconnect(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(mut c) = mine(&k, &p.id, &id) else { return err(StatusCode::NOT_FOUND, "no such computer") };
    c.removed = true;
    match k.emit(&format!("person:{}", p.id), crate::events::Ev::LocalComputer(c)).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

async fn next(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    line(&id, |l| l.polls += 1);
    let _polling = Polling(id.clone());
    let until = Instant::now() + NEXT_WAIT;
    loop {
        let (job, wake) = line(&id, |l| (l.jobs.pop_front(), l.wake.clone()));
        if let Some(job) = job {
            return Json(job).into_response();
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || tokio::time::timeout(left, wake.notified()).await.is_err() {
            return StatusCode::NO_CONTENT.into_response();
        }
    }
}

#[derive(Deserialize)]
struct Done {
    job: String,
    ok: bool,
    text: String,
}

async fn result(State(k): K, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Done>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    let text: String = b.text.chars().take(MAX_RESULT_BYTES).collect();
    if let Some(waiter) = line(&id, |l| l.waiting.remove(&b.job)) {
        // The run may have stopped waiting; then there is no one to tell.
        let _ = waiter.send((b.ok, text));
    }
    Json(json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Computer {
        Computer { id: name.into(), person: "a".into(), name: name.into(), folders: vec!["/home/a/work".into()], mode: Mode::Allow, removed: false }
    }

    #[test]
    fn the_stricter_mode_is_the_lower_one() {
        assert_eq!(Mode::Allow.min(Mode::Ask), Mode::Ask);
        assert_eq!(Mode::Ask.min(Mode::Never), Mode::Never);
    }

    #[test]
    fn a_plain_path_outside_is_caught_here() {
        let f = c("x").folders;
        assert!(within(&f, "/home/a/work/notes.txt"));
        for p in ["/home/a/workshop/x", "/home/a/work/../other", "/etc/passwd", "relative"] {
            assert!(!within(&f, p), "{p}");
        }
    }

    #[test]
    fn with_no_computer_there_is_nothing_to_choose() {
        assert!(chosen(&[], &json!({})).is_err());
        assert_eq!(chosen(&[c("x")], &json!({})).unwrap().name, "x");
        assert!(chosen(&[c("x"), c("y")], &json!({})).is_err());
        assert_eq!(chosen(&[c("x"), c("y")], &json!({ "computer": "y" })).unwrap().name, "y");
    }

    #[test]
    fn removing_a_computer_forgets_it() {
        let mut l = Ledger::default();
        let mut x = c("x");
        l.apply(&x);
        assert_eq!(l.of("a").len(), 1);
        x.removed = true;
        l.apply(&x);
        assert!(l.of("a").is_empty());
    }
}
