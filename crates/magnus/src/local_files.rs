//! The files Magnus wrote on a person's own computer. Each is listed in the
//! app beside that computer, where the person can download or delete it, one
//! at a time or many at once; a run is told of one the person deleted.
use crate::api::{err, who};
use crate::config::Person;
use crate::kernel::Kernel;
use crate::local::{mine, online, run_on, Computer, BY_PERSON};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Deletions kept for telling runs, per person.
const MAX_DELETED_FILES: usize = 200;
/// How long a run is still told of a deletion.
const TELL_DAYS: i64 = 30;
/// What the machine says when the file is already gone.
const NOT_THERE: &str = "No such file";

/// What happened to a file Magnus wrote on someone's own computer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Written { computer: String, person: String, path: String, at: DateTime<Utc> },
    Deleted { computer: String, person: String, path: String, at: DateTime<Utc>, by_person: bool },
}

impl Change {
    /// What a finished local tool call changed, if it made or removed a file.
    pub(crate) fn of_tool(tool: &str, c: &Computer, path: &str) -> Option<Change> {
        let (computer, person, path, at) = (c.id.clone(), c.person.clone(), path.to_string(), Utc::now());
        match tool {
            "local_write_file" => Some(Change::Written { computer, person, path, at }),
            "local_delete" => Some(Change::Deleted { computer, person, path, at, by_person: false }),
            _ => None,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Made {
    pub path: String,
    pub at: DateTime<Utc>,
}

#[derive(Default, Clone, Debug)]
pub struct Files {
    made: BTreeMap<String, BTreeMap<String, DateTime<Utc>>>,
    deleted: BTreeMap<String, Vec<(String, DateTime<Utc>)>>,
}

impl Files {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Written { computer, path, at, .. } => {
                self.made.entry(computer.clone()).or_default().insert(path.clone(), *at);
            }
            Change::Deleted { computer, person, path, at, by_person } => {
                if let Some(m) = self.made.get_mut(computer) {
                    m.remove(path);
                }
                if *by_person {
                    let gone = self.deleted.entry(person.clone()).or_default();
                    gone.push((path.clone(), *at));
                    let over = gone.len().saturating_sub(MAX_DELETED_FILES);
                    gone.drain(..over);
                }
            }
        }
    }

    /// Newest first.
    pub fn of(&self, computer: &str) -> Vec<Made> {
        let mut v: Vec<Made> = self.made.get(computer).into_iter().flatten().map(|(path, at)| Made { path: path.clone(), at: *at }).collect();
        v.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.path.cmp(&b.path)));
        v
    }

    fn holds(&self, computer: &str, path: &str) -> bool {
        self.made.get(computer).is_some_and(|m| m.contains_key(path))
    }

    /// What a run starting at `at` must be told the person deleted. Fixed by
    /// the run's start, so a replay says the same.
    pub fn deletion_section(&self, person: &str, at: DateTime<Utc>) -> String {
        let oldest = at - Duration::days(TELL_DAYS);
        let gone: Vec<String> = self.deleted.get(person).into_iter().flatten().filter(|(_, t)| *t < at && *t >= oldest).map(|(p, _)| format!("- {p}\n")).collect();
        if gone.is_empty() {
            return String::new();
        }
        format!("\n\nThese files you made on the person's own computer were deleted by them. They are gone: do not rely on them or recreate them.\n{}", gone.concat())
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/local/computers/{id}/files", get(list).delete(remove)).route("/v1/local/computers/{id}/files/raw", get(raw))
}

type K = State<Arc<Kernel>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePath {
    path: String,
}

/// The computer and the file, when it is one Magnus made there.
fn made(k: &Kernel, p: &Person, id: &str, path: &str) -> Result<Computer, (StatusCode, String)> {
    let c = mine(k, &p.id, id).ok_or_else(|| (StatusCode::NOT_FOUND, "no such computer".to_string()))?;
    if !k.read(|s| s.local.files.holds(id, path)) {
        return Err((StatusCode::NOT_FOUND, "Magnus made no such file there".into()));
    }
    if !online(id) {
        return Err((StatusCode::CONFLICT, format!("{} is offline. Open the Magnus app on it to reach its files.", c.name)));
    }
    Ok(c)
}

/// The person's own job on their machine: it goes even while they are at it.
async fn on_machine(c: &Computer, op: &str, path: &str) -> Result<String, (StatusCode, String)> {
    match run_on(&c.id, json!({ "op": op, "path": path, BY_PERSON: true })).await {
        (Some((true, text)), _) => Ok(text),
        (Some((false, why)), _) => Err((StatusCode::BAD_GATEWAY, why)),
        (None, _) => Err((StatusCode::GATEWAY_TIMEOUT, format!("{} did not answer", c.name))),
    }
}

async fn list(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    if mine(&k, &p.id, &id).is_none() {
        return err(StatusCode::NOT_FOUND, "no such computer");
    }
    Json(json!({ "files": k.read(|s| s.local.files.of(&id)) })).into_response()
}

async fn raw(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let c = match made(&k, &p, &id, &q.path) {
        Ok(c) => c,
        Err((status, why)) => return err(status, why),
    };
    match on_machine(&c, "read", &q.path).await {
        Ok(text) => ([(header::CONTENT_TYPE, "application/octet-stream")], text).into_response(),
        Err((status, why)) => err(status, why),
    }
}

async fn remove(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<FilePath>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let c = match made(&k, &p, &id, &q.path) {
        Ok(c) => c,
        Err((status, why)) => return err(status, why),
    };
    // Already gone from the machine: it leaves the list all the same.
    if let Err((status, why)) = on_machine(&c, "delete", &q.path).await {
        if !why.contains(NOT_THERE) {
            return err(status, why);
        }
    }
    let gone = Change::Deleted { computer: id, person: p.id.clone(), path: q.path.clone(), at: Utc::now(), by_person: true };
    match k.emit(&format!("person:{}", p.id), crate::events::Ev::LocalFile(gone)).await {
        Ok(_) => Json(json!({ "deleted": q.path })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer() -> Computer {
        Computer { id: "c1".into(), person: "alice".into(), name: "laptop".into(), folders: vec!["/home/alice/work".into()], mode: crate::local::Mode::Allow, removed: false }
    }

    #[test]
    fn a_file_the_agent_wrote_is_listed_until_it_is_deleted() {
        let mut f = Files::default();
        let c = computer();
        f.apply(&Change::of_tool("local_write_file", &c, "/home/alice/work/report.md").unwrap());
        f.apply(&Change::of_tool("local_write_file", &c, "/home/alice/work/data.csv").unwrap());
        assert!(Change::of_tool("local_read_file", &c, "/home/alice/work/x").is_none(), "a read made nothing");
        assert_eq!(f.of("c1").len(), 2);
        assert!(f.holds("c1", "/home/alice/work/report.md"));
        assert!(!f.holds("c2", "/home/alice/work/report.md"), "another computer's list");
        f.apply(&Change::of_tool("local_delete", &c, "/home/alice/work/report.md").unwrap());
        assert_eq!(f.of("c1").iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["/home/alice/work/data.csv"]);
        assert_eq!(f.deletion_section("alice", Utc::now() + Duration::seconds(1)), "", "the agent's own deletion is not news to it");
    }

    #[test]
    fn a_run_is_told_of_a_file_the_person_deleted() {
        let mut f = Files::default();
        let t = Utc::now();
        f.apply(&Change::Written { computer: "c1".into(), person: "alice".into(), path: "/w/x".into(), at: t });
        f.apply(&Change::Deleted { computer: "c1".into(), person: "alice".into(), path: "/w/x".into(), at: t, by_person: true });
        assert!(f.of("c1").is_empty());
        assert!(f.deletion_section("alice", t + Duration::seconds(1)).contains("/w/x"));
        assert_eq!(f.deletion_section("alice", t - Duration::seconds(1)), "", "a run was told of a later deletion");
        assert_eq!(f.deletion_section("bob", t + Duration::seconds(1)), "", "another person was told");
    }

    #[test]
    fn the_change_reads_back_as_written() {
        let c = Change::Deleted { computer: "c1".into(), person: "a".into(), path: "/w/x".into(), at: Utc::now(), by_person: true };
        let ev = crate::events::Ev::LocalFile(c.clone());
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "local_file");
        assert_eq!(v["change"], "deleted");
        assert!(matches!(serde_json::from_value(v).unwrap(), crate::events::Ev::LocalFile(back) if back == c));
    }
}
